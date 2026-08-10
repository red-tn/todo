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
{ "enabled": false, "webhookUrl": null, "threshold": "today", "time": "09:00" }
```

- `threshold`: `"today" | "tomorrow" | "week"` — cumulative horizons
  (today = due today; tomorrow = today + tomorrow; week = next 7 days).
- `webhookUrl` is a secret, treated exactly like `databaseUrl`: stored in
  `config.json`, never returned to the webview. The get-config command
  returns a `hasWebhook: bool` instead.

### Message building (`src-tauri/src/notify.rs`)

`build_slack_digest(todos, today, horizon_days)`: overdue tasks always
included and listed first, then live tasks due within the horizon
(0 / 1 / 6 extra days). Skips done, deleted, archived, and undated tasks
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
control, threshold segmented control (Today / Tomorrow / This week),
"Notify at" time input, webhook URL password-style input with Save (masked
placeholder saying a webhook is saved), and a Test button that reports
success or the error inline.

### Testing

`cargo test` covers horizon bucketing and message text. Live verification
via the Test button once the user's Slack workflow is published.
