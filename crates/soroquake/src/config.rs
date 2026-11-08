//! Campaign settings, read from the environment so the same harness runs
//! locally and in CI without changes.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    /// How long to explore. `SOROQUAKE_TIME`: `90`, `90s`, `5m`, `1h`. Default 60s.
    pub time: Duration,
    /// Campaign seed. `SOROQUAKE_SEED` (decimal or 0x-hex) replays a campaign.
    pub seed: u64,
    /// Calls per sequence. `SOROQUAKE_STEPS`, default 32.
    pub max_steps: usize,
    /// `pr`, `push`, `nightly` or `local`, from GitHub Actions when present.
    pub trigger: String,
    pub branch: String,
    /// Dashboard API base, e.g. `https://soroquake.dev/api`. `SOROQUAKE_API`.
    pub api: String,
    /// Upload token. Reporting is skipped when empty. `SOROQUAKE_TOKEN`.
    pub token: String,
}

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_default().trim().to_string()
}

pub fn parse_duration(s: &str) -> Option<Duration> {
    let s = s.trim();
    let (num, mult) = match s.chars().last()? {
        's' => (&s[..s.len() - 1], 1),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 3600),
        _ => (s, 1),
    };
    num.parse::<u64>().ok().map(|n| Duration::from_secs(n * mult))
}

pub fn parse_seed(s: &str) -> Option<u64> {
    let s = s.trim();
    match s.strip_prefix("0x") {
        Some(hex) => u64::from_str_radix(hex, 16).ok(),
        None => s.parse().ok(),
    }
}

fn git_branch() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

impl Config {
    pub fn from_env() -> Self {
        let trigger = match env("GITHUB_EVENT_NAME").as_str() {
            "pull_request" | "pull_request_target" => "pr",
            "schedule" => "nightly",
            "push" | "workflow_dispatch" => "push",
            _ => "local",
        }
        .to_string();
        let branch = [env("GITHUB_HEAD_REF"), env("GITHUB_REF_NAME")]
            .into_iter()
            .find(|b| !b.is_empty())
            .unwrap_or_else(git_branch);
        let seed = parse_seed(&env("SOROQUAKE_SEED")).unwrap_or_else(|| {
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            (t.as_nanos() as u64 ^ (std::process::id() as u64).rotate_left(16)) & 0xffff_ffff
        });
        let api = env("SOROQUAKE_API");
        Config {
            time: parse_duration(&env("SOROQUAKE_TIME")).unwrap_or(Duration::from_secs(60)),
            seed,
            max_steps: env("SOROQUAKE_STEPS").parse().unwrap_or(32),
            trigger,
            branch,
            api: if api.is_empty() {
                "https://soroquake.dev/api".into()
            } else {
                api.trim_end_matches('/').to_string()
            },
            token: env("SOROQUAKE_TOKEN"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("90"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration("5m"), Some(Duration::from_secs(300)));
        assert_eq!(parse_duration("1h"), Some(Duration::from_secs(3600)));
        assert_eq!(parse_duration("x"), None);
    }

    #[test]
    fn seeds() {
        assert_eq!(parse_seed("0x10"), Some(16));
        assert_eq!(parse_seed("42"), Some(42));
    }
}
