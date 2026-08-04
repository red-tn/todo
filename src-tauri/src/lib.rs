// lib.rs — Tauri command surface and app wiring.
//
// The modules below own the real work: `config` the credential, `db` the Neon
// queries, `sync` the local cache and the merge between the two.
mod config;
mod db;
mod model;
mod notify;
mod recur;
mod sync;
mod tray;
mod update;

use chrono::Utc;
use model::Todo;
use std::sync::Arc;
use sync::{Status, SyncState};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

/// Emitted whenever the sync status changes, so the settings panel can update
/// without polling.
const STATUS_EVENT: &str = "sync-status";

type App = tauri::AppHandle;

fn state(app: &App) -> Arc<SyncState> {
    app.state::<Arc<SyncState>>().inner().clone()
}

/// Persist the cache and kick off a debounced sync.
fn touch(app: &App, st: &Arc<SyncState>) -> Result<(), String> {
    st.save()?;
    let handle = app.clone();
    sync::schedule(st.clone(), move |status: Status| {
        let _ = handle.emit(STATUS_EVENT, status);
    });
    Ok(())
}

/* ---------- commands ---------- */

/// The live todo list. Tombstones are filtered out, so the frontend sees
/// exactly the shape it always has.
#[tauri::command]
fn load_todos(app: App) -> Vec<Todo> {
    state(&app).live_todos()
}

/// Create or update one todo. Always succeeds locally; the network is not in
/// the path.
#[tauri::command]
fn upsert_todo(app: App, todo: Todo) -> Result<(), String> {
    let st = state(&app);
    {
        let mut todos = st.todos.lock().unwrap();
        let mut incoming = todo;
        incoming.updated_at = Some(Utc::now());
        incoming.dirty = true;
        incoming.deleted_at = None;

        // Spawning keys off the transition, not the final state, so a sync that
        // merely delivers an already-completed row cannot create a duplicate.
        let just_completed = match todos.iter_mut().find(|t| t.id == incoming.id) {
            Some(existing) => {
                let transitioned = !existing.done && incoming.done;
                // createdAt belongs to the original row, not to this edit.
                incoming.created_at = existing.created_at;
                *existing = incoming.clone();
                transitioned
            }
            None => {
                todos.push(incoming.clone());
                incoming.done
            }
        };

        if just_completed {
            let today = chrono::Local::now().date_naive();
            if let Some(next) = recur::next_instance(&incoming, today, new_id()) {
                todos.push(next);
            }
        }
    }
    touch(&app, &st)
}

/// Id for a spawned recurring task. Mirrors the frontend's scheme so ids look
/// the same wherever they were created.
fn new_id() -> String {
    format!(
        "{:x}-{:x}",
        Utc::now().timestamp_millis(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    )
}

/// Soft-delete a todo, leaving a tombstone so the delete reaches the other
/// machine instead of being resurrected by its copy.
#[tauri::command]
fn delete_todo(app: App, id: String) -> Result<(), String> {
    let st = state(&app);
    {
        let mut todos = st.todos.lock().unwrap();
        if let Some(t) = todos.iter_mut().find(|t| t.id == id) {
            let now = Utc::now();
            t.deleted_at = Some(now);
            t.updated_at = Some(now);
            t.dirty = true;
        }
    }
    touch(&app, &st)
}

/// Pull and push right now. Used on window focus and by the Sync button.
#[tauri::command]
async fn sync_now(app: App) -> Status {
    let st = state(&app);
    let status = sync::run_sync(&st).await;
    let _ = app.emit(STATUS_EVENT, status.clone());
    status
}

#[tauri::command]
fn get_sync_status(app: App) -> Status {
    state(&app).snapshot_status()
}

/// Save a connection string after proving it works.
///
/// Validating before writing means a typo surfaces immediately instead of
/// silently disabling sync. An empty string clears the credential and returns
/// the app to purely local operation.
#[tauri::command]
async fn set_db_url(app: App, url: String) -> Result<Status, String> {
    let st = state(&app);
    let url = url.trim().to_string();
    let dir = st.dir.clone();

    if url.is_empty() {
        *st.pool.write().await = None;
        st.set_host(None);
        config::update(&dir, |c| c.database_url = None)?;
        let status = st.snapshot_status();
        let _ = app.emit(STATUS_EVENT, status.clone());
        return Ok(status);
    }

    sync::connect(&st, &url).await?;
    config::update(&dir, |c| c.database_url = Some(url))?;

    let status = sync::run_sync(&st).await;
    let _ = app.emit(STATUS_EVENT, status.clone());
    Ok(status)
}

/* ---------- daily digest ---------- */

#[tauri::command]
fn get_digest_config(app: App) -> config::DigestConfig {
    config::load(&state(&app).dir).digest
}

#[tauri::command]
fn set_digest_config(app: App, enabled: bool, time: String) -> Result<config::DigestConfig, String> {
    let dir = state(&app).dir.clone();
    let cfg = config::update(&dir, |c| {
        c.digest = config::DigestConfig { enabled, time };
    })?;
    Ok(cfg.digest)
}

/// Send the digest right now regardless of schedule, so the Settings "Preview"
/// button can prove notifications actually reach the desktop.
#[tauri::command]
fn preview_digest(app: App) -> Result<String, String> {
    let st = state(&app);
    let digest = notify::build_digest(&st.live_todos(), chrono::Local::now().date_naive());
    if digest.is_empty() {
        return Ok("Nothing due or overdue right now.".into());
    }
    send_digest(&app, &digest);
    Ok(digest.title())
}

fn send_digest(app: &App, digest: &notify::Digest) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title(digest.title())
        .body(digest.body())
        .show();
}

