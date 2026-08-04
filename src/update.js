// update.js — checking for releases and installing them, with consent.
//
// The app never restarts itself unasked: an update installs only when the user
// presses the button, because a surprise restart would discard whatever they
// were typing.
const invoke = window.__TAURI__.core.invoke;

let status = { state: "idle", currentVersion: "", availableVersion: null, notes: null, error: null };
const listeners = [];
let checking = false;

/** Register a function called with the update status whenever it changes. */
export function subscribeUpdate(fn) {
  listeners.push(fn);
  fn(status);
}

function setStatus(next) {
  status = next;
  for (const fn of listeners) fn(status);
}

export function getUpdateStatus() {
  return status;
}

/** Ask GitHub whether a newer release exists. Safe to call when offline. */
export async function checkForUpdate() {
  if (checking) return status;
  checking = true;
  try {
    setStatus(await invoke("check_update"));
  } catch (err) {
    console.error("check_update failed:", err);
  } finally {
    checking = false;
  }
  return status;
}

/**
 * Install the pending update and restart.
 *
 * On success the process is replaced, so nothing after this resolves.
 */
export async function installUpdate() {
  setStatus({ ...status, state: "downloading" });
  try {
    await invoke("install_update");
  } catch (err) {
    setStatus({ ...status, state: "error", error: String(err) });
    throw err;
  }
}

/** One-line description of update state for the settings panel. */
export function describeUpdate(s) {
  switch (s.state) {
    case "available":
      return `Update available — v${s.availableVersion}`;
    case "downloading":
      return "Downloading update… the app will restart when it finishes.";
    case "error":
      return `Update check failed: ${s.error || "unknown"}`;
    case "idle":
    default:
      return s.currentVersion ? `Up to date — v${s.currentVersion}` : "Checking…";
  }
}

/** Check once at startup, quietly. */
export async function initUpdates() {
  await checkForUpdate();
}
