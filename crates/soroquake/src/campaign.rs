//! The campaign loop: explore random call sequences, check every invariant
//! after every step, shrink failures and report.

use std::collections::BTreeSet;
use std::fmt::Debug;
use std::time::{Duration, Instant};

use crate::check::Check;
use crate::config::Config;
use crate::lab::{LabAction, LabResult, LabStatus};
use crate::rng::SeededRng;

/// One applied call, as shown in the dashboard and the generated test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// Who made the call, e.g. `alice` or `admin`.
    pub who: String,
    /// The call with its arguments, e.g. `deposit(3)`.
    pub call: String,
    /// What happened, e.g. `minted 3 shares` or `rejected: InsufficientShares`.
    pub note: String,
    /// A label for the behaviour this call exercised, used for coverage,
    /// e.g. `withdraw:ok`. Declare every label in [`Harness::branches`].
    pub branch: String,
}

impl Step {
    pub fn new(who: impl Into<String>, call: impl Into<String>) -> Self {
        Step { who: who.into(), call: call.into(), note: String::new(), branch: String::new() }
    }
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = note.into();
        self
    }
    pub fn branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = branch.into();
        self
    }
}

/// A property that must hold after every step.
pub struct Invariant<S> {
    pub name: &'static str,
    /// Grouping shown in the dashboard, e.g. `vault` or `sep41`.
    pub pack: &'static str,
    pub check: fn(&S) -> Check,
}

impl<S> Invariant<S> {
    pub fn new(name: &'static str, pack: &'static str, check: fn(&S) -> Check) -> Self {
        Invariant { name, pack, check }
    }
}

/// Describes how to drive your contracts.
pub trait Harness {
    /// Your contracts plus any ghost variables the invariants need.
    type State;
    /// One call, as plain data so sequences can be replayed and shrunk.
    type Op: Clone + Debug;

    /// A fresh environment with contracts deployed. Must be deterministic.
    fn setup(&self) -> Self::State;
    /// Pick the next call. Use only `rng` for randomness.
    fn generate(&self, state: &Self::State, rng: &mut SeededRng) -> Self::Op;
    /// Apply the call. Contract errors are fine (use `try_` clients); report
    /// them in the returned step rather than panicking.
    fn apply(&self, state: &mut Self::State, op: &Self::Op) -> Step;
    fn invariants(&self) -> Vec<Invariant<Self::State>>;
    /// Every behaviour label `apply` can return. Coverage is the share seen.
    fn branches(&self) -> Vec<&'static str> {
        Vec::new()
    }
    /// Map a Replay lab action to a call on your contracts, or `None` if your
    /// contracts have no equivalent (cases using it are skipped).
    fn lab_op(&self, _state: &Self::State, _action: &LabAction) -> Option<Self::Op> {
        None
    }
}

/// A broken invariant, shrunk to the fewest calls that still break it.
#[derive(Debug, Clone)]
pub struct Failure {
    pub invariant: &'static str,
    pub pack: &'static str,
    pub reason: String,
    /// Seed of the sequence that first broke it, as hex.
    pub seed: String,
    pub steps_before: usize,
    pub sequence: Vec<Step>,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub seed: String,
    pub sequences: u64,
    pub coverage: f32,
    /// `(name, pack, held)` for every invariant.
    pub results: Vec<(&'static str, &'static str, bool)>,
    pub failures: Vec<Failure>,
    /// Replay lab results, one per built-in case.
    pub lab: Vec<LabResult>,
    pub elapsed: Duration,
}

/// Replay `ops` from a fresh state. Returns the steps and, if invariant `inv`
/// broke, the index of the step after which it broke and the reason.
pub(crate) fn replay_until<H: Harness>(h: &H, ops: &[H::Op], inv: &Invariant<H::State>) -> (Vec<Step>, Option<(usize, String)>) {
    let mut state = h.setup();
    let mut steps = Vec::with_capacity(ops.len());
    for (i, op) in ops.iter().enumerate() {
        steps.push(h.apply(&mut state, op));
        if let Check::Fail(reason) = (inv.check)(&state) {
            return (steps, Some((i, reason)));
        }
    }
    (steps, None)
}

