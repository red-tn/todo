// main.js — wires the window chrome, modules, and data flow together.
import { load, subscribe, toggleDone, deleteTodo } from "./store.js";
import { initRender, renderList, toggleTagPanel, toggleSettings, toggleArchive } from "./render.js";
import { initArchivePanel, renderArchive } from "./archive.js";
import { initAddForm, openEdit } from "./addform.js";
import { initTheme, getTheme, setTheme } from "./theme.js";
import { initSync, subscribeStatus, setDbUrl, syncNow, describe } from "./sync.js";
import {
  initUpdates,
  subscribeUpdate,
  checkForUpdate,
  installUpdate,
  describeUpdate,
} from "./update.js";

const appWindow = window.__TAURI__.window.getCurrentWindow();
const invoke = window.__TAURI__.core.invoke;

function setDate() {
  const now = new Date();
  const wd = now.toLocaleDateString(undefined, { weekday: "short" });
  const md = now.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  document.getElementById("topbar-date").textContent = `${wd} · ${md}`;
}

function initWindowChrome() {
  document.getElementById("btn-tags").addEventListener("click", () => toggleTagPanel());
  document.getElementById("btn-archive").addEventListener("click", () => toggleArchive());
  document.getElementById("btn-settings").addEventListener("click", () => toggleSettings());
  document.getElementById("btn-min").addEventListener("click", () => appWindow.minimize());
  document.getElementById("btn-close").addEventListener("click", () => appWindow.close());

  const grip = document.getElementById("resize-grip");
  grip.addEventListener("mousedown", (e) => {
    e.preventDefault();
    appWindow.startResizeDragging("SouthEast");
  });
}

function initSettings() {
  const seg = document.getElementById("theme-seg");
  const buttons = Array.from(seg.querySelectorAll("button"));
  const refresh = () => {
    const cur = getTheme();
    for (const b of buttons) b.classList.toggle("active", b.dataset.themePref === cur);
  };
  for (const b of buttons) {
    b.addEventListener("click", () => {
      setTheme(b.dataset.themePref);
      refresh();
    });
  }
  refresh();
}

function initSyncSettings() {
  const input = document.getElementById("f-dburl");
  const save = document.getElementById("btn-dburl-save");
  const sync = document.getElementById("btn-sync");
  const line = document.getElementById("sync-status");
  const dot = document.getElementById("sync-dot");

  subscribeStatus((s) => {
    line.textContent = describe(s);
    line.classList.toggle("is-error", s.state === "error");
    dot.className = "sync-dot " + s.state;
    // The credential is never returned to the frontend, so show the masked host
    // as a placeholder to signal that one is already saved.
    input.placeholder = s.host ? `Connected to ${s.host}` : "postgresql://…";
  });

  save.addEventListener("click", async () => {
    save.disabled = true;
    const previous = save.textContent;
    save.textContent = "Checking…";
    try {
      await setDbUrl(input.value);
      input.value = ""; // never keep the credential in the DOM
    } catch (err) {
      line.textContent = `Could not connect: ${err}`;
      line.classList.add("is-error");
    } finally {
      save.disabled = false;
      save.textContent = previous;
    }
  });

  sync.addEventListener("click", () => syncNow());
}

/** Pull the archived list and hand it to the panel. */
async function refreshArchive() {
  try {
    renderArchive(await invoke("archived_todos"));
  } catch (err) {
    console.error("archived_todos failed:", err);
    renderArchive([]);
  }
}

function initArchive() {
  document.getElementById("btn-archive-done").addEventListener("click", async (e) => {
    const btn = e.currentTarget;
    btn.disabled = true;
    try {
      const n = await invoke("archive_all_done");
      // Say what happened rather than silently emptying the Done section.
      btn.textContent = n === 1 ? "1 archived" : `${n} archived`;
      await load();
      setTimeout(() => {
        btn.textContent = "Archive all";
      }, 2000);
    } catch (err) {
      console.error("archive_all_done failed:", err);
      btn.textContent = "Failed";
    } finally {
      btn.disabled = false;
    }
  });
}

async function initAutostartSetting() {
  const seg = document.getElementById("autostart-seg");
  const buttons = Array.from(seg.querySelectorAll("button"));
  const paint = (on) => {
    for (const b of buttons) b.classList.toggle("active", (b.dataset.autostart === "on") === on);
  };

  let enabled = false;
  try {
    enabled = await invoke("get_autostart");
  } catch (err) {
    console.error("get_autostart failed:", err);
  }
  paint(enabled);

  for (const b of buttons) {
    b.addEventListener("click", async () => {
      const want = b.dataset.autostart === "on";
      try {
        // Paint what actually took effect, not what was asked for — the OS can
        // refuse, and a toggle that lies is worse than one that fails visibly.
        paint(await invoke("set_autostart", { enabled: want }));
      } catch (err) {
        console.error("set_autostart failed:", err);
        paint(!want);
      }
    });
  }
}

