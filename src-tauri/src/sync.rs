// sync.rs — the local cache and the merge between it and Neon.
//
// The local cache is what the UI reads. It is always writable, so a mutation
// never fails because the network is down; changed rows are flagged `dirty` and
// pushed on the next successful sync.
use crate::config;
use crate::db;
use crate::model::Todo;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPool;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

const TOMBSTONE_TTL_DAYS: i64 = 30;
/// Completed tasks drop out of the working list after this long.
pub const ARCHIVE_AFTER_DAYS: i64 = 30;
/// Mutations arrive in bursts (the edit form autosaves on a 300ms debounce), so
/// a sync is scheduled rather than fired per keystroke.
const SYNC_DEBOUNCE_MS: u64 = 1500;

/* ---------- persisted state ---------- */

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    /// Server timestamp of the last completed pull; the next pull's watermark.
    #[serde(default)]
    pub last_sync: Option<DateTime<Utc>>,
    /// Whether this machine has adopted the remote list yet.
    #[serde(default)]
    pub first_sync_done: bool,
}

/* ---------- status reported to the UI ---------- */

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// "unconfigured" | "ok" | "offline" | "error"
    pub state: String,
    pub host: Option<String>,
    pub pending: usize,
    pub last_sync: Option<DateTime<Utc>>,
    pub error: Option<String>,
    /// True when the last sync pulled in rows the UI has not rendered yet.
    /// Lets the frontend reload only when something actually arrived.
    pub changed: bool,
}

impl Default for Status {
    fn default() -> Self {
        Status {
            state: "unconfigured".into(),
            host: None,
            pending: 0,
            last_sync: None,
            error: None,
            changed: false,
        }
    }
}

/* ---------- app state ---------- */

pub struct SyncState {
    pub dir: PathBuf,
    pub todos: Mutex<Vec<Todo>>,
    pub meta: Mutex<Meta>,
    pub status: Mutex<Status>,
    pub pool: tokio::sync::RwLock<Option<PgPool>>,
    /// Serializes sync runs so a pull and a push can never interleave.
    pub gate: tokio::sync::Mutex<()>,
    scheduled: AtomicBool,
    /// When tombstones were last swept. Polling runs every few seconds; the
    /// sweep does not need to.
    last_purge: Mutex<Option<DateTime<Utc>>>,
}

impl SyncState {
    pub fn new(dir: PathBuf) -> Self {
        let todos = read_todos(&dir);
        let meta = read_meta(&dir);
        SyncState {
            dir,
            todos: Mutex::new(todos),
            meta: Mutex::new(meta),
            status: Mutex::new(Status::default()),
            pool: tokio::sync::RwLock::new(None),
            gate: tokio::sync::Mutex::new(()),
            scheduled: AtomicBool::new(false),
            last_purge: Mutex::new(None),
        }
    }

    /// Whether a tombstone sweep is due. At a 60s poll this is true roughly
    /// once an hour instead of 60 times.
    fn purge_due(&self) -> bool {
        let mut last = self.last_purge.lock().unwrap();
        let now = Utc::now();
        match *last {
            Some(t) if now - t < chrono::Duration::hours(1) => false,
            _ => {
                *last = Some(now);
                true
            }
        }
    }

    /// Rows the working list should see: neither deleted nor archived.
    pub fn live_todos(&self) -> Vec<Todo> {
        self.todos
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.is_live())
            .cloned()
            .collect()
    }

    /// Rows for the archive panel: archived, but not deleted.
    pub fn archived_todos(&self) -> Vec<Todo> {
        self.todos
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.is_archived() && !t.is_deleted())
            .cloned()
            .collect()
    }

    pub fn pending_count(&self) -> usize {
        self.todos.lock().unwrap().iter().filter(|t| t.dirty).count()
    }

    /// Snapshot the status, refreshing the fields that derive from the cache.
    pub fn snapshot_status(&self) -> Status {
        let mut s = self.status.lock().unwrap().clone();
        s.pending = self.pending_count();
        s.last_sync = self.meta.lock().unwrap().last_sync;
        s
    }

    fn set_state(&self, state: &str, error: Option<String>) {
        let mut s = self.status.lock().unwrap();
        s.state = state.into();
        s.error = error;
    }

    pub fn set_host(&self, host: Option<String>) {
        self.status.lock().unwrap().host = host;
    }

    /// Record that the database is unreachable. Edits keep working locally.
    pub fn mark_offline(&self, error: String) {
        self.set_state("offline", Some(error));
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_todos()?;
        self.save_meta()
    }

    /// Rewrite the cache file. The expensive one — only call it when the list
    /// actually changed.
    pub fn save_todos(&self) -> Result<(), String> {
        let todos = self.todos.lock().unwrap().clone();
        write_todos(&self.dir, &todos)
    }

    /// Rewrite the watermark. Small enough to write on every sync.
    pub fn save_meta(&self) -> Result<(), String> {
        let meta = self.meta.lock().unwrap().clone();
        write_meta(&self.dir, &meta)
    }
}

