// store.js — in-memory todo list, persisted per-row through Tauri commands.
//
// Writes go to the local cache in Rust and return immediately; mirroring them
// to Neon happens in the background, so the list stays usable offline.
const invoke = window.__TAURI__.core.invoke;

let todos = [];
const listeners = [];

/** Stable unique id, with a fallback if crypto.randomUUID is unavailable. */
function newId() {
  if (window.crypto && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return "id-" + Date.now().toString(36) + "-" + Math.random().toString(36).slice(2, 8);
}

/** Current todo list (live reference — read only). */
export function getTodos() {
  return todos;
}

/** Register a function to be called with the todo list whenever it changes. */
export function subscribe(fn) {
  listeners.push(fn);
}
function emit() {
  for (const fn of listeners) fn(todos);
}

/** Load todos from the local cache. Falls back to an empty list on any error. */
export async function load() {
  try {
    const rows = await invoke("load_todos");
    todos = Array.isArray(rows) ? rows : [];
  } catch (err) {
    console.error("load_todos failed:", err);
    todos = [];
  }
  emit();
}

/** Write one row through to the cache. Failures are logged, never surfaced as a broken UI. */
async function persist(todo) {
  try {
    await invoke("upsert_todo", { todo });
  } catch (err) {
    console.error("upsert_todo failed:", err);
  }
}

export function addTodo({ title, note, due, link, tags, refs, priority }) {
  const todo = {
    id: newId(),
    title: title.trim(),
    note: (note || "").trim(),
    link: (link || "").trim() || null,
    tags: Array.isArray(tags) ? tags : [],
    refs: Array.isArray(refs) ? refs : [],
    due: due || null,
    priority: priority || null,
    done: false,
    createdAt: new Date().toISOString(),
  };
  todos.push(todo);
  persist(todo);
  emit();
}

export function updateTodo(id, patch) {
  const t = todos.find((t) => t.id === id);
  if (!t) return;
  if (typeof patch.title === "string") patch.title = patch.title.trim();
  if (typeof patch.note === "string") patch.note = patch.note.trim();
  if (typeof patch.link === "string") patch.link = patch.link.trim() || null;
  Object.assign(t, patch);
  persist(t);
  emit();
}

export function toggleDone(id) {
  const t = todos.find((t) => t.id === id);
  if (!t) return;
  t.done = !t.done;
  persist(t);
  emit();
}

export function deleteTodo(id) {
  todos = todos.filter((t) => t.id !== id);
  emit();
  invoke("delete_todo", { id }).catch((err) => console.error("delete_todo failed:", err));
}
