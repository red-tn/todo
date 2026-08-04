// archive.js — the panel listing completed tasks that have aged out of the list.
//
// Kept out of render.js, which already owns sorting, due tiers, cards, and the
// tag panel. Archive rendering shares none of that and only needs its own two
// elements and a restore callback.
const els = {};
let onRestore = () => {};

export function initArchivePanel(handlers) {
  onRestore = handlers.onRestore;
  els.list = document.getElementById("archive-list");
  els.empty = document.getElementById("archive-empty");
}

/** Group archived tasks by the month they were archived, newest first. */
function byMonth(todos) {
  const groups = new Map();
  for (const t of todos) {
    const when = new Date(t.archivedAt || t.createdAt);
    const key = `${when.getFullYear()}-${String(when.getMonth() + 1).padStart(2, "0")}`;
    const label = when.toLocaleDateString(undefined, { month: "long", year: "numeric" });
    if (!groups.has(key)) groups.set(key, { label, items: [] });
    groups.get(key).items.push(t);
  }
  return [...groups.entries()].sort((a, b) => b[0].localeCompare(a[0])).map(([, v]) => v);
}

export function renderArchive(todos) {
  els.empty.hidden = todos.length > 0;

  els.list.replaceChildren(
    ...byMonth(todos).flatMap(({ label, items }) => {
      const head = document.createElement("div");
      head.className = "archive-month";
      head.textContent = `${label} · ${items.length}`;

      const list = document.createElement("ul");
      list.className = "archive-items";
      for (const t of items) {
        const li = document.createElement("li");
        li.className = "archive-item";

        const title = document.createElement("span");
        title.className = "archive-title";
        title.textContent = t.title;

        const restore = document.createElement("button");
        restore.type = "button";
        restore.className = "archive-restore";
        restore.textContent = "Restore";
        restore.title = "Put this back in the list";
        restore.addEventListener("click", () => onRestore(t.id));

        li.append(title, restore);
        list.appendChild(li);
      }
      return [head, list];
    })
  );
}