/// Check once a minute whether the digest is due, and fire it if so.
fn spawn_digest_loop(app: App) {
    tauri::async_runtime::spawn(async move {
        let mut last_fired: Option<chrono::NaiveDate> = None;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(notify::TICK_SECONDS)).await;

            let st = state(&app);
            let cfg = config::load(&st.dir).digest;
            if !cfg.enabled {
                continue;
            }
            let (hour, minute) = cfg.hour_minute();
            let now = chrono::Local::now();
            if !notify::should_fire(now, hour, minute, last_fired) {
                continue;
            }

            // Mark the day as handled even when there is nothing to say, so an
            // empty list does not retry every minute until midnight.
            last_fired = Some(now.date_naive());

            let digest = notify::build_digest(&st.live_todos(), now.date_naive());
            if !digest.is_empty() {
                send_digest(&app, &digest);
            }
        }
    });
}

/* ---------- autostart ---------- */

#[tauri::command]
fn get_autostart(app: App) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// Turn start-on-login on or off, returning the state that actually took
/// effect so the UI can correct itself if the OS refused.
#[tauri::command]
fn set_autostart(app: App, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    if enabled {
        launcher.enable().map_err(|e| e.to_string())?;
    } else {
        launcher.disable().map_err(|e| e.to_string())?;
    }
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

/* ---------- updates ---------- */

/// Ask GitHub whether a newer release exists. Never throws for "offline".
#[tauri::command]
async fn check_update(app: App) -> update::UpdateStatus {
    update::check(&app).await
}

/// Install the pending update and restart. Called only from an explicit
/// button press, never automatically.
#[tauri::command]
async fn install_update(app: App) -> Result<(), String> {
    update::install(&app).await
}

/// Open a URL in the user's default browser.
#[tauri::command]
fn open_link(app: App, url: String) -> Result<(), String> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/* ---------- app setup ---------- */

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_process::init())
        // macOS ships a single universal binary, which tauri-action publishes
        // under the `darwin-universal` key. The updater otherwise looks for
        // `darwin-aarch64` or `darwin-x86_64` based on the running architecture
        // and would find nothing, so the target has to be set explicitly.
        .plugin({
            let builder = tauri_plugin_updater::Builder::new();
            #[cfg(target_os = "macos")]
            let builder = builder.target("darwin-universal");
            builder.build()
        })
        .plugin(tauri_plugin_notification::init())
        // Launching at login passes --hidden so the app starts quietly in the
        // tray instead of throwing a window up on every boot.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        // Closing the window hides it instead of quitting, so sync and the
        // daily digest keep running. Quit lives in the tray menu.
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            tray::MENU_SHOW => tray::show_window(app),
            tray::MENU_SYNC => {
                let st = state(app);
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    let status = sync::run_sync(&st).await;
                    let _ = handle.emit(STATUS_EVENT, status);
                });
            }
            tray::MENU_AUTOSTART => {
                // The checkbox has already flipped itself; mirror it to the OS.
                let launcher = app.autolaunch();
                let now_on = launcher.is_enabled().unwrap_or(false);
                let _ = if now_on {
                    launcher.disable()
                } else {
                    launcher.enable()
                };
            }
            tray::MENU_QUIT => app.exit(0),
            _ => {}
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;

            let st = Arc::new(SyncState::new(dir.clone()));
            app.manage(st.clone());

            let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
            tray::build(app.handle(), autostart_on)?;

            spawn_digest_loop(app.handle().clone());

            // Started by the login item: stay in the tray rather than stealing
            // focus during boot.
            if std::env::args().any(|a| a == "--hidden") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            // Connect and run the first sync in the background: a slow or
            // unreachable database must not delay the window appearing.
            if let Some(url) = config::load(&dir).database_url {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let status = match sync::connect(&st, &url).await {
                        Ok(()) => sync::run_sync(&st).await,
                        Err(e) => {
                            st.set_host(config::mask_host(&url));
                            st.mark_offline(e);
                            st.snapshot_status()
                        }
                    };
                    let _ = handle.emit(STATUS_EVENT, status);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            load_todos,
            upsert_todo,
            delete_todo,
            sync_now,
            get_sync_status,
            set_db_url,
            get_autostart,
            set_autostart,
            get_digest_config,
            set_digest_config,
            preview_digest,
            check_update,
            install_update,
            open_link
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