/* ---------- local file I/O ---------- */

fn todos_path(dir: &Path) -> PathBuf {
    dir.join("todos.json")
}
fn meta_path(dir: &Path) -> PathBuf {
    dir.join("sync.json")
}

/// Read the cache. A missing or corrupt file is treated as an empty list, which
/// matches the app's previous behaviour and self-heals on the next pull.
pub fn read_todos(dir: &Path) -> Vec<Todo> {
    std::fs::read_to_string(todos_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<Todo>>(&s).ok())
        .unwrap_or_default()
}

fn write_todos(dir: &Path, todos: &[Todo]) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(todos).map_err(|e| e.to_string())?;
    std::fs::write(todos_path(dir), json).map_err(|e| e.to_string())
}

fn read_meta(dir: &Path) -> Meta {
    std::fs::read_to_string(meta_path(dir))
        .ok()
        .and_then(|s| serde_json::from_str::<Meta>(&s).ok())
        .unwrap_or_default()
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(meta).map_err(|e| e.to_string())?;
    std::fs::write(meta_path(dir), json).map_err(|e| e.to_string())
}

/// Copy the current cache aside before this machine adopts the remote list.
fn backup_local(dir: &Path) -> Result<(), String> {
    let src = todos_path(dir);
    if !src.exists() {
        return Ok(());
    }
    let stamp = Utc::now().format("%Y%m%d-%H%M%S");
    let dest = dir.join(format!("todos.local-backup-{stamp}.json"));
    std::fs::copy(&src, &dest).map(|_| ()).map_err(|e| e.to_string())
}

/* ---------- the merge ---------- */

#[derive(Default, Debug, PartialEq)]
pub struct MergeStats {
    pub added: usize,
    pub updated: usize,
    pub kept_local: usize,
}

/// Fold remote rows into the local cache.
///
/// A dirty local row always wins: it holds an edit this machine has not pushed
/// yet, and the push that follows the merge makes it the remote value too.
/// Otherwise the newer timestamp wins.
pub fn merge(local: &mut Vec<Todo>, remote: Vec<Todo>) -> MergeStats {
    let mut stats = MergeStats::default();
    for r in remote {
        match local.iter_mut().find(|l| l.id == r.id) {
            None => {
                local.push(r);
                stats.added += 1;
            }
            Some(l) => {
                if l.dirty || r.stamp() <= l.stamp() {
                    stats.kept_local += 1;
                } else {
                    *l = r;
                    stats.updated += 1;
                }
            }
        }
    }
    stats
}

/// Move completed tasks that have sat untouched out of the working list.
///
/// Completing a task sets `updated_at` and nothing touches it afterwards, so
/// "done and not modified for N days" is a good enough stand-in for "completed
/// N days ago" without carrying a separate completion timestamp.
///
/// Pass `days = 0` for the manual "archive all done" action, which should not
/// wait for anything to age.
pub fn archive_completed(local: &mut [Todo], days: i64, now: DateTime<Utc>) -> usize {
    let cutoff = now - chrono::Duration::days(days.max(0));
    let mut archived = 0;
    for t in local.iter_mut() {
        let eligible = t.done && !t.is_archived() && !t.is_deleted() && t.stamp() <= cutoff;
        if eligible {
            t.archived_at = Some(now);
            t.updated_at = Some(now);
            t.dirty = true;
            archived += 1;
        }
    }
    archived
}

