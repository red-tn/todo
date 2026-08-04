// render.js — sorting, due tiers, card rendering, the tag panel, and jump-to-highlight.
const invoke = window.__TAURI__.core.invoke;
const els = {};
let handlers = {};
let doneOpen = false;
let overlay = null; // null | "tags" | "settings"
let current = []; // last rendered todo list, for the tag panel

const SORT_KEY = "todo.sortMode";
const SORT_MODES = ["due", "priority", "tag", "added"];
let sortMode = "due";

/** Grab element refs and wire the Done toggle + tag panel back button. */
export function initRender(h) {
  handlers = h;
  els.listView = document.getElementById("list-view");
  els.list = document.getElementById("list");
  els.empty = document.getElementById("empty");
  els.doneBlock = document.getElementById("done-block");
  els.doneList = document.getElementById("done-list");
  els.doneToggle = document.getElementById("done-toggle");
  els.doneChev = document.getElementById("done-chev");
  els.doneCount = document.getElementById("done-count");

  els.tagPanel = document.getElementById("tag-panel");
  els.tagList = document.getElementById("tag-list");
  els.tagEmpty = document.getElementById("tag-empty");
  els.tagBack = document.getElementById("tag-back");
  els.settingsPanel = document.getElementById("settings-panel");
  els.settingsBack = document.getElementById("settings-back");

  els.doneToggle.addEventListener("click", () => {
    doneOpen = !doneOpen;
    els.doneList.hidden = !doneOpen;
    els.doneChev.classList.toggle("open", doneOpen);
  });
  els.tagBack.addEventListener("click", closeOverlay);
  els.settingsBack.addEventListener("click", closeOverlay);

  els.sortSeg = document.getElementById("sort-seg");
  els.sortBtns = Array.from(els.sortSeg.querySelectorAll("button"));
  loadSortMode();
  for (const b of els.sortBtns) {
    b.addEventListener("click", () => {
      sortMode = b.dataset.sort;
      saveSortMode();
      refreshSortButtons();
      renderList(current);
    });
  }
  refreshSortButtons();
}

/* ---------- sort mode ---------- */

function loadSortMode() {
  try {
    const s = localStorage.getItem(SORT_KEY);
    if (SORT_MODES.includes(s)) sortMode = s;
  } catch (err) {
    /* localStorage unavailable — keep default */
  }
}
function saveSortMode() {
  try {
    localStorage.setItem(SORT_KEY, sortMode);
  } catch (err) {
    /* ignore persistence failures */
  }
}
function refreshSortButtons() {
  for (const b of els.sortBtns) b.classList.toggle("active", b.dataset.sort === sortMode);
}

/* ---------- dates / tiers ---------- */

