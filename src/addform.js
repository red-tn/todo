// addform.js — the inline add/edit form (title, note, link, tags, references, due).
import { addTodo, updateTodo, getTodos } from "./store.js";

let editingId = null;
let formTags = [];
let formRefs = [];
let formPriority = ""; // "" | "low" | "med" | "high"
let commitTimer = null; // debounce handle for live edit-mode saves
const els = {};

export function initAddForm() {
  els.section = document.getElementById("add-section");
  els.toggle = document.getElementById("add-toggle");
  els.form = document.getElementById("add-form");
  els.title = document.getElementById("f-title");
  els.note = document.getElementById("f-note");
  els.link = document.getElementById("f-link");
  els.tag = document.getElementById("f-tag");
  els.tagChips = document.getElementById("f-tags-chips");
  els.tagSuggest = document.getElementById("tag-suggestions");
  els.ref = document.getElementById("f-ref");
  els.refChips = document.getElementById("f-refs-chips");
  els.due = document.getElementById("f-due");
  els.prio = document.getElementById("f-prio");
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
  els.link.addEventListener("input", scheduleCommit);
  els.due.addEventListener("change", commitEditNow);

  // Priority picker: clicking a button selects that level.
  for (const b of els.prioBtns) {
    b.addEventListener("click", () => {
      formPriority = b.dataset.prio;
      renderPriority();
      commitEditNow();
    });
  }

  // Start collapsed; prime the selects/suggestions for when it opens.
  resetFields();

  // Tag entry: Enter or comma commits the current token as a chip.
  els.tag.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      commitTagInput();
    } else if (e.key === "Backspace" && els.tag.value === "" && formTags.length) {
      formTags.pop();
      renderTagChips();
      commitEditNow();
    }
  });
  // Picking a datalist suggestion fires 'input'; commit it immediately.
  els.tag.addEventListener("input", () => {
    const opts = Array.from(els.tagSuggest.options).map((o) => o.value);
    if (opts.includes(els.tag.value)) commitTagInput();
  });

  els.ref.addEventListener("change", () => {
    if (els.ref.value) {
      addRef(els.ref.value);
      els.ref.value = "";
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

function commitTagInput() {
  const tag = normalizeTag(els.tag.value);
  els.tag.value = "";
  if (tag && !formTags.includes(tag)) {
    formTags.push(tag);
    renderTagChips();
    refreshTagSuggestions();
    commitEditNow();
  }
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
        refreshTagSuggestions();
        commitEditNow();
      });
      chip.appendChild(x);
      return chip;
    })
  );
}

function refreshTagSuggestions() {
  const avail = allExistingTags().filter((t) => !formTags.includes(t));
  els.tagSuggest.replaceChildren(
    ...avail.map((t) => {
      const o = document.createElement("option");
      o.value = t;
      return o;
    })
  );
}

/* ---------- references ---------- */

function addRef(id) {
  if (!formRefs.includes(id)) {
    formRefs.push(id);
    renderRefChips();
    refreshRefSelect();
    commitEditNow();
  }
}

function renderRefChips() {
  const byId = new Map(getTodos().map((t) => [t.id, t]));
  els.refChips.replaceChildren(
    ...formRefs
      .filter((id) => byId.has(id))
      .map((id) => {
        const chip = document.createElement("span");
        chip.className = "chip ref-chip";
        const label = document.createElement("span");
        label.textContent = "↳ " + byId.get(id).title;
        chip.appendChild(label);
        const x = document.createElement("button");
        x.type = "button";
        x.className = "chip-x";
        x.textContent = "×";
        x.addEventListener("click", () => {
          formRefs = formRefs.filter((r) => r !== id);
          renderRefChips();
          refreshRefSelect();
          commitEditNow();
        });
        chip.appendChild(x);
        return chip;
      })
  );
}

function refreshRefSelect() {
  const options = [el("option", "↳ Reference a task…", "")];
  for (const t of getTodos()) {
    if (t.id === editingId) continue; // can't reference itself
    if (formRefs.includes(t.id)) continue; // already referenced
    if (t.done) continue; // keep the picker to active tasks
    options.push(el("option", t.title, t.id));
  }
  els.ref.replaceChildren(...options);
  els.ref.value = "";
}

function el(tag, text, value) {
  const o = document.createElement(tag);
  o.textContent = text;
  if (value !== undefined) o.value = value;
  return o;
}

/* ---------- priority ---------- */

function renderPriority() {
  for (const b of els.prioBtns) {
    b.classList.toggle("active", b.dataset.prio === formPriority);
  }
}

/* ---------- live edit-mode saves ---------- */

/** Write the current form state back to the task being edited. No-op in add mode. */
function commitEdit() {
  if (!editingId) return;
  updateTodo(editingId, {
    title: els.title.value,
    note: els.note.value,
    link: els.link.value,
    due: els.due.value || null,
    tags: [...formTags],
    refs: [...formRefs],
    priority: formPriority || null,
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
  els.link.value = todo.link || "";
  els.due.value = todo.due || "";
  formTags = [...(todo.tags || [])];
  formRefs = [...(todo.refs || [])];
  formPriority = todo.priority || "";
  renderTagChips();
  renderRefChips();
  renderPriority();
  refreshTagSuggestions();
  refreshRefSelect();
  setMode("edit"); // auto-saves from here; Done just collapses
  expand();
  els.title.focus();
  els.title.select();
}

function resetFields() {
  els.title.value = "";
  els.note.value = "";
  els.link.value = "";
  els.tag.value = "";
  els.due.value = "";
  formTags = [];
  formRefs = [];
  formPriority = "";
  renderTagChips();
  renderRefChips();
  renderPriority();
  refreshTagSuggestions();
  refreshRefSelect();
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
    link: els.link.value,
    due: els.due.value || null,
    tags: [...formTags],
    refs: [...formRefs],
    priority: formPriority || null,
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