/// Drop tombstones old enough that every machine has seen them.
pub fn purge_local_tombstones(local: &mut Vec<Todo>) -> usize {
    let cutoff = Utc::now() - chrono::Duration::days(TOMBSTONE_TTL_DAYS);
    let before = local.len();
    local.retain(|t| match t.deleted_at {
        // A dirty tombstone still needs to be pushed, so it stays.
        Some(ts) => ts >= cutoff || t.dirty,
        None => true,
    });
    before - local.len()
}

/* ---------- connection ---------- */

/// Open a pool for the saved credential and verify the schema exists.
pub async fn connect(state: &SyncState, url: &str) -> Result<(), String> {
    let pool = db::connect(url).await?;
    db::ensure_schema(&pool).await?;
    *state.pool.write().await = Some(pool);
    state.set_host(config::mask_host(url));
    Ok(())
}

/* ---------- the sync run ---------- */

/// Pull, merge, push. Returns the resulting status.
///
/// Ordering matters: pulling first lets the merge decide conflicts while the
/// local edit is still flagged dirty, and pushing second makes this machine's
/// intent the shared truth.
pub async fn run_sync(state: &SyncState) -> Status {
    let _guard = state.gate.lock().await;

    let pool = { state.pool.read().await.clone() };
    let Some(pool) = pool else {
        state.set_state("unconfigured", None);
        return state.snapshot_status();
    };

    match sync_inner(state, &pool).await {
        Ok(outcome) => {
            // Settings ride along with every successful sync. A failure here
            // is logged, not surfaced: the todo sync already proved the
            // connection works, and settings retry on the next run anyway.
            if let Err(e) = sync_settings(state, &pool).await {
                eprintln!("settings sync failed: {e}");
            }
            state.set_state("ok", None);
            state.status.lock().unwrap().changed = outcome.changed;
            // Most polls are no-ops; skip rewriting the cache file when nothing
            // about the list moved.
            if outcome.touched_todos {
                let _ = state.save_todos();
            }
            let _ = state.save_meta();
            return state.snapshot_status();
        }
        Err(e) => {
            // A failed sync is normal (closed laptop, no wifi) — the cache is
            // untouched and the dirty rows simply wait.
            let offline = e.contains("timed out")
                || e.contains("dns")
                || e.contains("os error")
                || e.contains("connection")
                || e.contains("Connection");
            state.set_state(if offline { "offline" } else { "error" }, Some(e));
        }
    }
    let _ = state.save();
    state.snapshot_status()
}

struct Outcome {
    /// Remote rows landed in the cache; the UI needs to re-render.
    changed: bool,
    /// The list was modified at all — including flags the UI cannot see — so
    /// the cache file needs rewriting.
    touched_todos: bool,
}

async fn sync_inner(state: &SyncState, pool: &PgPool) -> Result<Outcome, String> {
    let first_done = { state.meta.lock().unwrap().first_sync_done };

    // First connect on this machine: adopt the remote list wholesale, keeping a
    // backup of whatever was here before.
    if !first_done {
        let (remote, server_now) = db::pull(pool, None).await?;
        backup_local(&state.dir)?;
        {
            let mut t = state.todos.lock().unwrap();
            *t = remote;
        }
        {
            let mut m = state.meta.lock().unwrap();
            m.first_sync_done = true;
            m.last_sync = Some(server_now);
        }
        return Ok(Outcome {
            changed: true,
            touched_todos: true,
        });
    }

    // 1. Pull everything changed since the last watermark and merge it in.
    let since = { state.meta.lock().unwrap().last_sync };
    let (remote, server_now) = db::pull(pool, since).await?;
    let stats = {
        let mut t = state.todos.lock().unwrap();
        merge(&mut t, remote)
    };
    let changed = stats.added > 0 || stats.updated > 0;

    // 2. Push rows this machine has changed. Snapshot them with their stamps so
    //    an edit made during the push is not mistakenly marked clean.
    let dirty: Vec<Todo> = {
        state
            .todos
            .lock()
            .unwrap()
            .iter()
            .filter(|t| t.dirty)
            .cloned()
            .collect()
    };
    let pushed_any = !dirty.is_empty();
    if pushed_any {
        db::push(pool, &dirty).await?;
        let pushed: Vec<(String, Option<DateTime<Utc>>)> =
            dirty.iter().map(|t| (t.id.clone(), t.updated_at)).collect();
        let mut t = state.todos.lock().unwrap();
        for (id, stamp) in pushed {
            if let Some(row) = t.iter_mut().find(|x| x.id == id) {
                if row.updated_at == stamp {
                    row.dirty = false;
                }
            }
        }
    }

    // 3. Advance the watermark to the pre-push server time. Our own rows come
    //    back on the next pull, which is how the cache picks up the server's
    //    authoritative timestamps.
    {
        state.meta.lock().unwrap().last_sync = Some(server_now);
    }

    // 4. Housekeeping, at most hourly — both of these write, and neither has
    //    any business running on every poll.
    let mut swept = 0;
    if state.purge_due() {
        swept = {
            let mut t = state.todos.lock().unwrap();
            let archived = archive_completed(&mut t, ARCHIVE_AFTER_DAYS, Utc::now());
            archived + purge_local_tombstones(&mut t)
        };
        let _ = db::purge_tombstones(pool).await;
        let _ = db::purge_slack_fired(pool).await;
    }

    Ok(Outcome {
        changed,
        touched_todos: changed || pushed_any || swept > 0,
    })
}

