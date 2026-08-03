# Todo: Auto-save edits, persistent trash, sort bar

**Date:** 2026-06-03
**Status:** Approved

## Goal

Three focused UX changes to the existing Tauri todo app:

1. Editing a task auto-saves as you type — no Save/Cancel buttons in edit mode.
2. Keep the per-task delete (trash) button exactly as it is.
3. Add a sort bar above the task list (Due date · Priority · Tag · Added) that replaces the automatic urgency sort and persists across sessions.

## 1. Auto-save in edit mode

Files: `src/addform.js`, `src/index.html`, `src/styles.css`

- **Add mode keeps an explicit commit.** Clicking "New task" shows the form with the **Add** button and **Cancel**. One tap commits the task. (Rationale: a new task has no id to save to yet; an explicit commit avoids empty-draft churn.)
- **Add collapses after committing.** After **Add**, the form collapses back to the slim "New task" toggle (`clearForm()`) rather than reopening blank. (Previously it stayed open for rapid entry.)
- **Edit mode auto-saves.** When `openEdit()` runs (`editingId` set), all fields write through live to `updateTodo(editingId, …)`:
  - `title`, `note`, `link`: `input` listeners, debounced ~300ms.
  - `due`: `change` → save immediately.
  - priority buttons: on click → save immediately.
  - tag chips add/remove, reference chips add/remove: on each mutation → save immediately.
- **No Save/Cancel in edit mode.** The `.add-actions` row is replaced by a single **Done** button that only collapses the form (data is already saved). Escape also collapses.
- The form keeps a single `editingId`-driven mode switch; the actions row swaps content based on add-vs-edit.
- `src/store.js` is untouched — `updateTodo` already trims fields and persists on each call.

### Edge cases
- Debounced text save must flush on close (Done/Escape) so a fast close doesn't drop the last keystrokes.
- Clearing the title to empty in edit mode: `updateTodo` trims to `""`. Allowed (task persists with empty title); not auto-deleted. Out of scope to block.

## 2. Trash bin stays

No change. `item-del` button in `src/render.js` `itemEl()` remains.

## 3. Sort bar

Files: `src/index.html`, `src/render.js`, `src/styles.css`

- A thin segmented bar at the top of `#list-view`, above `<ul class="list">`. Buttons: **Due date** (default) · **Priority** · **Tag** · **Added**.
- Replaces `byUrgency`. The selected mode is the single source of active-task order.
- Persisted in `localStorage` under key `todo.sortMode`; restored on load. Invalid/missing → `due`.
- Sort comparators (active tasks only):
  - `due`: ascending day-diff (soonest first), undated last; tiebreak `createdAt` asc.
  - `priority`: high → med → low → none; tiebreak day-diff then `createdAt`.
  - `tag`: first tag (alphabetical), untagged last; tiebreak `createdAt`.
  - `added`: `createdAt` descending (newest first).
- Per-card colour tiers (overdue/today/soon) from `tierFor()` are unchanged — styling, not order.
- The **Done** list keeps its existing "most recently completed first" order; the bar only affects active tasks.

## 4. Bug fix: `hidden` attribute defeated by `display` rules

File: `src/styles.css`

Symptom: the add form would not collapse — `Cancel`/`Add`/`Done` had no visual
effect, and the form showed even when collapsed.

Root cause (confirmed by instrumenting the running app — computed `display` was
`flex` while `hidden=true`): the JS correctly toggles the `hidden` attribute, but
`hidden` hides elements only via the UA rule `[hidden] { display: none }`. Author
rules of normal importance override UA rules in the cascade, so
`.add-form { display: flex }` and `.add-toggle { display: flex }` defeated it. The
Done list (`#done-list` carries `.list { display: flex }`) had the same latent bug.

Fix (single global rule, after the `*` reset):

```css
[hidden] { display: none !important; }
```

Makes the `hidden` attribute authoritative everywhere. Safe because every use of
`hidden` in this app means "remove the element"; no element is meant to be
`hidden` yet still shown. Fixes form collapse and the Done section toggle together.

## Out of scope
- No change to add-mode behavior, store schema, or Rust backend.
- No grouping/section headers — order only.
- No new sort options beyond the four listed.

## Notes
- Working directory is not a git repository, so the spec is not committed to git.
