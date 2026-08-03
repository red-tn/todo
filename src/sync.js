// sync.js — sync status, the connection-string setting, and the refresh triggers.
//
// The webview never holds the credential: it can write one in and read a masked
// status back, but there is no command that returns the string.
import { load } from "./store.js";

const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;
const appWindow = window.__TAURI__.window.getCurrentWindow();

/**
 * How often to poll the database for changes made on another machine.
 *
 * Set to 0 to disable polling and rely on launch and window focus alone. Note
 * that any interval shorter than Neon's auto-suspend window (5 minutes by
 * default) keeps its compute awake, whatever the value.
 */
const POLL_MS = 60000;

let status = { state: "unconfigured", host: null, pending: 0, lastSync: null, error: null };
const listeners = [];
let inFlight = false;

/** Register a function called with the sync status whenever it changes. */
export function subscribeStatus(fn) {
  listeners.push(fn);
  fn(status);
}

function setStatus(next) {
  status = next;
  for (const fn of listeners) fn(status);
}

export function getStatus() {
  return status;
}

/** Save a connection string, or clear it by passing "". Rejects if it can't connect. */
export async function setDbUrl(url) {
  const next = await invoke("set_db_url", { url });
  setStatus(next);
  await load();
  return next;
}

/**
 * Pull and push right now.
 *
 * Overlapping calls are skipped rather than queued: on a slow connection a sync
 * can outlast the poll interval, and stacking them would only pile up work.
 */
export async function syncNow() {
  if (inFlight) return status;
  inFlight = true;
  try {
    const next = await invoke("sync_now");
    setStatus(next);
    if (next.changed) await load();
    return next;
  } catch (err) {
    console.error("sync_now failed:", err);
    return status;
  } finally {
    inFlight = false;
  }
}

/** Human-readable "2m ago" style stamp. */
function ago(iso) {
  const secs = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 1000));
  if (secs < 45) return "just now";
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return `${Math.round(hrs / 24)}d ago`;
}

/** One-line description of sync state for the settings panel. */
export function describe(s) {
  switch (s.state) {
    case "unconfigured":
      return "Not configured — this list is local to this machine.";
    case "ok":
      return s.pending > 0
        ? `Synced · ${s.pending} pending`
        : s.lastSync
          ? `Synced · ${ago(s.lastSync)}`
          : "Synced";
    case "offline":
      return s.pending > 0 ? `Offline · ${s.pending} pending` : "Offline — will sync when reconnected.";
    case "error":
      return `Sync error: ${s.error || "unknown"}`;
    default:
      return s.state;
  }
}

/**
 * Start listening for status changes and keep the list fresh.
 *
 * Three triggers: a 60s poll so an open window updates itself, window focus for
 * the moment you come back to a machine, and the status events Rust emits after
 * its own background syncs.
 */
export async function initSync() {
  try {
    setStatus(await invoke("get_sync_status"));
  } catch (err) {
    console.error("get_sync_status failed:", err);
  }

  // Rust pushes a status after every background sync.
  await listen("sync-status", async (event) => {
    setStatus(event.payload);
    if (event.payload.changed) await load();
  });

  // The sync Rust starts at launch can finish before the listener above is
  // attached, so its event would be missed. Re-read the cache once here rather
  // than leaving the list stale until the next focus.
  await load();

  await appWindow.onFocusChanged(({ payload: focused }) => {
    if (focused) syncNow();
  });

  if (POLL_MS > 0) setInterval(syncNow, POLL_MS);
}