/* ---------- shared settings ---------- */

/// Fold the local slack config together with a remote one that is about to be
/// adopted. Whole-value last-write-wins, with one exception: a remote row with
/// no webhook URL never erases a URL this machine already has — a machine that
/// saved settings before ever pasting the URL must not blank it everywhere.
///
/// Returns the value to adopt plus whether it still differs from the remote
/// (in which case it must be marked dirty so the preserved URL propagates).
pub fn merge_slack(
    local: &config::SlackConfig,
    mut remote: config::SlackConfig,
) -> (config::SlackConfig, bool) {
    let mut still_dirty = false;
    if remote.webhook_url.is_none() && local.webhook_url.is_some() {
        remote.webhook_url = local.webhook_url.clone();
        still_dirty = true;
    }
    (remote, still_dirty)
}

/// Pull-and-push for the two synced settings, mirroring the todo merge:
/// a dirty local value wins and gets pushed; otherwise the newer server
/// stamp wins. A key missing remotely is pushed regardless of dirtiness so
/// existing machines bootstrap the table on their first sync after upgrading.
async fn sync_settings(state: &SyncState, pool: &PgPool) -> Result<(), String> {
    let remote = db::pull_settings(pool).await?;
    let local = config::load(&state.dir);

    for key in ["digest", "slack"] {
        let (local_json, meta) = match key {
            "digest" => (
                serde_json::to_string(&local.digest).map_err(|e| e.to_string())?,
                local.settings_meta.digest.clone(),
            ),
            _ => (
                serde_json::to_string(&local.slack).map_err(|e| e.to_string())?,
                local.settings_meta.slack.clone(),
            ),
        };
        let remote_row = remote.iter().find(|(k, _, _)| k == key);

        if meta.dirty || remote_row.is_none() {
            let stamp = db::push_setting(pool, key, &local_json).await?;
            config::update(&state.dir, |c| {
                let m = match key {
                    "digest" => &mut c.settings_meta.digest,
                    _ => &mut c.settings_meta.slack,
                };
                m.updated_at = Some(stamp);
                m.dirty = false;
            })?;
            continue;
        }

        let Some((_, value, stamp)) = remote_row else { continue };
        let newer = meta.updated_at.map_or(true, |local_ts| *stamp > local_ts);
        if !newer || *value == local_json {
            // Even when the value is identical, adopt the stamp so this
            // comparison stops running every sync.
            if newer {
                config::update(&state.dir, |c| match key {
                    "digest" => c.settings_meta.digest.updated_at = Some(*stamp),
                    _ => c.settings_meta.slack.updated_at = Some(*stamp),
                })?;
            }
            continue;
        }

        match key {
            "digest" => {
                let incoming: config::DigestConfig =
                    serde_json::from_str(value).map_err(|e| e.to_string())?;
                config::update(&state.dir, |c| {
                    c.digest = incoming;
                    c.settings_meta.digest.updated_at = Some(*stamp);
                    c.settings_meta.digest.dirty = false;
                })?;
            }
            _ => {
                let incoming: config::SlackConfig =
                    serde_json::from_str(value).map_err(|e| e.to_string())?;
                let (merged, still_dirty) = merge_slack(&local.slack, incoming);
                config::update(&state.dir, |c| {
                    c.slack = merged;
                    c.settings_meta.slack.updated_at = Some(*stamp);
                    c.settings_meta.slack.dirty = still_dirty;
                })?;
            }
        }
    }
    Ok(())
}