function dayDiff(dueStr) {
  const [y, m, d] = dueStr.split("-").map(Number);
  const due = new Date(y, m - 1, d);
  const now = new Date();
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  return Math.round((due - today) / 86400000);
}
function prettyDate(dueStr) {
  const [y, m, d] = dueStr.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
function tierFor(t) {
  if (!t.due) return { tier: "", label: "" };
  const diff = dayDiff(t.due);
  if (diff < 0)
    return { tier: "over", label: diff === -1 ? "Overdue · yesterday" : `Overdue · ${-diff} days` };
  if (diff === 0) return { tier: "today", label: "Due today" };
  if (diff <= 3) return { tier: "soon", label: diff === 1 ? "Tomorrow" : `In ${diff} days` };
  return { tier: "", label: prettyDate(t.due) };
}
function sortValue(t) {
  return t.due ? dayDiff(t.due) : Infinity;
}
function byCreated(a, b) {
  return (a.createdAt || "").localeCompare(b.createdAt || "");
}
function byUrgency(a, b) {
  const d = sortValue(a) - sortValue(b);
  return d !== 0 ? d : byCreated(a, b);
}

const PRIO_RANK = { high: 0, med: 1, low: 2 };
function prioRank(t) {
  return t.priority in PRIO_RANK ? PRIO_RANK[t.priority] : 3;
}
function firstTag(t) {
  const tags = t.tags || [];
  return tags.length ? [...tags].sort()[0] : null;
}

/** Comparator for the active-task list, chosen by the sort bar. */
function comparatorFor(mode) {
  switch (mode) {
    case "priority":
      return (a, b) => prioRank(a) - prioRank(b) || sortValue(a) - sortValue(b) || byCreated(a, b);
    case "tag":
      return (a, b) => {
        const ta = firstTag(a);
        const tb = firstTag(b);
        if (ta === null && tb === null) return byCreated(a, b);
        if (ta === null) return 1; // untagged last
        if (tb === null) return -1;
        return ta.localeCompare(tb) || byCreated(a, b);
      };
    case "added":
      return (a, b) => byCreated(b, a); // newest first
    case "due":
    default:
      return byUrgency;
  }
}

/* ---------- links ---------- */

function svg(viewBox, inner) {
  return `<svg viewBox="${viewBox}">${inner}</svg>`;
}
function normalizeUrl(raw) {
  const s = raw.trim();
  return /^[a-z][a-z0-9+.-]*:\/\//i.test(s) ? s : "https://" + s;
}
function displayLink(raw) {
  return raw.trim().replace(/^[a-z][a-z0-9+.-]*:\/\//i, "").replace(/\/$/, "");
}

/* ---------- recurrence ---------- */

/** "monthly" -> "monthly"; interval 2 -> "every 2 weeks". */
function recurrenceLabel(t) {
  const n = t.recurrenceInterval || 1;
  if (n === 1) return t.recurrence;
  const plural = { daily: "days", weekly: "weeks", monthly: "months", yearly: "years" };
  return `every ${n} ${plural[t.recurrence] || t.recurrence}`;
}

/* ---------- card ---------- */

function itemEl(t, byId) {
  const { tier, label } = t.done ? { tier: "", label: "" } : tierFor(t);
  const li = document.createElement("li");
  li.className =
    "item enter" +
    (tier ? " " + tier : "") +
    (t.priority ? " prio-" + t.priority : "") +
    (t.done ? " is-done" : "");
  li.dataset.id = t.id;

  const check = document.createElement("button");
  check.className = "check";
  check.title = t.done ? "Mark as not done" : "Mark as done";
  check.innerHTML = svg("0 0 12 12", '<polyline points="2,6.5 5,9.5 10,3.5" />');
  check.addEventListener("click", (e) => {
    e.stopPropagation();
    handlers.onToggle(t.id);
  });

  const body = document.createElement("div");
  body.className = "item-body";

  const title = document.createElement("div");
  title.className = "item-title";
  title.textContent = t.title;
  body.appendChild(title);

  if (t.note) {
    const note = document.createElement("div");
    note.className = "item-note";
    note.textContent = t.note;
    body.appendChild(note);
  }

  if (t.link) {
    const a = document.createElement("div");
    a.className = "item-link";
    a.title = t.link;
    a.innerHTML =
      svg(
        "0 0 14 14",
        '<path d="M6 8 a3 3 0 0 0 4 0 l2 -2 a3 3 0 0 0 -4 -4 l-1 1 M8 6 a3 3 0 0 0 -4 0 l-2 2 a3 3 0 0 0 4 4 l1 -1" />'
      ) + "<span></span>";
    a.querySelector("span").textContent = displayLink(t.link);
    a.addEventListener("click", (e) => {
      e.stopPropagation();
      invoke("open_link", { url: normalizeUrl(t.link) }).catch((err) =>
        console.error("open_link failed:", err)
      );
    });
    body.appendChild(a);
  }

  if (!t.done && t.due) {
    const meta = document.createElement("div");
    meta.className = "item-meta";
    meta.innerHTML = '<span class="item-dot"></span><span class="item-meta-text"></span>';
    meta.querySelector(".item-meta-text").textContent = label;
    body.appendChild(meta);
  }

  if (t.recurrence) {
    const chip = document.createElement("div");
    chip.className = "item-recur";
    chip.textContent = "↻ " + recurrenceLabel(t);
    chip.title = "Completing this creates the next one";
    body.appendChild(chip);
  }

  // Reference chips → jump to the referenced task
  const refs = (t.refs || []).filter((id) => byId.has(id));
  if (refs.length) {
    const row = document.createElement("div");
    row.className = "item-refs";
    for (const id of refs) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "ref-pill";
      chip.textContent = "↳ " + byId.get(id).title;
      chip.title = "Go to referenced task";
      chip.addEventListener("click", (e) => {
        e.stopPropagation();
        jumpTo(id);
      });
      row.appendChild(chip);
    }
    body.appendChild(row);
  }

  // Tag chips → open the tag panel focused on that tag
  const tags = t.tags || [];
  if (tags.length) {
    const row = document.createElement("div");
    row.className = "item-tags";
    for (const tag of tags) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "tag-pill";
      chip.textContent = "#" + tag;
      chip.addEventListener("click", (e) => {
        e.stopPropagation();
        openTagPanel(tag);
      });
      row.appendChild(chip);
    }
    body.appendChild(row);
  }

  if (!t.done) {
    body.title = "Click to edit";
    body.addEventListener("click", () => handlers.onEdit(t));
  }

  const del = document.createElement("button");
  del.className = "item-del";
  del.title = "Delete";
  del.innerHTML = svg(
    "0 0 14 14",
    '<polyline points="2.5,4 11.5,4" /><path d="M5.4 4 V2.9 h3.2 V4 M4 4 l.6 8 h4.8 l.6 -8" />'
  );
  del.addEventListener("click", (e) => {
    e.stopPropagation();
    handlers.onDelete(t.id);
  });

  li.append(check, body, del);
  return li;
}