async function initDigestSettings() {
  const seg = document.getElementById("digest-seg");
  const buttons = Array.from(seg.querySelectorAll("button"));
  const time = document.getElementById("f-digest-time");
  const preview = document.getElementById("btn-digest-preview");
  const line = document.getElementById("digest-status");

  const paint = (cfg) => {
    for (const b of buttons) b.classList.toggle("active", (b.dataset.digest === "on") === cfg.enabled);
    time.value = cfg.time;
    time.disabled = !cfg.enabled;
    preview.disabled = !cfg.enabled;
    line.textContent = cfg.enabled
      ? `One notification a day at ${cfg.time}, listing what's due and overdue.`
      : "Off — no notifications from this machine.";
  };

  let cfg = { enabled: true, time: "08:00" };
  try {
    cfg = await invoke("get_digest_config");
  } catch (err) {
    console.error("get_digest_config failed:", err);
  }
  paint(cfg);

  const save = async (next) => {
    try {
      paint(await invoke("set_digest_config", next));
    } catch (err) {
      line.textContent = `Could not save: ${err}`;
      line.classList.add("is-error");
    }
  };

  for (const b of buttons) {
    b.addEventListener("click", () => save({ enabled: b.dataset.digest === "on", time: time.value }));
  }
  time.addEventListener("change", () => save({ enabled: true, time: time.value }));

  // Proves notifications actually reach the desktop, which is otherwise only
  // discoverable by waiting until tomorrow morning.
  preview.addEventListener("click", async () => {
    preview.disabled = true;
    try {
      line.textContent = await invoke("preview_digest");
      line.classList.remove("is-error");
    } catch (err) {
      line.textContent = `Preview failed: ${err}`;
      line.classList.add("is-error");
    } finally {
      preview.disabled = false;
    }
  });
}

/**
 * Badge the settings gear when an update is waiting.
 *
 * Settings is where the Install button lives, so the dot points at its own
 * action rather than just announcing something.
 */
function initUpdateBadge() {
  const badge = document.getElementById("update-badge");
  const gear = document.getElementById("btn-settings");
  subscribeUpdate((s) => {
    const waiting = s.state === "available";
    badge.hidden = !waiting;
    gear.title = waiting ? `Update available — v${s.availableVersion}` : "Settings";
  });
}

function initUpdateSettings() {
  const line = document.getElementById("update-status");
  const dot = document.getElementById("update-dot");
  const check = document.getElementById("btn-check-update");
  const install = document.getElementById("btn-install-update");

  subscribeUpdate((s) => {
    line.textContent = describeUpdate(s);
    line.classList.toggle("is-error", s.state === "error");
    dot.className = "sync-dot " + (s.state === "available" ? "offline" : s.state === "error" ? "error" : "ok");
    // The install button only exists when there is genuinely something to install.
    install.hidden = s.state !== "available";
    check.disabled = s.state === "downloading";
    install.disabled = s.state === "downloading";
  });

  check.addEventListener("click", async () => {
    check.disabled = true;
    const previous = check.textContent;
    check.textContent = "Checking…";
    try {
      await checkForUpdate();
    } finally {
      check.disabled = false;
      check.textContent = previous;
    }
  });

  // The only path that restarts the app, and only ever from this click.
  install.addEventListener("click", () => {
    install.textContent = "Installing…";
    installUpdate().catch(() => {
      install.textContent = "Install and restart";
    });
  });
}

// type="module" scripts run after the DOM is parsed, so elements exist here.
initTheme(); // apply saved theme before first paint
setDate();
initWindowChrome();
initSettings();
initSyncSettings();
initAutostartSetting();
initDigestSettings();
initUpdateSettings();
initUpdateBadge();
initAddForm();
initArchive();
initArchivePanel({
  onRestore: async (id) => {
    await invoke("restore_todo", { id });
    await Promise.all([load(), refreshArchive()]);
  },
});
initRender({
  onToggle: toggleDone,
  onDelete: deleteTodo,
  onEdit: openEdit,
  onOpenArchive: refreshArchive,
});
subscribe(renderList);
load();
initSync();
initUpdates();
