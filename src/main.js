// main.js — wires the window chrome, modules, and data flow together.
import { load, subscribe, toggleDone, deleteTodo } from "./store.js";
import { initRender, renderList, toggleTagPanel, toggleSettings } from "./render.js";
import { initAddForm, openEdit } from "./addform.js";
import { initTheme, getTheme, setTheme } from "./theme.js";
import { initSync, subscribeStatus, setDbUrl, syncNow, describe } from "./sync.js";

const appWindow = window.__TAURI__.window.getCurrentWindow();

function setDate() {
  const now = new Date();
  const wd = now.toLocaleDateString(undefined, { weekday: "short" });
  const md = now.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  document.getElementById("topbar-date").textContent = `${wd} · ${md}`;
}

function initWindowChrome() {
  document.getElementById("btn-tags").addEventListener("click", () => toggleTagPanel());
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

// type="module" scripts run after the DOM is parsed, so elements exist here.
initTheme(); // apply saved theme before first paint
setDate();
initWindowChrome();
initSettings();
initSyncSettings();
initAddForm();
initRender({ onToggle: toggleDone, onDelete: deleteTodo, onEdit: openEdit });
subscribe(renderList);
load();
initSync();
