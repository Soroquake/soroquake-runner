//! Fork mode: fuzz against real ledger state.
//!
//! Create a snapshot of deployed contracts with the Stellar CLI:
//!
//! ```text
//! stellar snapshot create --address C... --network testnet --out snapshots/testnet.json
//! ```
//!
//! Next to it, write `snapshots/testnet.meta.json` naming what's inside:
//!
//! ```json
//! { "contracts": { "vault": "C..." }, "actors": { "alice": "G..." } }
//! ```
//!
//! Then set `SOROQUAKE_SNAPSHOT=snapshots/testnet.json`. In your harness,
//! build each sequence's `Env` from the snapshot (see the vault example) and
//! use [`Fork::contract`] and [`Fork::actor`] for the real addresses.

use std::path::{Path, PathBuf};

use serde_json::Value;

/// A loaded snapshot and its metadata.
#[derive(Debug, Clone)]
pub struct Fork {
    pub path: PathBuf,
    /// `testnet`, `mainnet` or the network id when unknown.
    pub network: String,
    pub ledger: u64,
    /// Unix seconds of the snapshotted ledger.
    pub timestamp: u64,
    pub entries: usize,
    pub bytes: u64,
    /// Content fingerprint, so campaigns on the same state group together.
    pub fingerprint: String,
    /// `(name, address)` from the meta file.
    pub contracts: Vec<(String, String)>,
    pub actors: Vec<(String, String)>,
}

const TESTNET_ID: &str = "cee0302d59844d32bdca915c8203dd44b33fbb7edc19051ea37abedf28ecd472";
const MAINNET_ID: &str = "7ac33997544e3175d266bd022439b22cdb16508c01163f26e5cb2a3e1045a979";

fn fnv(bytes: &[u8]) -> String {
    let h = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01b3));
    format!("{h:016x}")
}

fn pairs(v: Option<&Value>) -> Vec<(String, String)> {
    v.and_then(Value::as_object)
        .map(|m| m.iter().filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string()))).collect())
        .unwrap_or_default()
}

impl Fork {
    /// The snapshot named by `SOROQUAKE_SNAPSHOT`, if set.
    pub fn from_env() -> Option<Fork> {
        let path = std::env::var("SOROQUAKE_SNAPSHOT").ok().filter(|p| !p.trim().is_empty())?;
        let path = path.trim().to_string();
        // cargo runs tests from the package directory; accept paths relative to
        // the repository root too by searching parent directories.
        let resolved = std::env::current_dir()
            .ok()
            .and_then(|cwd| cwd.ancestors().map(|d| d.join(&path)).find(|p| p.is_file()))
            .unwrap_or_else(|| PathBuf::from(&path));
        match Fork::load(&resolved) {
            Ok(f) => Some(f),
            Err(e) => panic!("soroquake: couldn't load snapshot {path}: {e}"),
        }
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Fork, String> {
        let path = path.as_ref().to_path_buf();
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        let snap: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let network_id = snap["network_id"].as_str().unwrap_or_default();
        let network = match network_id {
            TESTNET_ID => "testnet".to_string(),
            MAINNET_ID => "mainnet".to_string(),
            other => other.chars().take(12).collect(),
        };
        let meta_path = path.with_extension("meta.json");
        let meta: Value = std::fs::read(&meta_path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        Ok(Fork {
            network,
            ledger: snap["sequence_number"].as_u64().unwrap_or(0),
            timestamp: snap["timestamp"].as_u64().unwrap_or(0),
            entries: snap["ledger_entries"].as_array().map(Vec::len).unwrap_or(0),
            bytes: bytes.len() as u64,
            fingerprint: fnv(&bytes),
            contracts: pairs(meta.get("contracts")),
            actors: pairs(meta.get("actors")),
            path,
        })
    }

    /// Address of a contract named in the meta file.
    pub fn contract(&self, name: &str) -> Option<&str> {
        self.contracts.iter().find(|(n, _)| n == name).map(|(_, a)| a.as_str())
    }

    /// Address of an actor named in the meta file.
    pub fn actor(&self, name: &str) -> Option<&str> {
        self.actors.iter().find(|(n, _)| n == name).map(|(_, a)| a.as_str())
    }

    pub(crate) fn to_json(&self) -> Value {
        serde_json::json!({
            "network": self.network,
            "ledger": self.ledger,
            "timestamp": self.timestamp,
            "entries": self.entries,
            "bytes": self.bytes,
            "fingerprint": self.fingerprint,
            "contracts": self.contracts.iter().map(|(n, a)| serde_json::json!({ "name": n, "address": a })).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_snapshot_and_meta() {
        let dir = std::env::temp_dir().join(format!("sq-fork-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let snap = dir.join("t.json");
        std::fs::write(&snap, format!(r#"{{"sequence_number":42,"timestamp":7,"network_id":"{TESTNET_ID}","ledger_entries":[1,2,3]}}"#)).unwrap();
        std::fs::write(dir.join("t.meta.json"), r#"{"contracts":{"vault":"CABC"},"actors":{"alice":"GXYZ"}}"#).unwrap();
        let f = Fork::load(&snap).unwrap();
        assert_eq!((f.network.as_str(), f.ledger, f.entries), ("testnet", 42, 3));
        assert_eq!(f.contract("vault"), Some("CABC"));
        assert_eq!(f.actor("alice"), Some("GXYZ"));
        assert_eq!(f.fingerprint.len(), 16);
        std::fs::remove_dir_all(dir).ok();
    }
}
