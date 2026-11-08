//! Replay lab: scripted call patterns behind well-known exploit classes,
//! aimed at your contracts and judged by your own invariants.
//!
//! Each case is written in generic actions ([`LabAction`]). Your harness maps
//! them to concrete calls in [`crate::Harness::lab_op`]; cases that use an
//! action your contracts don't have are skipped. The engine runs every case
//! as written, then mutates it (amounts, repeated steps) because real exploits
//! rarely match a textbook script exactly. Mutations never add calls outside
//! the script, so a broken case always points at its own pattern.

use std::time::{Duration, Instant};

use crate::campaign::{replay_until, shrink_until, Harness, Invariant, Step};
use crate::check::Check;
use crate::rng::SeededRng;

/// Who performs a lab action. Map each to one of your harness actors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Actor {
    Attacker,
    Victim,
    Admin,
}

/// A generic action a lab case can take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabAction {
    /// Put assets into a vault and receive shares.
    Deposit(Actor, i128),
    /// Burn this many shares.
    Withdraw(Actor, i128),
    /// Burn every share the actor holds.
    WithdrawAll(Actor),
    /// Assets reach the vault without minting shares (a direct transfer,
    /// a donation or yield). Moves the share price.
    Donate(Actor, i128),
    /// Create tokens.
    Mint(Actor, i128),
    Transfer(Actor, Actor, i128),
    Approve(Actor, Actor, i128),
    /// `spender` moves `from`'s tokens to `to`.
    TransferFrom { spender: Actor, from: Actor, to: Actor, amount: i128 },
    Burn(Actor, i128),
}

impl LabAction {
    fn amount_mut(&mut self) -> Option<&mut i128> {
        match self {
            LabAction::Deposit(_, a)
            | LabAction::Withdraw(_, a)
            | LabAction::Donate(_, a)
            | LabAction::Mint(_, a)
            | LabAction::Transfer(_, _, a)
            | LabAction::Approve(_, _, a)
            | LabAction::Burn(_, a) => Some(a),
            LabAction::TransferFrom { amount, .. } => Some(amount),
            LabAction::WithdrawAll(_) => None,
        }
    }
}

/// One scripted exploit pattern.
#[derive(Debug, Clone)]
pub struct LabCase {
    pub id: &'static str,
    pub title: &'static str,
    /// The class of bug, e.g. `share inflation`.
    pub class: &'static str,
    /// What the attack does, in one or two sentences.
    pub summary: &'static str,
    pub script: Vec<LabAction>,
}

/// The built-in cases. Each follows a pattern documented across smart contract
/// audits; none is a copy of one specific incident.
pub fn cases() -> Vec<LabCase> {
    use Actor::*;
    use LabAction::*;
    vec![
        LabCase {
            id: "first-depositor-inflation",
            title: "First depositor inflates the share price",
            class: "share inflation",
            summary: "The first depositor mints a single share, then pushes assets in without minting. \
                      The next depositor's shares round down to zero and their deposit is absorbed.",
            script: vec![Deposit(Attacker, 1), Donate(Attacker, 1_000), Deposit(Victim, 500), WithdrawAll(Attacker)],
        },
        LabCase {
            id: "rounding-drain",
            title: "Withdrawals round in the caller's favour",
            class: "rounding direction",
            summary: "Once the share price is above one, small withdrawals that round up pay out a little \
                      more than they should. Repeated, they drain value from everyone else.",
            script: vec![
                Deposit(Victim, 3),
                Donate(Admin, 1),
                Deposit(Attacker, 3),
                Withdraw(Attacker, 1),
                Withdraw(Attacker, 1),
                Withdraw(Attacker, 1),
            ],
        },
        LabCase {
            id: "withdraw-others-shares",
            title: "Withdraw shares you don't own",
            class: "access and accounting",
            summary: "An attacker with no position asks to burn shares that belong to someone else. \
                      The vault must refuse, and its books must still add up.",
            script: vec![Deposit(Victim, 10), Withdraw(Attacker, 10), WithdrawAll(Victim)],
        },
        LabCase {
            id: "allowance-overspend",
            title: "Spend an allowance twice",
            class: "allowance accounting",
            summary: "A spender uses most of an allowance, then tries again for the same amount. \
                      The second spend must fail and the allowance must shrink by exactly what was spent.",
            script: vec![
                Mint(Victim, 100),
                Approve(Victim, Attacker, 10),
                TransferFrom { spender: Attacker, from: Victim, to: Attacker, amount: 6 },
                TransferFrom { spender: Attacker, from: Victim, to: Attacker, amount: 6 },
            ],
        },
        LabCase {
            id: "self-transfer-supply",
            title: "Create tokens with self-transfers",
            class: "balance accounting",
            summary: "Transfers and allowance spends where sender and receiver are the same account, \
                      followed by a burn larger than the balance. Supply must still equal the sum of balances.",
            script: vec![
                Mint(Attacker, 50),
                Transfer(Attacker, Attacker, 50),
                Approve(Attacker, Attacker, 50),
                TransferFrom { spender: Attacker, from: Attacker, to: Attacker, amount: 50 },
                Burn(Attacker, 60),
            ],
        },
    ]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabStatus {
    Held,
    Broke,
    Skipped,
}

impl LabStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            LabStatus::Held => "held",
            LabStatus::Broke => "broke",
            LabStatus::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct LabResult {
    pub case: LabCase,
    pub status: LabStatus,
    pub invariant: Option<&'static str>,
    pub reason: String,
    /// How many versions of the script ran (the original plus mutations).
    pub variants: u32,
    /// Shrunk failing sequence when the case broke.
    pub sequence: Vec<Step>,
}

fn mutate(script: &[LabAction], rng: &mut SeededRng) -> Vec<LabAction> {
    let mut out: Vec<LabAction> = Vec::with_capacity(script.len() + 2);
    for a in script {
        out.push(a.clone());
        // Repeat a step now and then: attacks are often the same call, looped.
        if rng.chance(15) {
            out.push(a.clone());
        }
    }
    out.into_iter()
        .map(|mut a| {
            if let Some(amount) = a.amount_mut() {
                if rng.chance(50) {
                    let factor = *rng.pick(&[2i128, 3, 10, 100]);
                    *amount = if rng.chance(50) { *amount * factor } else { (*amount / factor).max(1) };
                } else if rng.chance(30) {
                    *amount = (*amount + rng.range(-1, 1)).max(1);
                }
            }
            a
        })
        .collect()
}

fn hash(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3))
}

