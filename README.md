# Todo

A small always-there todo widget, built with Tauri (Rust) and vanilla HTML/CSS/JS.
The list is stored locally and synced between machines through a Neon Postgres
database.

## How sync works

The app is local-first. Every edit writes to a local cache and returns
immediately, so the list keeps working with no network at all. Changed rows are
flagged and pushed to Neon on the next successful sync; remote changes are
pulled on launch, every 5 seconds, and whenever the window regains focus.

> **Note on Neon usage:** polling every 5 seconds keeps the Neon compute from
> auto-suspending, so an open window consumes compute hours continuously. If you
> are on the free tier, either keep an eye on the usage dashboard or raise
> `POLL_MS` in `src/sync.js`.

Conflicts resolve by last write, with two rules that keep it predictable:

- Ordering uses the **database's** clock, never a laptop's, so a machine with a
  wrong clock cannot win conflicts it shouldn't.
- A local edit that hasn't been pushed yet always beats an incoming remote row,
  and then becomes the remote value on the push that follows.

Deletes travel as tombstones rather than missing rows, so deleting on one
machine doesn't get undone by the other machine's copy. Tombstones are cleaned
up after 30 days.

## Setup on a new machine

### 1. Prerequisites

- [Rust](https://rustup.rs/)
- **macOS:** Xcode command line tools — `xcode-select --install`
- **Windows:** the MSVC build tools and WebView2 (both come with a normal Rust +
  Visual Studio Build Tools install)
- Node, only for the Tauri CLI — `npm install`

### 2. Build

```sh
npm install
npm run tauri build
```

Tauri only builds the current platform's targets: an NSIS installer on Windows,
a `.dmg` and `.app` on macOS. Output lands in
`src-tauri/target/release/bundle/`.

For development, `npm run tauri dev` runs it without bundling.

### 3. First launch on macOS

The app is not code-signed, so Gatekeeper will refuse a normal double-click.
Right-click the app and choose **Open**, then confirm. This is only needed once.

### 4. Connect it to the database

Open **Settings** (the gear in the title bar), paste the Neon connection string
into **Cloud sync**, and press **Save**. The string is validated by actually
connecting before it's saved, so a typo tells you immediately.

On its first successful connect, a machine adopts the shared list from the
database. Whatever was in its local file beforehand is copied to
`todos.local-backup-<timestamp>.json` in the app data directory first — nothing
is thrown away.

## Where things live

| What | Windows | macOS |
|---|---|---|
| App data directory | `%APPDATA%\com.ryan.todowidget\` | `~/Library/Application Support/com.ryan.todowidget/` |
| Local cache | `todos.json` | same |
| Sync watermark | `sync.json` | same |
| Connection string | `config.json` | same (`0600`) |

`TODO_DATABASE_URL` overrides `config.json` when set, which is handy for
pointing a machine at a different database without changing the saved setting.

The connection string is never sent to the webview. The UI can write one in and
read back a masked host, but no command returns the credential.

## Project layout

```
src/                    frontend (no build step — Tauri serves it directly)
  store.js              in-memory list + per-row persistence
  sync.js               sync status, the connection setting, refresh triggers
  render.js             sorting, due tiers, cards, tag panel
  addform.js            the add/edit form
  theme.js              light/dark/system

src-tauri/src/
  lib.rs                Tauri commands and app wiring
  model.rs              the Todo record
  config.rs             the connection string
  db.rs                 Neon queries
  sync.rs               local cache and the merge
```

## Tests

```sh
cd src-tauri
cargo test                                    # merge logic, URL masking, legacy file parsing
TODO_TEST_DATABASE_URL=<url> cargo test -- --ignored   # round-trip against a real database
```

The integration tests only touch rows whose id starts with `zz-test-`, so they
are safe to run against the live database.

## Recommended IDE setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
