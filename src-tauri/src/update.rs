// update.rs — checking for and installing releases published on GitHub.
//
// Updates are signed; the plugin refuses anything that does not verify against
// the public key embedded in tauri.conf.json, so a spoofed or compromised
// endpoint cannot install arbitrary code.
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::UpdaterExt;

/// What the Settings panel needs to know about updates.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// "idle" | "available" | "downloading" | "error"
    pub state: String,
    pub current_version: String,
    /// Set only when an update is waiting.
    pub available_version: Option<String>,
    pub notes: Option<String>,
    pub error: Option<String>,
}

fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

fn idle(app: &AppHandle) -> UpdateStatus {
    UpdateStatus {
        state: "idle".into(),
        current_version: current_version(app),
        ..Default::default()
    }
}

/// Ask the endpoint whether a newer release exists.
///
/// A missing endpoint or no network is reported as "idle", not an error: this
/// app is expected to run offline and must not nag about it. A signature or
/// parse failure *is* surfaced, because that means something is wrong rather
/// than merely absent.
pub async fn check(app: &AppHandle) -> UpdateStatus {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            return UpdateStatus {
                state: "error".into(),
                current_version: current_version(app),
                error: Some(e.to_string()),
                ..Default::default()
            }
        }
    };

    match updater.check().await {
        Ok(Some(update)) => UpdateStatus {
            state: "available".into(),
            current_version: current_version(app),
            available_version: Some(update.version.clone()),
            notes: update.body.clone(),
            error: None,
        },
        Ok(None) => idle(app),
        Err(e) => {
            let msg = e.to_string();
            // No release published yet, or simply offline. Neither is a problem
            // the user needs to see.
            let benign = msg.contains("404")
                || msg.contains("Could not fetch")
                || msg.contains("error sending request")
                || msg.contains("dns")
                || msg.contains("connect");
            if benign {
                idle(app)
            } else {
                UpdateStatus {
                    state: "error".into(),
                    current_version: current_version(app),
                    error: Some(msg),
                    ..Default::default()
                }
            }
        }
    }
}

/// Download and install the pending update, then restart.
///
/// Only ever called from an explicit button press — an unannounced restart
/// would discard whatever the user was typing.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available".to_string())?;

    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;

    // Replaced on disk; restart to run the new version.
    tauri::process::restart(&app.env())
}