/* ---------- list render ---------- */

export function renderList(todos) {
  current = todos;
  const byId = new Map(todos.map((t) => [t.id, t]));
  const active = todos.filter((t) => !t.done).sort(comparatorFor(sortMode));
  const done = todos
    .filter((t) => t.done)
    .sort((a, b) => (b.createdAt || "").localeCompare(a.createdAt || ""));

  els.list.replaceChildren(...active.map((t) => itemEl(t, byId)));
  els.empty.hidden = todos.length > 0;

  if (done.length) {
    els.doneBlock.hidden = false;
    els.doneCount.textContent = String(done.length);
    els.doneList.replaceChildren(...done.map((t) => itemEl(t, byId)));
    els.doneList.hidden = !doneOpen;
    els.doneChev.classList.toggle("open", doneOpen);
  } else {
    els.doneBlock.hidden = true;
  }

  if (overlay === "tags") renderTagPanel();
}

/* ---------- jump + highlight ---------- */

function jumpTo(id) {
  if (overlay) closeOverlay();
  const li = els.list.querySelector(`[data-id="${id}"]`);
  if (!li) return; // referenced task may be completed/filtered
  li.scrollIntoView({ behavior: "smooth", block: "center" });
  li.classList.remove("flash");
  void li.offsetWidth; // restart animation
  li.classList.add("flash");
}

/* ---------- tag panel ---------- */

function setOverlay(name) {
  overlay = name;
  els.listView.hidden = name !== null;
  els.tagPanel.hidden = name !== "tags";
  els.settingsPanel.hidden = name !== "settings";
}
function closeOverlay() {
  setOverlay(null);
}

export function toggleTagPanel() {
  if (overlay === "tags") return closeOverlay();
  setOverlay("tags");
  renderTagPanel();
}

export function toggleSettings() {
  setOverlay(overlay === "settings" ? null : "settings");
}

function openTagPanel(expandTag) {
  setOverlay("tags");
  renderTagPanel(expandTag);
}

function renderTagPanel(expandTag) {
  // tag -> list of todos
  const map = new Map();
  for (const t of current) {
    for (const tag of t.tags || []) {
      if (!map.has(tag)) map.set(tag, []);
      map.get(tag).push(t);
    }
  }
  const tags = [...map.keys()].sort(
    (a, b) => map.get(b).length - map.get(a).length || a.localeCompare(b)
  );

  els.tagEmpty.hidden = tags.length > 0;
  els.tagList.replaceChildren(
    ...tags.map((tag) => {
      const items = map.get(tag);
      const entry = document.createElement("div");
      entry.className = "tag-entry";

      const row = document.createElement("button");
      row.type = "button";
      row.className = "tag-row";
      row.innerHTML =
        '<span class="tchev">▸</span><span class="tname"></span><span class="tag-count"></span>';
      row.querySelector(".tname").textContent = "#" + tag;
      row.querySelector(".tag-count").textContent = String(items.length);

      const list = document.createElement("ul");
      list.className = "tag-todos";
      list.hidden = tag !== expandTag;
      if (tag === expandTag) row.querySelector(".tchev").classList.add("open");

      for (const t of items) {
        const li = document.createElement("li");
        li.className = "tag-todo" + (t.done ? " is-done" : "");
        li.textContent = t.title;
        li.title = "Go to task";
        li.addEventListener("click", () => jumpTo(t.id));
        list.appendChild(li);
      }

      row.addEventListener("click", () => {
        const open = list.hidden;
        list.hidden = !open;
        row.querySelector(".tchev").classList.toggle("open", open);
      });

      entry.append(row, list);
      return entry;
    })
  );
}
