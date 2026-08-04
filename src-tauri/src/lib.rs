// lib.rs — Tauri command surface and app wiring.
//
// The modules below own the real work: `config` the credential, `db` the Neon
// queries, `sync` the local cache and the merge between the two.
mod config;
mod db;
mod model;
mod sync;
mod update;

use chrono::Utc;
use model::Todo;
use std::sync::Arc;
use sync::{Status, SyncState};
use tauri::{Emitter, Manager};
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
        match todos.iter_mut().find(|t| t.id == incoming.id) {
            Some(existing) => {
                // createdAt belongs to the original row, not to this edit.
                incoming.created_at = existing.created_at;
                *existing = incoming;
            }
            None => todos.push(incoming),
        }
    }
    touch(&app, &st)
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
        config::save(&dir, &config::Config { database_url: None })?;
        let status = st.snapshot_status();
        let _ = app.emit(STATUS_EVENT, status.clone());
        return Ok(status);
    }

    sync::connect(&st, &url).await?;
    config::save(
        &dir,
        &config::Config {
            database_url: Some(url),
        },
    )?;

    let status = sync::run_sync(&st).await;
    let _ = app.emit(STATUS_EVENT, status.clone());
    Ok(status)
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;

            let st = Arc::new(SyncState::new(dir.clone()));
            app.manage(st.clone());

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
            check_update,
            install_update,
            open_link
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
