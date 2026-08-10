// render.js — sorting, due tiers, card rendering, the tag filter, and the
// tag/archive panels.
const els = {};
let handlers = {};
let doneOpen = false;
let overlay = null; // null | "tags" | "settings" | "archive"
let current = []; // last rendered todo list, for the tag panel
let activeTag = null; // tag filter, session-only

const SORT_KEY = "todo.sortMode";
const SORT_MODES = ["due", "priority", "tag", "added"];
let sortMode = "due";

/** Grab element refs and wire the Done toggle, filter, and panel back buttons. */
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

  els.filterBar = document.getElementById("filter-bar");
  els.filterChip = document.getElementById("filter-chip");
  els.filterClear = document.getElementById("filter-clear");

  els.tagPanel = document.getElementById("tag-panel");
  els.tagList = document.getElementById("tag-list");
  els.tagEmpty = document.getElementById("tag-empty");
  els.tagBack = document.getElementById("tag-back");
  els.settingsPanel = document.getElementById("settings-panel");
  els.settingsBack = document.getElementById("settings-back");
  els.archivePanel = document.getElementById("archive-panel");
  els.archiveBack = document.getElementById("archive-back");

  els.doneToggle.addEventListener("click", () => {
    doneOpen = !doneOpen;
    els.doneList.hidden = !doneOpen;
    els.doneChev.classList.toggle("open", doneOpen);
  });
  els.tagBack.addEventListener("click", closeOverlay);
  els.settingsBack.addEventListener("click", closeOverlay);
  els.archiveBack.addEventListener("click", closeOverlay);
  els.filterClear.addEventListener("click", () => setTagFilter(null));

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

function svg(viewBox, inner) {
  return `<svg viewBox="${viewBox}">${inner}</svg>`;
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

function itemEl(t) {
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

  // Tag chips → filter the list to that tag
  const tags = t.tags || [];
  if (tags.length) {
    const row = document.createElement("div");
    row.className = "item-tags";
    for (const tag of tags) {
      const chip = document.createElement("button");
      chip.type = "button";
      chip.className = "tag-pill";
      chip.textContent = "#" + tag;
      chip.classList.toggle("active", tag === activeTag);
      chip.title = tag === activeTag ? "Clear filter" : `Show only #${tag}`;
      chip.addEventListener("click", (e) => {
        e.stopPropagation();
        // Clicking the tag you are already filtered by clears it, so the same
        // gesture toggles rather than being a dead end.
        setTagFilter(tag === activeTag ? null : tag);
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

/** Tasks matching the active tag filter, or all of them when none is set. */
export function applyFilter(todos, tag) {
  if (!tag) return todos;
  return todos.filter((t) => (t.tags || []).includes(tag));
}

export function renderList(todos) {
  current = todos;
  const shown = applyFilter(todos, activeTag);
  const active = shown.filter((t) => !t.done).sort(comparatorFor(sortMode));
  const done = shown
    .filter((t) => t.done)
    .sort((a, b) => (b.createdAt || "").localeCompare(a.createdAt || ""));

  els.filterBar.hidden = !activeTag;
  if (activeTag) els.filterChip.textContent = "#" + activeTag;

  els.list.replaceChildren(...active.map((t) => itemEl(t)));

  // A filter matching nothing must not look like an empty database.
  const nothingShown = shown.length === 0;
  els.empty.hidden = !nothingShown;
  if (nothingShown) {
    els.empty.innerHTML = activeTag
      ? `Nothing tagged #${activeTag}.<br /><span>Clear the filter to see everything.</span>`
      : "Nothing here yet.<br /><span>Add your first task above.</span>";
  }

  if (done.length) {
    els.doneBlock.hidden = false;
    els.doneCount.textContent = String(done.length);
    els.doneList.replaceChildren(...done.map((t) => itemEl(t)));
    els.doneList.hidden = !doneOpen;
    els.doneChev.classList.toggle("open", doneOpen);
  } else {
    els.doneBlock.hidden = true;
  }

  if (overlay === "tags") renderTagPanel();
}

/* ---------- tag filter ---------- */

/**
 * Show only tasks carrying `tag`, or everything when `null`.
 *
 * Deliberately not persisted: sort mode is always visibly in effect, but a
 * filter left over from last session just looks like a list that lost its data.
 */
export function setTagFilter(tag) {
  activeTag = tag;
  if (overlay === "tags") closeOverlay();
  renderList(current);
}

/* ---------- overlays ---------- */

function setOverlay(name) {
  overlay = name;
  els.listView.hidden = name !== null;
  els.tagPanel.hidden = name !== "tags";
  els.settingsPanel.hidden = name !== "settings";
  els.archivePanel.hidden = name !== "archive";
}
export function closeOverlay() {
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

export function toggleArchive() {
  if (overlay === "archive") return closeOverlay();
  setOverlay("archive");
  handlers.onOpenArchive?.();
}

/* ---------- tag panel ---------- */

/**
 * A flat list of tags with counts; picking one filters the main list.
 *
 * The panel used to expand each tag into its tasks with a jump-and-flash into
 * the list. Filtering answers the same question better — in the real list, with
 * real cards — so the sublists are gone.
 */
function renderTagPanel() {
  const counts = new Map();
  for (const t of current) {
    for (const tag of t.tags || []) counts.set(tag, (counts.get(tag) || 0) + 1);
  }
  const tags = [...counts.keys()].sort(
    (a, b) => counts.get(b) - counts.get(a) || a.localeCompare(b)
  );

  els.tagEmpty.hidden = tags.length > 0;
  els.tagList.replaceChildren(
    ...tags.map((tag) => {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "tag-row" + (tag === activeTag ? " active" : "");
      row.innerHTML = '<span class="tname"></span><span class="tag-count"></span>';
      row.querySelector(".tname").textContent = "#" + tag;
      row.querySelector(".tag-count").textContent = String(counts.get(tag));
      row.title = `Show only #${tag}`;
      row.addEventListener("click", () => setTagFilter(tag));
      return row;
    })
  );
}