/// Delta-debugging style shrink: drop chunks of calls while the invariant
/// still breaks, halving the chunk size until single calls.
pub(crate) fn shrink_until<H: Harness>(h: &H, mut ops: Vec<H::Op>, inv: &Invariant<H::State>) -> Vec<H::Op> {
    let mut chunk = (ops.len() / 2).max(1);
    loop {
        let mut i = 0;
        let mut removed = false;
        while i < ops.len() {
            let end = (i + chunk).min(ops.len());
            let candidate: Vec<H::Op> = ops[..i].iter().chain(ops[end..].iter()).cloned().collect();
            if !candidate.is_empty() {
                if let (_, Some((at, _))) = replay_until(h, &candidate, inv) {
                    ops = candidate[..=at].to_vec();
                    removed = true;
                    continue;
                }
            }
            i += chunk;
        }
        if chunk == 1 && !removed {
            return ops;
        }
        if !removed {
            chunk = (chunk / 2).max(1);
        }
    }
}

/// Run a campaign with settings from the environment (see [`Config`]).
/// Panics at the end if any invariant broke, so CI fails.
pub fn run<H: Harness>(harness: H) -> Report {
    run_with(harness, Config::from_env())
}

pub fn run_with<H: Harness>(h: H, cfg: Config) -> Report {
    let invariants = h.invariants();
    let branches: Vec<&'static str> = h.branches();
    let seed_hex = format!("{:#x}", cfg.seed);
    println!(
        "soroquake: shaking for {}s · seed {seed_hex} · {} invariants · {} calls per sequence",
        cfg.time.as_secs(),
        invariants.len(),
        cfg.max_steps
    );

    #[cfg(feature = "report")]
    let reporter = crate::report::Reporter::start(&cfg, &seed_hex, invariants.len());

    let started = Instant::now();

    // Replay lab first: a fifth of the budget, at most 20 seconds.
    let lab_budget = (cfg.time / 5).min(Duration::from_secs(20));
    let lab = crate::lab::run_lab(&h, &invariants, cfg.seed, lab_budget);
    for r in &lab {
        match r.status {
            LabStatus::Broke => println!("soroquake: lab ✗ {} broke {} ({} variants)", r.case.id, r.invariant.unwrap_or(""), r.variants),
            LabStatus::Held => println!("soroquake: lab ✓ {} held ({} variants)", r.case.id, r.variants),
            LabStatus::Skipped => println!("soroquake: lab · {} skipped (not applicable)", r.case.id),
        }
    }

    let deadline = started + cfg.time;
    let mut master = SeededRng::new(cfg.seed);
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut broken: Vec<Option<Failure>> = invariants.iter().map(|_| None).collect();
    let mut sequences: u64 = 0;
    let mut last_tick = Instant::now();
    let coverage = |seen: &BTreeSet<String>| -> f32 {
        if branches.is_empty() {
            return 0.0;
        }
        let hit = branches.iter().filter(|b| seen.contains(**b)).count();
        hit as f32 * 100.0 / branches.len() as f32
    };

    'campaign: while Instant::now() < deadline {
        let seq_seed = master.next_u64() & 0xffff_ffff;
        let mut rng = SeededRng::new(seq_seed);
        let mut state = h.setup();
        let mut ops: Vec<H::Op> = Vec::with_capacity(cfg.max_steps);
        for _ in 0..cfg.max_steps {
            let op = h.generate(&state, &mut rng);
            let step = h.apply(&mut state, &op);
            if !step.branch.is_empty() {
                seen.insert(step.branch.clone());
            }
            ops.push(op);
            let mut any_broke = false;
            for (k, inv) in invariants.iter().enumerate() {
                if broken[k].is_some() {
                    continue;
                }
                if let Check::Fail(_) = (inv.check)(&state) {
                    any_broke = true;
                    let before = ops.len();
                    let small = shrink_until(&h, ops.clone(), inv);
                    let (mut steps, fail) = replay_until(&h, &small, inv);
                    let reason = fail.map(|(_, r)| r).unwrap_or_default();
                    if let Some(last) = steps.last_mut() {
                        last.note = reason.clone();
                    }
                    println!(
                        "soroquake: ✗ {} broke after {before} calls, shrunk to {} (seed {:#x})",
                        inv.name,
                        steps.len(),
                        seq_seed
                    );
                    broken[k] = Some(Failure {
                        invariant: inv.name,
                        pack: inv.pack,
                        reason,
                        seed: format!("{seq_seed:#x}"),
                        steps_before: before,
                        sequence: steps,
                    });
                }
            }
            if broken.iter().all(Option::is_some) {
                sequences += 1;
                break 'campaign;
            }
            if any_broke {
                break;
            }
        }
        sequences += 1;
        if last_tick.elapsed() >= Duration::from_secs(1) {
            last_tick = Instant::now();
            #[cfg(feature = "report")]
            reporter.progress(sequences, coverage(&seen));
        }
    }

    let report = Report {
        seed: seed_hex,
        sequences,
        coverage: coverage(&seen),
        results: invariants.iter().zip(&broken).map(|(i, b)| (i.name, i.pack, b.is_none())).collect(),
        failures: broken.into_iter().flatten().collect(),
        lab,
        elapsed: started.elapsed(),
    };

    #[cfg(feature = "report")]
    reporter.finish(&report);

    print_summary(&report);
    let lab_broke: Vec<_> = report.lab.iter().filter(|r| r.status == LabStatus::Broke).map(|r| r.case.id).collect();
    if !report.failures.is_empty() || !lab_broke.is_empty() {
        let names: Vec<_> = report.failures.iter().map(|f| f.invariant).collect();
        panic!(
            "soroquake: {} invariant(s) broke{}{}",
            names.len(),
            if names.is_empty() { String::new() } else { format!(": {}", names.join(", ")) },
            if lab_broke.is_empty() { String::new() } else { format!("; lab cases broke: {}", lab_broke.join(", ")) },
        );
    }
    report
}

