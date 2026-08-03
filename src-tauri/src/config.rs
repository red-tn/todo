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
}

fn path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

/// Read the configured connection string.
///
/// `TODO_DATABASE_URL` wins when set and non-empty, so a machine can be pointed
/// at a different database without touching the saved file.
pub fn load(dir: &Path) -> Config {
    if let Ok(url) = std::env::var(ENV_VAR) {
        if !url.trim().is_empty() {
            return Config {
                database_url: Some(url.trim().to_string()),
            };
        }
    }
    fs::read_to_string(path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Config>(&s).ok())
        .unwrap_or_default()
}

/// Persist the connection string, readable only by the current user on Unix.
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