/// Run every lab case within `budget`.
pub(crate) fn run_lab<H: Harness>(h: &H, invariants: &[Invariant<H::State>], seed: u64, budget: Duration) -> Vec<LabResult> {
    let all = cases();
    let per_case = budget / all.len().max(1) as u32;
    let mut results = Vec::with_capacity(all.len());

    for case in all {
        let fresh = h.setup();
        if case.script.iter().any(|a| h.lab_op(&fresh, a).is_none()) {
            results.push(LabResult { case, status: LabStatus::Skipped, invariant: None, reason: String::new(), variants: 0, sequence: vec![] });
            continue;
        }
        drop(fresh);

        let deadline = Instant::now() + per_case;
        let mut variants = 0u32;
        let mut broke: Option<(usize, Vec<H::Op>, String)> = None;
        while broke.is_none() && (variants == 0 || (Instant::now() < deadline && variants < 2_000)) {
            let mut rng = SeededRng::new(seed ^ hash(case.id) ^ (variants as u64).wrapping_mul(0x9E37_79B9));
            let script = if variants == 0 { case.script.clone() } else { mutate(&case.script, &mut rng) };
            let mut state = h.setup();
            let mut ops: Vec<H::Op> = Vec::new();
            'steps: for action in &script {
                let Some(op) = h.lab_op(&state, action) else { continue };
                h.apply(&mut state, &op);
                ops.push(op);
                for (k, inv) in invariants.iter().enumerate() {
                    if let Check::Fail(reason) = (inv.check)(&state) {
                        broke = Some((k, ops.clone(), reason));
                        break 'steps;
                    }
                }
            }
            variants += 1;
        }

        match broke {
            Some((k, ops, _)) => {
                let inv = &invariants[k];
                let small = shrink_until(h, ops, inv);
                let (mut steps, fail) = replay_until(h, &small, inv);
                let reason = fail.map(|(_, r)| r).unwrap_or_default();
                if let Some(last) = steps.last_mut() {
                    last.note = reason.clone();
                }
                results.push(LabResult { case, status: LabStatus::Broke, invariant: Some(inv.name), reason, variants, sequence: steps });
            }
            None => results.push(LabResult { case, status: LabStatus::Held, invariant: None, reason: String::new(), variants, sequence: vec![] }),
        }
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutation_keeps_amounts_positive() {
        let mut rng = SeededRng::new(1);
        for case in cases() {
            for _ in 0..200 {
                for a in mutate(&case.script, &mut rng) {
                    let mut a = a;
                    if let Some(x) = a.amount_mut() {
                        assert!(*x >= 1, "{a:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn case_ids_are_unique() {
        let ids: std::collections::BTreeSet<_> = cases().iter().map(|c| c.id).collect();
        assert_eq!(ids.len(), cases().len());
    }
}
