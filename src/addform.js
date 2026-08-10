// addform.js — the inline add/edit form (title, note, tags, priority, repeats, due).
import { addTodo, updateTodo, getTodos } from "./store.js";

let editingId = null;
let formTags = [];
let ddIndex = -1; // highlighted row in the tag dropdown, -1 = none
let formPriority = ""; // "" | "low" | "med" | "high"
let formRecur = ""; // "" | "daily" | "weekly" | "monthly" | "yearly"
let formRecurInterval = 1;
let commitTimer = null; // debounce handle for live edit-mode saves
const els = {};

export function initAddForm() {
  els.section = document.getElementById("add-section");
  els.toggle = document.getElementById("add-toggle");
  els.form = document.getElementById("add-form");
  els.title = document.getElementById("f-title");
  els.note = document.getElementById("f-note");
  els.tag = document.getElementById("f-tag");
  els.tagChips = document.getElementById("f-tags-chips");
  els.tagDd = document.getElementById("tag-dd");
  els.due = document.getElementById("f-due");
  els.prio = document.getElementById("f-prio");
  els.recur = document.getElementById("f-recur");
  els.recurEvery = document.getElementById("f-recur-every");
  els.recurInterval = document.getElementById("f-recur-interval");
  els.recurUnit = document.getElementById("f-recur-unit");
  els.prioBtns = Array.from(els.prio.querySelectorAll("button"));
  els.cancel = document.getElementById("f-cancel");
  els.save = document.getElementById("f-save");
  els.done = document.getElementById("f-done");

  els.toggle.addEventListener("click", openAdd);
  els.cancel.addEventListener("click", clearForm);
  els.done.addEventListener("click", clearForm);
  els.form.addEventListener("submit", onSubmit);
  els.form.addEventListener("keydown", (e) => {
    if (e.key === "Escape") clearForm();
  });

  // Edit mode auto-saves: text fields debounce, the date commits on change.
  els.title.addEventListener("input", scheduleCommit);
  els.note.addEventListener("input", scheduleCommit);
  els.due.addEventListener("change", commitEditNow);

  // Priority picker: clicking a button selects that level.
  for (const b of els.prioBtns) {
    b.addEventListener("click", () => {
      formPriority = b.dataset.prio;
      renderPriority();
      commitEditNow();
    });
  }

  // Repeats: the interval control only matters once a unit is chosen.
  els.recur.addEventListener("change", () => {
    formRecur = els.recur.value;
    renderRecur();
    commitEditNow();
  });
  els.recurInterval.addEventListener("change", () => {
    const n = parseInt(els.recurInterval.value, 10);
    formRecurInterval = Number.isFinite(n) && n > 0 ? Math.min(n, 99) : 1;
    els.recurInterval.value = String(formRecurInterval);
    commitEditNow();
  });

  // Start collapsed; prime the fields for when it opens.
  resetFields();

  // Tag entry: the dropdown lists existing tags to pick; Enter or comma
  // commits the typed token as a (possibly new) chip.
  els.tag.addEventListener("focus", openTagDd);
  els.tag.addEventListener("click", openTagDd);
  els.tag.addEventListener("blur", closeTagDd);
  els.tag.addEventListener("input", renderTagDd);
  // Keep focus on the input while clicking inside the dropdown, so picking
  // an item doesn't blur-close it mid-click.
  els.tagDd.addEventListener("mousedown", (e) => e.preventDefault());
  els.tag.addEventListener("keydown", (e) => {
    const open = !els.tagDd.hidden;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (!open) return openTagDd();
      const n = els.tagDd.children.length;
      if (!n) return;
      ddIndex = e.key === "ArrowDown" ? (ddIndex + 1) % n : (ddIndex - 1 + n) % n;
      renderDdHighlight();
    } else if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      const picked = open && ddIndex >= 0 ? els.tagDd.children[ddIndex] : null;
      if (picked) addTag(picked.dataset.tag);
      else commitTagInput();
    } else if (e.key === "Escape" && open) {
      e.stopPropagation(); // close only the dropdown, not the whole form
      closeTagDd();
    } else if (e.key === "Backspace" && els.tag.value === "" && formTags.length) {
      formTags.pop();
      renderTagChips();
      renderTagDd();
      commitEditNow();
    }
  });
}

/* ---------- tags ---------- */

