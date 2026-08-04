# Tray, daily digest notifications, and recurring tasks

**Date:** 2026-08-04
**Status:** Approved

## Problem

The app is a widget that only exists while its window is open. Three
consequences:

- Closing it stops sync, so the two machines drift until it is reopened.
- It cannot remind anyone of anything. The live list currently holds tasks 24
  and 10 days overdue.
- Anything that repeats — monthly expenses, a recurring review — has to be
  retyped from scratch each time, and dies the first time it is forgotten.

## Scope

Three features, built and committed in order so the work can be stopped or
redirected after any one of them:

1. Tray icon and start on login
2. Daily digest notifications
3. Recurring tasks

The order is a dependency, not a preference. Notifications are pointless if the
app is not running, and keeping it running is what the tray provides.

## 1. Tray and start on login

Tauri's built-in `tray-icon` feature plus `tauri-plugin-autostart`.

- Tray menu: **Show Todo**, **Sync now**, **Start on login** (checkbox),
  **Quit**.
- Left-clicking the tray icon toggles the window.
- The window's close button intercepts `CloseRequested`, prevents the default,
  and hides the window. **Quit in the tray menu becomes the only true exit.**
- Settings gets a matching "Start on login" toggle, so the setting is
  discoverable without knowing the tray menu exists.

Two macOS specifics: the tray icon must be registered with
`icon_as_template(true)` or it renders as a colored blob rather than adapting to
the menu bar, and autostart normally requires the app to be in `/Applications`.

### Why hide rather than quit

Notifications and background sync both require a running process. A close button
that quits would silently disable the two features built on top of it. The tray
menu keeps a real exit available.

## 2. Daily digest notifications

A new `notify.rs` module and `tauri-plugin-notification`.

The `due` column is a date with no time component, so there is no per-task
moment to fire at. A single daily digest fits the data and is far harder to
start ignoring than a stream of individual alerts.

- A 60-second timer checks whether the digest time has passed and whether today's
  digest has already fired.
- The notification is one line: `3 due today · 2 overdue`, plus the first few
  titles. Clicking it shows the window.
- If the app was not running at the digest time, the digest fires when it next
  starts, provided it is still the same day. Late is better than missed.

### Settings are per-machine

Enabled state and time live in the local `config.json` beside the connection
string, not in Neon. A desktop that nags and a laptop that stays quiet is a
reasonable setup, and syncing the preference would make it impossible.

Default: enabled, 08:00.

### Duplicate digests are accepted

With both machines running, the digest fires on both.

The alternative is coordinating through a `notified_at` column in Neon, where
the first machine to fire claims the day. That was rejected: if the digest fires
on a sleeping Mac at 08:00, the notification is never seen anywhere. A duplicate
notification to an empty room is harmless; a missed one is the failure the
feature exists to prevent.

## 3. Recurring tasks

### Migration

Two new columns:

```sql
alter table todos add column if not exists recurrence text;
alter table todos add column if not exists recurrence_interval integer not null default 1;
```

`ensure_schema` currently runs only `create table if not exists`, which does
nothing to an existing table. These statements run beside it on every launch.
They are idempotent, so every machine self-migrates the first time it starts the
new version, with no manual step.

`recurrence` is `daily` / `weekly` / `monthly` / `yearly` / null.
`recurrence_interval` combines with it to give "every 2 weeks" or "every 3
months".

The matching fields on the local `Todo` struct carry serde defaults, so caches
written by the previous version still parse.

### Spawning

Completing a recurring task creates a **new** task with a new id and the due
date advanced by `interval × unit`. The completed task stays in Done, preserving
history.

The spawn happens in Rust, inside `upsert_todo`, when `done` transitions false →
true and `recurrence` is set. Putting it there rather than in the UI means it
behaves identically regardless of which machine completes the task, and a sync
that merely delivers an already-completed row cannot trigger a second spawn,
because no transition occurs locally.

A task with no due date uses today as the base.

### Date arithmetic

`Jan 31 + 1 month` has no naively correct answer. The target day is clamped to
the last valid day of the destination month, giving Feb 28 (or Feb 29 in a leap
year) — the behaviour calendar applications use. This is the most likely place
for a subtle bug and is covered by table-driven tests.

### UI

A "Repeats" row in the add/edit form: a unit dropdown and an interval number.
Recurring cards carry a `↻ every 2 weeks` chip.

## Error handling

- **Notification permission denied (macOS):** the digest is skipped silently and
  the Settings row reports that permission is needed. Not an error state.
- **Autostart registration fails:** the toggle reverts and shows why, rather
  than claiming a setting that did not take.
- **Migration fails:** surfaced as a sync error. The app keeps working against
  the local cache; recurrence simply does not sync until it succeeds.
- **Spawn fails:** the completion still succeeds. A missing successor is
  recoverable by hand; a lost completion is not.

## Testing

- **Recurrence date math** — table-driven: month-end clamping, leap years,
  interval multiples, each unit. The highest-risk logic here.
- **Spawn behaviour** — completing a recurring task yields exactly one successor
  with the correct fields; completing a non-recurring task yields none; a task
  already done does not re-spawn.
- **Digest selection** — due-today and overdue sets from a fixture list, plus
  same-day suppression.
- **Backward compatibility** — a cache written before these fields existed still
  parses.

Tray behaviour, notification delivery, and autostart are verified by hand; a
menu bar and an OS notification centre cannot be meaningfully automated here.
