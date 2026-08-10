// config.rs — the Neon connection string: where it lives and how it is read.
//
// The credential never reaches the webview. It is written to config.json in the
// app data directory (0600 on Unix) and only ever leaves this process as a
// masked host string via `mask_host`.
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const ENV_VAR: &str = "TODO_DATABASE_URL";

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(default)]
    pub database_url: Option<String>,
    /// Daily digest settings. Deliberately per-machine rather than synced: a
    /// desktop that nags and a laptop that stays quiet is a reasonable setup.
    #[serde(default)]
    pub digest: DigestConfig,
    /// Slack alert settings. Per-machine for the same reason as the digest —
    /// and running the app on two machines with this enabled posts twice.
    #[serde(default)]
    pub slack: SlackConfig,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DigestConfig {
    pub enabled: bool,
    /// Local time of day to notify, "HH:MM".
    pub time: String,
}

impl Default for DigestConfig {
    fn default() -> Self {
        DigestConfig {
            enabled: true,
            time: "08:00".into(),
        }
    }
}

impl DigestConfig {
    /// Parse `time` into (hour, minute), falling back to 08:00 if it is
    /// malformed — a bad string should not silently disable the digest.
    pub fn hour_minute(&self) -> (u32, u32) {
        parse_time(&self.time)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SlackConfig {
    pub enabled: bool,
    /// Incoming-webhook or Workflow Builder trigger URL. A secret like
    /// `database_url`: stored here, never returned to the webview.
    #[serde(default)]
    pub webhook_url: Option<String>,
    /// Independent due-date buckets, any combination of "today" | "tomorrow" |
    /// "week" (= 2–7 days out). Empty means overdue tasks only.
    #[serde(default = "default_thresholds")]
    pub thresholds: Vec<String>,
    /// One or two local times of day to post, "HH:MM".
    #[serde(default = "default_times")]
    pub times: Vec<String>,
}

fn default_thresholds() -> Vec<String> {
    vec!["today".into()]
}

fn default_times() -> Vec<String> {
    vec!["09:00".into()]
}

impl Default for SlackConfig {
    fn default() -> Self {
        SlackConfig {
            enabled: false,
            webhook_url: None,
            thresholds: default_thresholds(),
            times: default_times(),
        }
    }
}

impl SlackConfig {
    /// The configured send times as (hour, minute), capped at two.
    pub fn hours_minutes(&self) -> Vec<(u32, u32)> {
        self.times.iter().take(2).map(|t| parse_time(t)).collect()
    }
}

/// Parse "HH:MM", falling back to 08:00 — a bad string should not silently
/// disable a schedule.
fn parse_time(time: &str) -> (u32, u32) {
    let mut parts = time.split(':');
    let h = parts.next().and_then(|s| s.parse::<u32>().ok());
    let m = parts.next().and_then(|s| s.parse::<u32>().ok());
    match (h, m) {
        (Some(h), Some(m)) if h < 24 && m < 60 => (h, m),
        _ => (8, 0),
    }
}

fn path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Read the configured connection string.
///
/// `TODO_DATABASE_URL` wins when set and non-empty, so a machine can be pointed
/// at a different database without touching the saved file.
pub fn load(dir: &Path) -> Config {
    let mut cfg = load_file(dir);
    // Override only the URL: the environment says which database to use, not
    // what the rest of the settings should be.
    if let Ok(url) = std::env::var(ENV_VAR) {
        if !url.trim().is_empty() {
            cfg.database_url = Some(url.trim().to_string());
        }
    }
    cfg
}

/// Read, modify, and write the config in one step.
///
/// Callers must never construct a whole `Config` to change one field: doing so
/// silently resets everything else in the file.
pub fn update<F: FnOnce(&mut Config)>(dir: &Path, f: F) -> Result<Config, String> {
    let mut cfg = load_file(dir);
    f(&mut cfg);
    save(dir, &cfg)?;
    Ok(cfg)
}

/// The config exactly as stored, ignoring the environment override.
///
/// Saving must not bake a `TODO_DATABASE_URL` value into the file, so writes go
/// through this rather than `load`.
fn load_file(dir: &Path) -> Config {
    fs::read_to_string(path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default()
}

/// Persist the config, readable only by the current user on Unix.
pub fn save(dir: &Path, cfg: &Config) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let p = path(dir);
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    fs::write(&p, json).map_err(|e| e.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Extract just the host from a connection string, for display.
///
/// Returns `None` rather than risking leaking any part of the credential if the
/// string is not shaped like a URL.
pub fn mask_host(url: &str) -> Option<String> {
    // postgresql://user:pass@HOST/db?params
    let after_scheme = url.split("://").nth(1)?;
    let authority = after_scheme.split('/').next()?;
    let host = match authority.rsplit_once('@') {
        Some((_creds, h)) => h,
        None => authority,
    };
    let host = host.split(':').next()?.trim();
    if host.is_empty() {
        return None;
    }
    // Neon hosts are long; show enough to identify the project, not the whole thing.
    match host.split_once('.') {
        Some((first, _rest)) => Some(format!("{first}…")),
        None => Some(host.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_a_neon_url_to_its_endpoint_id() {
        // A representative Neon URL shape, not a real endpoint — no part of a
        // live connection string belongs in the repository.
        let url = "postgresql://u:p@ep-example-endpoint-1234-pooler.c-2.us-east-1.aws.neon.tech/neondb?sslmode=require";
        assert_eq!(
            mask_host(url).as_deref(),
            Some("ep-example-endpoint-1234-pooler…")
        );
    }

    #[test]
    fn mask_never_returns_the_password() {
        let url = "postgresql://user:sup3rsecret@host.example.com/db";
        let masked = mask_host(url).unwrap();
        assert!(!masked.contains("sup3rsecret"));
        assert!(!masked.contains("user"));
    }

    #[test]
    fn mask_handles_garbage_without_panicking() {
        assert_eq!(mask_host("not a url"), None);
        assert_eq!(mask_host(""), None);
        assert_eq!(mask_host("postgresql://"), None);
    }
}
