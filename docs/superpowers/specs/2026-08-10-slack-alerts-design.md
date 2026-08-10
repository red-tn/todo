# Slack alerts and top-bar home link — design

Date: 2026-08-10
Status: approved

## A. Top-bar title as home link

Clicking the "To Do" title in the top bar closes whatever overlay panel is
open (tags, archive, settings) and returns to the list view. The title span
loses `data-tauri-drag-region` (drag regions trigger on the exact element
clicked, so the rest of the bar stays draggable) and gains a pointer cursor
and accent hover. The active tag filter is left alone — it has its own clear
button. `render.js` exports its existing `closeOverlay()` for `main.js` to
wire up.

## B. Slack alerts

A daily Slack message listing overdue and upcoming tasks, sent from the
running app to a Slack webhook / workflow-trigger URL on its own schedule,
configured entirely in Settings.

### Config (`src-tauri/src/config.rs`)

New `slack` block alongside `digest`, per-machine like the digest:

```json
{ "enabled": false, "webhookUrl": null, "thresholds": ["today"], "times": ["09:00"] }
```

- `thresholds`: any combination of `"today" | "tomorrow" | "week"` —
  independent buckets (today = due today; tomorrow = due tomorrow; week =
  due 2–7 days out). Empty means overdue tasks only. *(Amended from a single
  cumulative threshold at the user's request: any-or-all selection.)*
- `times`: one or two "HH:MM" local times; the digest posts at each.
  *(Amended from a single time.)* If the app was closed while both times
  passed, the catch-up on launch sends one post, not two.
- `webhookUrl` is a secret, treated exactly like `databaseUrl`: stored in
  `config.json`, never returned to the webview. The get-config command
  returns a `hasWebhook: bool` instead.

### Message building (`src-tauri/src/notify.rs`)

`build_slack_message(todos, today, buckets)`: overdue tasks always
included and listed first, then live tasks whose due date falls in a
selected bucket. Skips done, deleted, archived, and undated tasks
like the desktop digest. Produces a single message string: a headline
("2 overdue · 3 due soon") followed by one line per task with its due date.
Unit-tested alongside the existing digest tests.

The POST body is `{"text": "<message>"}` — works with classic incoming
webhooks and with Workflow Builder triggers whose workflow defines a text
variable named `text`.

### Scheduling and sending (`src-tauri/src/lib.rs`)

- `spawn_slack_loop`, same shape as `spawn_digest_loop`: minute tick,
  reuses `should_fire`, so it inherits fire-late-if-app-was-closed and
  fire-once-per-day behavior. Sends nothing when the message is empty.
- `send_slack` posts with `reqwest` (rustls, matching sqlx's TLS stack),
  10 s timeout. Slack's response body is included in errors so the Test
  button can show e.g. `workflow_not_published`.
- Commands: `get_slack_config` (enabled, threshold, time, hasWebhook),
  `set_slack_config` (empty URL input keeps the saved one; the literal
  string `"clear"` is not special — clearing happens by disabling),
  `test_slack` (sends the current digest now, returns the message headline
  or the HTTP error).

### Settings UI (`src/index.html`, `src/main.js`)

A "Slack alerts" block matching the existing blocks: on/off segmented
control, multi-select bucket toggles (Today / Tomorrow / This week — each
toggles independently), two "Notify at" time inputs (second optional,
cleared to remove), webhook URL password-style input with Save (masked
placeholder saying a webhook is saved), and a Test button that reports
success or the error inline.

### Settings sync and single-sender dedup (amendment, same day)

Digest and Slack settings now sync between machines through the database,
mirroring the todo sync design:

- A `settings` table (key → JSON value, server-stamped `updated_at`) holds
  rows for `digest` and `slack`. Local `config.json` stays the offline copy,
  with per-key `settingsMeta` (`updatedAt`, `dirty`) for last-write-wins
  merging on server time. A dirty local value wins and is pushed; a missing
  remote key is pushed to bootstrap; otherwise the newer stamp wins.
- Exception in the merge: a remote slack value with no webhook URL never
  erases a locally saved URL — the merged value keeps the URL and is marked
  dirty so it propagates instead.
- The database credential itself stays local only.
- Settings edits schedule an immediate background sync; the settings panel
  re-fetches when opened so synced values appear.
- **Only one machine sends** even when both are online: a `slack_fired`
  table keyed on (date, "HH:MM" slot) is claimed with an
  insert-on-conflict-do-nothing before posting; only the machine whose
  insert lands sends. Claims older than a week are purged. With no database
  configured, or the claim unreachable, delivery is favored over dedup.
- Desktop digest notifications still fire on each running machine — they
  are per-device by nature; it is the settings that sync.

### Testing

`cargo test` covers bucket semantics, message text, and the URL-preserving
settings merge. Integration tests (`cargo test -- --ignored
--test-threads=1` with `TODO_TEST_DATABASE_URL`) cover the settings
round-trip and the slot claim race. Live verification via the Test button
once the user's Slack workflow is published.
