# Archive, tag filter, and removing unused features

**Date:** 2026-08-04
**Status:** Approved

## Problem

Three observations from the live list of 30 tasks:

- **23 are complete and 7 are active**, and nothing has been cleared since 3
  June. The Done section is over three times the working list, and every sync
  and every launch carries all of it.
- **Seven tags are in active use** (`ai` on eight tasks, then `hr`, `strongdm`,
  `team-task`, and others), but there is no way to see only one tag's tasks.
  The tag panel jumps to a single task rather than filtering.
- **Links and references are used by zero of thirty tasks**, despite having a
  field in the add form, a picker, chips on cards, and a browser-opening
  command behind them.

Separately, an available update is only visible inside Settings, so it is found
only by going to look for it.

## Scope

Four changes, of which one is a deletion:

1. Archive completed tasks
2. Filter the list by tag
3. Remove links and references
4. Show an update indicator in the title bar

A phone-accessible web client was considered and deferred. It is the largest
gap but also the largest build, and it needs an authentication story before the
database is reachable from the internet.

## 1. Archive

### Schema

```sql
alter table todos add column if not exists archived_at timestamptz;
```

Migrated the same idempotent way as recurrence, so every machine self-applies
it on first launch of the new version.

**No "completed at" column is needed.** Completing a task sets `updated_at` and
nothing touches it afterwards, so "done and untouched for 30 days" is already
expressible as `done AND updated_at < now() - 30 days`.

### Behaviour

- `live_todos()` already excludes tombstones; it now also excludes archived
  tasks, so the main list simply never sees them.
- Auto-archiving runs inside the sync loop beside the tombstone sweep and on
  the same hourly throttle. At a 60-second poll it should not re-scan the list
  every minute.
- An **Archive all done** action in the Done header clears the existing backlog
  immediately rather than waiting out the 30-day threshold.
- An **Archive panel**, reached from a title-bar button and grouped by month,
  lists archived tasks with a restore action. Restoring clears `archived_at`
  and the task returns to the live list.

Archived rows stay in Neon and continue to sync, so archiving on one machine
archives on both.

## 2. Tag filter

Clicking a tag chip on a card filters the list to that tag. A `#ai ✕` bar above
the list shows the active filter and clears it. The Done section is filtered
too, so its count stays consistent with what is shown.

### The tag panel gets smaller

The panel currently expands each tag into a list of its tasks, with
click-to-jump-and-flash into the main list. That exists to answer "what is under
this tag", which filtering answers better — in the real list, with real cards.

So the panel becomes a flat list of tags with counts; clicking one applies the
filter and closes the panel. This removes the expandable sublists, the
`jumpTo` scroll-and-highlight machinery, and most of `renderTagPanel`.

### The filter does not persist

Sort mode persists because its effect is always visible. A filter does not: a
forgotten filter is a list that looks mysteriously empty on next launch. It
resets on restart.

## 3. Remove links and references

No task uses either, so **there is no data to lose**.

Removed: the link field and reference picker in the add form, the link chip and
reference chips on cards, the `link` and `refs` fields on the `Todo` struct and
in every query, the `open_link` command, and the `tauri-plugin-opener`
dependency that nothing else uses.

**The database columns stay.** Dropping them is destructive and gains nothing.

Old caches still load: serde ignores unknown JSON fields by default, so a
`todos.json` containing `link` and `refs` parses against the slimmer struct.

## 4. Title-bar update indicator

An accent dot on the settings gear whenever an update is waiting, with a
tooltip naming the version.

Badging the gear rather than adding an element keeps a 384px title bar that
already holds four buttons from getting crowded, and points at the action —
the Install button is in Settings. It reuses the existing status-dot styling so
it reads as part of the same visual language, and clears itself once the app is
current.

No new plumbing: `subscribeUpdate` already pushes status changes to listeners.

## Effect on the code

`render.js` is the largest file and currently owns sorting, due tiers, card
rendering, the tag panel, and jump-to-highlight. This work removes link
rendering, reference chips, `jumpTo`, and the tag panel's sublists while adding
a filter of a few lines, so it should come out smaller and more focused.

## Error handling

- **Migration fails:** surfaced as a sync error. The app keeps working from the
  local cache; archiving simply does not sync until it succeeds.
- **Restore fails:** the task stays archived and the error is shown. Nothing is
  lost either way, since archiving is reversible by definition.
- **Filter matches nothing:** the empty state says which tag is filtered and
  offers to clear it, rather than showing the generic "no tasks" message, which
  would look like data loss.

## Testing

- **Archive selection** — table-driven: done and old archives; done and recent
  does not; active and old does not; already-archived is left alone; a
  tombstone is never archived.
- **Restore** clears the flag and the task reappears in the live list.
- **Tag filter** — a pure filter over a fixture list, including a tag matching
  nothing and untagged tasks.
- **Backward compatibility** — a cache written with `link` and `refs` still
  parses against the slimmer struct.

The archive panel, the tag-filter interaction, and the title-bar dot are
verified by hand.