function normalizeTag(raw) {
  return raw.replace(/^#+/, "").trim().toLowerCase().replace(/\s+/g, "-");
}

function allExistingTags() {
  const set = new Set();
  for (const t of getTodos()) for (const tag of t.tags || []) set.add(tag);
  return [...set].sort();
}

/** Add a normalized tag as a chip, clear the input, and refresh the dropdown. */
function addTag(tag) {
  if (tag && !formTags.includes(tag)) {
    formTags.push(tag);
    renderTagChips();
    commitEditNow();
  }
  els.tag.value = "";
  renderTagDd();
}

function commitTagInput() {
  addTag(normalizeTag(els.tag.value));
}

function renderTagChips() {
  els.tagChips.replaceChildren(
    ...formTags.map((tag) => {
      const chip = document.createElement("span");
      chip.className = "chip tag-chip";
      chip.innerHTML = `<span>#${tag}</span>`;
      const x = document.createElement("button");
      x.type = "button";
      x.className = "chip-x";
      x.textContent = "×";
      x.addEventListener("click", () => {
        formTags = formTags.filter((t) => t !== tag);
        renderTagChips();
        renderTagDd();
        commitEditNow();
      });
      chip.appendChild(x);
      return chip;
    })
  );
}

function openTagDd() {
  els.tagDd.hidden = false;
  renderTagDd();
}

function closeTagDd() {
  els.tagDd.hidden = true;
  ddIndex = -1;
}

/** Rebuild the dropdown rows: existing tags not yet chosen, filtered by input. */
function renderTagDd() {
  if (els.tagDd.hidden) return;
  const q = normalizeTag(els.tag.value);
  const avail = allExistingTags().filter((t) => !formTags.includes(t) && (!q || t.includes(q)));
  ddIndex = -1;
  els.tagDd.replaceChildren(
    ...avail.map((t) => {
      const row = document.createElement("button");
      row.type = "button";
      row.className = "tag-dd-item";
      row.dataset.tag = t;
      row.textContent = `#${t}`;
      row.addEventListener("click", () => addTag(t));
      return row;
    })
  );
}

function renderDdHighlight() {
  Array.from(els.tagDd.children).forEach((el, i) => el.classList.toggle("active", i === ddIndex));
  const cur = els.tagDd.children[ddIndex];
  if (cur) cur.scrollIntoView({ block: "nearest" });
}

/* ---------- priority ---------- */

function renderPriority() {
  for (const b of els.prioBtns) {
    b.classList.toggle("active", b.dataset.prio === formPriority);
  }
}

/** Show the interval control only when the task actually repeats. */
function renderRecur() {
  els.recur.value = formRecur;
  els.recurInterval.value = String(formRecurInterval);
  els.recurEvery.hidden = !formRecur;
  const plural = { daily: "days", weekly: "weeks", monthly: "months", yearly: "years" };
  els.recurUnit.textContent = plural[formRecur] || "";
}

/* ---------- live edit-mode saves ---------- */

/** Write the current form state back to the task being edited. No-op in add mode. */
function commitEdit() {
  if (!editingId) return;
  updateTodo(editingId, {
    title: els.title.value,
    note: els.note.value,
    due: els.due.value || null,
    tags: [...formTags],
    priority: formPriority || null,
    recurrence: formRecur || null,
    recurrenceInterval: formRecurInterval,
  });
}

/** Debounced save for fast-changing text fields. */
function scheduleCommit() {
  if (!editingId) return;
  clearTimeout(commitTimer);
  commitTimer = setTimeout(commitEdit, 300);
}

/** Immediate save for discrete actions (date, priority, chips). */
function commitEditNow() {
  if (!editingId) return;
  clearTimeout(commitTimer);
  commitTimer = null;
  commitEdit();
}

/** Persist any pending debounced edit — call before leaving edit mode. */
function flushCommit() {
  if (commitTimer) {
    clearTimeout(commitTimer);
    commitTimer = null;
    commitEdit();
  }
}

/** Swap the actions row between add (Cancel + Add) and edit (Done) modes. */
function setMode(mode) {
  const editing = mode === "edit";
  els.cancel.hidden = editing;
  els.save.hidden = editing;
  els.done.hidden = !editing;
}

/* ---------- collapse / expand ---------- */

function expand() {
  els.form.hidden = false;
  els.toggle.hidden = true;
}

function collapse() {
  els.form.hidden = true;
  els.toggle.hidden = false;
}

/* ---------- open / close / submit ---------- */

function openAdd() {
  editingId = null;
  resetFields();
  setMode("add");
  els.save.textContent = "Add";
  expand();
  els.title.focus();
}

export function openEdit(todo) {
  editingId = todo.id;
  resetFields();
  els.title.value = todo.title;
  els.note.value = todo.note || "";
  els.due.value = todo.due || "";
  formTags = [...(todo.tags || [])];
  formPriority = todo.priority || "";
  formRecur = todo.recurrence || "";
  formRecurInterval = todo.recurrenceInterval || 1;
  renderTagChips();
  renderPriority();
  renderRecur();
  setMode("edit"); // auto-saves from here; Done just collapses
  expand();
  els.title.focus();
  els.title.select();
}

function resetFields() {
  els.title.value = "";
  els.note.value = "";
  els.tag.value = "";
  els.due.value = "";
  formTags = [];
  formPriority = "";
  formRecur = "";
  formRecurInterval = 1;
  renderTagChips();
  renderPriority();
  renderRecur();
  closeTagDd();
}

/** Cancel/Escape/Done: persist any pending edit, then collapse back to the toggle. */
function clearForm() {
  flushCommit(); // persist last keystrokes before dropping editingId
  editingId = null;
  resetFields();
  setMode("add");
  els.save.textContent = "Add";
  els.title.blur();
  collapse();
}

function onSubmit(e) {
  e.preventDefault();
  commitTagInput(); // fold in any half-typed tag
  const title = els.title.value.trim();
  if (!title) {
    els.title.focus();
    return;
  }
  const data = {
    title,
    note: els.note.value,
    due: els.due.value || null,
    tags: [...formTags],
    priority: formPriority || null,
    recurrence: formRecur || null,
    recurrenceInterval: formRecurInterval,
  };
  if (editingId) {
    updateTodo(editingId, data);
    // Editing is a one-shot action — collapse back when done.
    clearForm();
  } else {
    addTodo(data);
    // Collapse back to the "New task" toggle after adding.
    clearForm();
  }
}