fn print_summary(r: &Report) {
    println!(
        "\nsoroquake: {} sequences in {:.1}s · {:.0}% coverage · seed {}",
        r.sequences,
        r.elapsed.as_secs_f32(),
        r.coverage,
        r.seed
    );
    for (name, pack, held) in &r.results {
        println!("  {} {name} ({pack})", if *held { "✓" } else { "✗" });
    }
    for l in r.lab.iter().filter(|l| l.status == LabStatus::Broke) {
        println!("\n  lab {} — {}", l.case.id, l.reason);
        for (i, s) in l.sequence.iter().enumerate() {
            println!("    {}. {:<6} {:<24} {}", i + 1, s.who, s.call, s.note);
        }
    }
    for f in &r.failures {
        println!("\n  {} — {}", f.invariant, f.reason);
        for (i, s) in f.sequence.iter().enumerate() {
            println!("    {}. {:<6} {:<24} {}", i + 1, s.who, s.call, s.note);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check;

    /// A counter that must stay below 5. `Add(n)` adds n; sequences of
    /// random adds break it, and the shrunk sequence should be short.
    struct Counter;

    impl Harness for Counter {
        type State = i64;
        type Op = i64;
        fn setup(&self) -> i64 {
            0
        }
        fn generate(&self, _: &i64, rng: &mut SeededRng) -> i64 {
            rng.range(0, 2) as i64
        }
        fn apply(&self, s: &mut i64, op: &i64) -> Step {
            *s += op;
            Step::new("alice", format!("add({op})")).branch(if *op == 0 { "add:zero" } else { "add:some" })
        }
        fn invariants(&self) -> Vec<Invariant<i64>> {
            vec![Invariant::new("below_five", "demo", |s| check!(*s < 5, "counter reached {s}"))]
        }
        fn branches(&self) -> Vec<&'static str> {
            vec!["add:zero", "add:some"]
        }
    }

    fn cfg() -> Config {
        Config {
            time: Duration::from_millis(300),
            seed: 7,
            max_steps: 32,
            trigger: "local".into(),
            branch: String::new(),
            api: String::new(),
            token: String::new(),
        }
    }

    #[test]
    fn finds_and_shrinks() {
        let result = std::panic::catch_unwind(|| run_with(Counter, cfg()));
        assert!(result.is_err(), "campaign should fail");
    }

    #[test]
    fn shrink_keeps_only_needed_calls() {
        let ops = vec![0, 2, 0, 0, 1, 0, 2, 0, 2];
        let inv = &Counter.invariants()[0];
        let small = shrink_until(&Counter, ops, inv);
        assert_eq!(small.iter().sum::<i64>(), 5);
        assert!(small.len() <= 3, "got {small:?}");
    }
}
