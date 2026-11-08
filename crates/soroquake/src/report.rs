//! Streams a campaign to the Soroquake API. Failures here never fail the
//! campaign: reporting is best effort, the local result is what counts.

use std::time::Duration;

use serde_json::json;

use crate::campaign::Report;
use crate::config::Config;

pub(crate) struct Reporter {
    agent: ureq::Agent,
    api: String,
    auth: String,
    campaign: Option<String>,
}

impl Reporter {
    pub(crate) fn start(cfg: &Config, seed: &str, invariants: usize, fork: Option<&crate::fork::Fork>) -> Self {
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(10)).build();
        let mut r = Reporter { agent, api: cfg.api.clone(), auth: format!("Bearer {}", cfg.token), campaign: None };
        if cfg.token.is_empty() {
            println!("soroquake: SOROQUAKE_TOKEN not set, results stay local");
            return r;
        }
        let body = json!({
            "trigger": cfg.trigger,
            "branch": cfg.branch,
            "seed": seed,
            "invariants": invariants,
            "budget_s": cfg.time.as_secs(),
            "snapshot": fork.map(|f| f.to_json()),
        });
        match r.post("/v1/ingest/campaigns", body) {
            Ok(v) => {
                let id = v["id"].as_str().map(str::to_string);
                if let (Some(id), Some(n)) = (&id, v["number"].as_i64()) {
                    println!("soroquake: reporting campaign #{n} to {} ({id})", cfg.api);
                }
                r.campaign = id;
            }
            Err(e) => println!("soroquake: couldn't reach {}: {e}. Results stay local.", cfg.api),
        }
        r
    }

    fn post(&self, path: &str, body: serde_json::Value) -> Result<serde_json::Value, String> {
        self.agent
            .post(&format!("{}{path}", self.api))
            .set("Authorization", &self.auth)
            .send_json(body)
            .map_err(|e| match e {
                ureq::Error::Status(code, resp) => format!("{code} {}", resp.into_string().unwrap_or_default()),
                other => other.to_string(),
            })?
            .into_json::<serde_json::Value>()
            .map_err(|e| e.to_string())
    }

    pub(crate) fn progress(&self, sequences: u64, coverage: f32) {
        if let Some(id) = &self.campaign {
            let _ = self.post(
                &format!("/v1/ingest/campaigns/{id}/progress"),
                json!({ "sequences": sequences, "coverage": coverage }),
            );
        }
    }

    pub(crate) fn finish(&self, r: &Report) {
        let Some(id) = &self.campaign else { return };
        let body = json!({
            "sequences": r.sequences,
            "coverage": r.coverage,
            "results": r.results.iter().map(|(name, pack, held)| json!({ "name": name, "pack": pack, "held": held })).collect::<Vec<_>>(),
            "failures": r.failures.iter().map(|f| json!({
                "invariant": f.invariant,
                "seed": f.seed,
                "steps_before": f.steps_before,
                "steps_after": f.sequence.len(),
                "sequence": f.sequence.iter().map(|s| json!({ "who": s.who, "call": s.call, "note": s.note })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "lab": r.lab.iter().map(|l| json!({
                "case": l.case.id,
                "title": l.case.title,
                "class": l.case.class,
                "summary": l.case.summary,
                "status": l.status.as_str(),
                "invariant": l.invariant.unwrap_or(""),
                "reason": l.reason,
                "variants": l.variants,
                "sequence": l.sequence.iter().map(|s| json!({ "who": s.who, "call": s.call, "note": s.note })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        });
        match self.post(&format!("/v1/ingest/campaigns/{id}/finish"), body) {
            Ok(_) => println!("soroquake: campaign saved to the dashboard"),
            Err(e) => println!("soroquake: couldn't save the campaign: {e}"),
        }
    }
}