/// Ask for a sync soon, coalescing a burst of edits into one run.
pub fn schedule(state: std::sync::Arc<SyncState>, on_done: impl Fn(Status) + Send + 'static) {
    if state.scheduled.swap(true, Ordering::SeqCst) {
        return; // one is already pending; it will pick up this change too
    }
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(SYNC_DEBOUNCE_MS)).await;
        state.scheduled.store(false, Ordering::SeqCst);
        let status = run_sync(&state).await;
        on_done(status);
    });
}

/* ---------- tests ---------- */

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn todo(id: &str, title: &str, updated: i64) -> Todo {
        Todo {
            id: id.into(),
            title: title.into(),
            note: String::new(),            due: None,
            priority: None,
            done: false,
            tags: vec![],            created_at: at(0),
            updated_at: Some(at(updated)),
            deleted_at: None,
            archived_at: None,
            dirty: false,
            recurrence: None,
            recurrence_interval: 1,
        }
    }

    #[test]
    fn unknown_remote_row_is_added() {
        let mut local = vec![];
        let stats = merge(&mut local, vec![todo("a", "from other machine", 10)]);
        assert_eq!(stats.added, 1);
        assert_eq!(local.len(), 1);
        assert_eq!(local[0].title, "from other machine");
    }

    #[test]
    fn newer_remote_row_overwrites_clean_local() {
        let mut local = vec![todo("a", "old", 10)];
        let stats = merge(&mut local, vec![todo("a", "new", 20)]);
        assert_eq!(stats.updated, 1);
        assert_eq!(local[0].title, "new");
    }

    #[test]
    fn older_remote_row_does_not_overwrite() {
        let mut local = vec![todo("a", "current", 20)];
        let stats = merge(&mut local, vec![todo("a", "stale", 10)]);
        assert_eq!(stats.kept_local, 1);
        assert_eq!(local[0].title, "current");
    }

    #[test]
    fn dirty_local_row_wins_even_against_a_newer_remote() {
        // The unpushed edit is what the user just typed on this machine; it
        // must survive the pull and become the remote value on the push.
        let mut local = vec![todo("a", "my unpushed edit", 10)];
        local[0].dirty = true;
        let stats = merge(&mut local, vec![todo("a", "remote newer", 99)]);
        assert_eq!(stats.kept_local, 1);
        assert_eq!(local[0].title, "my unpushed edit");
        assert!(local[0].dirty, "row must stay dirty so it still gets pushed");
    }

    #[test]
    fn remote_tombstone_deletes_a_clean_local_row() {
        let mut local = vec![todo("a", "delete me", 10)];
        let mut tomb = todo("a", "delete me", 20);
        tomb.deleted_at = Some(at(20));
        merge(&mut local, vec![tomb]);
        assert!(local[0].is_deleted());
    }

    #[test]
    fn local_edit_beats_a_remote_tombstone_when_dirty() {
        let mut local = vec![todo("a", "still working on this", 10)];
        local[0].dirty = true;
        let mut tomb = todo("a", "deleted elsewhere", 99);
        tomb.deleted_at = Some(at(99));
        merge(&mut local, vec![tomb]);
        assert!(!local[0].is_deleted());
        assert_eq!(local[0].title, "still working on this");
    }

    #[test]
    fn rows_without_updated_at_fall_back_to_created_at() {
        // Rows written before sync existed have no updatedAt.
        let mut legacy = todo("a", "legacy", 0);
        legacy.updated_at = None;
        legacy.created_at = at(5);
        let mut local = vec![legacy];
        merge(&mut local, vec![todo("a", "newer", 10)]);
        assert_eq!(local[0].title, "newer");
    }

    #[test]
    fn merge_touches_only_matching_ids() {
        let mut local = vec![todo("a", "keep", 10), todo("b", "keep too", 10)];
        merge(&mut local, vec![todo("a", "changed", 20)]);
        assert_eq!(local[0].title, "changed");
        assert_eq!(local[1].title, "keep too");
    }

    /* ---------- settings merge ---------- */

    #[test]
    fn adopting_remote_slack_settings_never_erases_a_local_webhook_url() {
        let local = config::SlackConfig {
            enabled: true,
            webhook_url: Some("https://hooks.slack.com/triggers/x".into()),
            thresholds: vec!["today".into()],
            times: vec!["09:00".into()],
        };
        // The other machine saved settings before ever pasting a URL.
        let remote = config::SlackConfig {
            enabled: false,
            webhook_url: None,
            thresholds: vec!["week".into()],
            times: vec!["08:00".into(), "17:00".into()],
        };
        let (merged, still_dirty) = merge_slack(&local, remote);
        assert_eq!(merged.webhook_url, local.webhook_url, "URL must survive");
        assert!(!merged.enabled, "everything else is last-write-wins");
        assert_eq!(merged.times, vec!["08:00", "17:00"]);
        assert!(still_dirty, "the preserved URL must be pushed back out");
    }

    #[test]
    fn a_remote_webhook_url_replaces_the_local_one_cleanly() {
        let local = config::SlackConfig {
            enabled: true,
            webhook_url: Some("https://hooks.slack.com/triggers/old".into()),
            thresholds: vec!["today".into()],
            times: vec!["09:00".into()],
        };
        let mut remote = local.clone();
        remote.webhook_url = Some("https://hooks.slack.com/triggers/new".into());
        let (merged, still_dirty) = merge_slack(&local, remote);
        assert_eq!(
            merged.webhook_url.as_deref(),
            Some("https://hooks.slack.com/triggers/new")
        );
        assert!(!still_dirty, "a clean adoption needs no push back");
    }

    /* ---------- archiving ---------- */

    fn done_todo(id: &str, stamp_secs: i64) -> Todo {
        let mut t = todo(id, id, stamp_secs);
        t.done = true;
        t
    }

    #[test]
    fn completed_tasks_archive_once_they_have_aged() {
        let now = Utc::now();
        let old = (now - chrono::Duration::days(45)).timestamp();
        let mut local = vec![done_todo("stale", old)];

        assert_eq!(archive_completed(&mut local, 30, now), 1);
        assert!(local[0].is_archived());
        assert!(local[0].dirty, "archiving must be pushed to the other machine");
    }

    #[test]
    fn recently_completed_tasks_stay_in_the_list() {
        let now = Utc::now();
        let recent = (now - chrono::Duration::days(3)).timestamp();
        let mut local = vec![done_todo("fresh", recent)];

        assert_eq!(archive_completed(&mut local, 30, now), 0);
        assert!(!local[0].is_archived());
    }

    #[test]
    fn unfinished_tasks_are_never_archived_however_old() {
        let now = Utc::now();
        let ancient = (now - chrono::Duration::days(400)).timestamp();
        let mut local = vec![todo("still open", "still open", ancient)];

        assert_eq!(archive_completed(&mut local, 30, now), 0);
        assert!(!local[0].is_archived(), "an old open task is still work to do");
    }

    #[test]
    fn already_archived_tasks_are_left_alone() {
        let now = Utc::now();
        let old = (now - chrono::Duration::days(45)).timestamp();
        let mut local = vec![done_todo("stale", old)];
        local[0].archived_at = Some(now - chrono::Duration::days(10));
        local[0].dirty = false;

        assert_eq!(archive_completed(&mut local, 30, now), 0);
        assert!(!local[0].dirty, "re-archiving would push a pointless update");
    }

    #[test]
    fn tombstones_are_not_archived() {
        let now = Utc::now();
        let old = (now - chrono::Duration::days(45)).timestamp();
        let mut local = vec![done_todo("deleted", old)];
        local[0].deleted_at = Some(now);

        assert_eq!(archive_completed(&mut local, 30, now), 0);
        assert!(!local[0].is_archived(), "a deleted task should not also be archived");
    }

    #[test]
    fn zero_days_archives_everything_done_for_the_manual_action() {
        let now = Utc::now();
        let mut local = vec![
            done_todo("just finished", now.timestamp()),
            done_todo("finished last week", (now - chrono::Duration::days(7)).timestamp()),
            todo("open", "open", now.timestamp()),
        ];

        assert_eq!(archive_completed(&mut local, 0, now), 2);
        assert!(local[0].is_archived());
        assert!(local[1].is_archived());
        assert!(!local[2].is_archived(), "the open task must survive");
    }

    #[test]
    fn old_tombstones_are_purged_but_unpushed_ones_survive() {
        let old = Utc::now() - chrono::Duration::days(TOMBSTONE_TTL_DAYS + 1);
        let mut a = todo("a", "old tombstone", 0);
        a.deleted_at = Some(old);
        let mut b = todo("b", "old but unpushed", 0);
        b.deleted_at = Some(old);
        b.dirty = true;
        let mut c = todo("c", "live", 0);
        c.deleted_at = None;

        let mut local = vec![a, b, c];
        let purged = purge_local_tombstones(&mut local);
        assert_eq!(purged, 1);
        let ids: Vec<_> = local.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, vec!["b", "c"]);
    }

    /* ---------- end-to-end against a real database ---------- */

    // Run with:
    //   TODO_TEST_DATABASE_URL=<url> cargo test -- --ignored
    // Only rows prefixed `zz-sync-` are touched.

    const E2E_PREFIX: &str = "zz-sync-";

    fn e2e_url() -> Option<String> {
        std::env::var("TODO_TEST_DATABASE_URL")
            .ok()
            .filter(|s| !s.is_empty())
    }

    fn temp_state(tag: &str) -> SyncState {
        let dir = std::env::temp_dir().join(format!("todo-sync-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        SyncState::new(dir)
    }

    async fn e2e_cleanup(pool: &PgPool) {
        let _ = sqlx::query("delete from todos where id like $1")
            .bind(format!("{E2E_PREFIX}%"))
            .execute(pool)
            .await;
    }

    fn e2e_todo(id: &str, title: &str) -> Todo {
        let mut t = todo(&format!("{E2E_PREFIX}{id}"), title, 0);
        t.created_at = Utc::now();
        t.updated_at = Some(Utc::now());
        t.dirty = true;
        t
    }

    /// The whole point of the feature: an edit here shows up there, and an edit
    /// there shows up here.
    #[tokio::test]
    #[ignore]
    async fn a_local_edit_reaches_the_database_and_a_remote_edit_comes_back() {
        let Some(url) = e2e_url() else { return };

        let state = temp_state("roundtrip");
        connect(&state, &url).await.expect("connect");
        let pool = state.pool.read().await.clone().unwrap();
        e2e_cleanup(&pool).await;

        // First sync adopts the shared list; after it the machine is in steady state.
        let s = run_sync(&state).await;
        assert_eq!(s.state, "ok", "first sync failed: {:?}", s.error);
        assert!(state.meta.lock().unwrap().first_sync_done);

        // --- outbound: a local edit must reach Neon ---
        state
            .todos
            .lock()
            .unwrap()
            .push(e2e_todo("outbound", "written on this machine"));
        assert_eq!(state.pending_count(), 1, "a new row starts dirty");

        let s = run_sync(&state).await;
        assert_eq!(s.state, "ok", "sync failed: {:?}", s.error);
        assert_eq!(state.pending_count(), 0, "a pushed row must be marked clean");

        let (remote, _) = db::pull(&pool, None).await.expect("pull");
        let landed = remote
            .iter()
            .find(|t| t.id == format!("{E2E_PREFIX}outbound"))
            .expect("the local edit must exist in the database");
        assert_eq!(landed.title, "written on this machine");

        // --- inbound: an edit made elsewhere must come back ---
        // Stand in for the other machine by writing straight to the database.
        let mut from_elsewhere = e2e_todo("inbound", "written on the other machine");
        from_elsewhere.dirty = false;
        db::push(&pool, &[from_elsewhere]).await.expect("remote write");

        let s = run_sync(&state).await;
        assert_eq!(s.state, "ok", "sync failed: {:?}", s.error);
        assert!(s.changed, "a pulled row must be reported as a change");

        let local = state.todos.lock().unwrap();
        let pulled = local
            .iter()
            .find(|t| t.id == format!("{E2E_PREFIX}inbound"))
            .expect("the other machine's row must arrive locally");
        assert_eq!(pulled.title, "written on the other machine");
        drop(local);

        e2e_cleanup(&pool).await;
        let _ = std::fs::remove_dir_all(&state.dir);
    }

    /// An edit made with no network must survive and push once it returns.
    #[tokio::test]
    #[ignore]
    async fn an_offline_edit_is_kept_and_pushed_on_reconnect() {
        let Some(url) = e2e_url() else { return };

        let state = temp_state("offline");
        connect(&state, &url).await.expect("connect");
        let pool = state.pool.read().await.clone().unwrap();
        e2e_cleanup(&pool).await;
        run_sync(&state).await;

        // Go offline and edit.
        *state.pool.write().await = None;
        state
            .todos
            .lock()
            .unwrap()
            .push(e2e_todo("offline", "typed on a plane"));

        let s = run_sync(&state).await;
        assert_eq!(s.state, "unconfigured", "no pool means nothing to sync to");
        assert_eq!(state.pending_count(), 1, "the edit must be kept, not dropped");

        // Reconnect.
        connect(&state, &url).await.expect("reconnect");
        let s = run_sync(&state).await;
        assert_eq!(s.state, "ok", "sync failed: {:?}", s.error);
        assert_eq!(state.pending_count(), 0, "the queued edit must flush");

        let (remote, _) = db::pull(&pool, None).await.expect("pull");
        assert!(
            remote.iter().any(|t| t.id == format!("{E2E_PREFIX}offline")),
            "the offline edit must reach the database after reconnecting"
        );

        e2e_cleanup(&pool).await;
        let _ = std::fs::remove_dir_all(&state.dir);
    }

    /// A delete must not be resurrected by the other machine's copy.
    #[tokio::test]
    #[ignore]
    async fn a_delete_propagates_instead_of_being_undone() {
        let Some(url) = e2e_url() else { return };

        let state = temp_state("delete");
        connect(&state, &url).await.expect("connect");
        let pool = state.pool.read().await.clone().unwrap();
        e2e_cleanup(&pool).await;
        run_sync(&state).await;

        let id = format!("{E2E_PREFIX}doomed");
        state.todos.lock().unwrap().push(e2e_todo("doomed", "delete me"));
        run_sync(&state).await;

        // Delete it the way the command does.
        {
            let mut t = state.todos.lock().unwrap();
            let row = t.iter_mut().find(|t| t.id == id).unwrap();
            let now = Utc::now();
            row.deleted_at = Some(now);
            row.updated_at = Some(now);
            row.dirty = true;
        }
        let s = run_sync(&state).await;
        assert_eq!(s.state, "ok", "sync failed: {:?}", s.error);

        // The database holds a tombstone, not a live row.
        let (remote, _) = db::pull(&pool, None).await.expect("pull");
        let tomb = remote.iter().find(|t| t.id == id).expect("row must still exist");
        assert!(tomb.is_deleted(), "the delete must be recorded as a tombstone");

        // And it is hidden from the UI.
        assert!(
            !state.live_todos().iter().any(|t| t.id == id),
            "a deleted row must not be shown"
        );

        e2e_cleanup(&pool).await;
        let _ = std::fs::remove_dir_all(&state.dir);
    }

    #[test]
    fn legacy_todos_json_deserializes_without_sync_fields() {
        // Exactly the shape the app has been writing until now.
        let raw = r#"[{
            "id":"0f7d8008-c04f-4710-8e01-0f4cee839445",
            "title":"Renew the domain",
            "note":"Before it lapses",
            "due":"2026-06-12",
            "done":true,
            "createdAt":"2026-06-03T16:25:53.194Z",
            "link":null,
            "tags":["admin"],
            "refs":[],
            "priority":"high"
        },{
            "id":"2cd374a8-b38b-4032-a860-71f31a951463",
            "title":"Draft the trip itinerary",
            "note":"Flights first",
            "link":null,
            "due":"2026-06-26",
            "done":true,
            "createdAt":"2026-06-03T16:35:43.892Z"
        }]"#;
        let todos: Vec<Todo> = serde_json::from_str(raw).expect("legacy file must still parse");
        assert_eq!(todos.len(), 2);
        assert_eq!(todos[0].tags, vec!["admin"]);
        assert!(todos[1].tags.is_empty(), "missing tags default to empty");
        assert!(todos[0].updated_at.is_none());
        assert!(!todos[0].dirty);
        assert!(!todos[0].is_deleted());
    }
}
