# Todo

A small always-there todo widget, built with Tauri (Rust) and vanilla HTML/CSS/JS.
The list is stored locally and synced between machines through a Neon Postgres
database.

## How sync works

The app is local-first. Every edit writes to a local cache and returns
immediately, so the list keeps working with no network at all. Changed rows are
flagged and pushed to Neon on the next successful sync; remote changes are
pulled on launch, every 60 seconds, and whenever the window regains focus.

> **Note on Neon usage:** any poll more frequent than Neon's auto-suspend window
> (5 minutes by default) keeps the compute awake, so an open window consumes
> compute hours continuously. The 60-second interval cuts query volume but does
> **not** change that — only an interval longer than the auto-suspend threshold
> would. To actually reduce compute hours, either raise `POLL_MS` in
> `src/sync.js` above 5 minutes or set it to `0` to disable polling and rely on
> launch and window focus alone.

Conflicts resolve by last write, with two rules that keep it predictable:

- Ordering uses the **database's** clock, never a laptop's, so a machine with a
  wrong clock cannot win conflicts it shouldn't.
- A local edit that hasn't been pushed yet always beats an incoming remote row,
  and then becomes the remote value on the push that follows.

Deletes travel as tombstones rather than missing rows, so deleting on one
machine doesn't get undone by the other machine's copy. Tombstones are cleaned
up after 30 days.

## Archive

Completed tasks leave the working list 30 days after they were finished, moving
to an **Archive** panel in the title bar where they stay searchable and can be
restored. **Archive all** in the Done header clears the current backlog without
waiting.

Archived tasks stay in the database and keep syncing, so archiving on one
machine archives on both. Nothing is deleted — restoring puts a task straight
back in the list.

## Filtering by tag

Click a tag on any task to show only that tag; click it again, or the × in the
filter bar, to clear. The Tags panel lists every tag with a count, and picking
one applies the same filter.

The filter is session-only and resets on restart. Sort mode persists because its
effect is always visible; a filter left over from last session just looks like a
list that lost its data.

## Recurring tasks

Set **Repeats** on a task — daily, weekly, monthly, or yearly, with an interval
so "every 2 weeks" and "every 3 months" work.

Completing a repeating task does not reopen it. The finished one stays in Done,
preserving history, and a new task is created with the due date moved on. If you
complete something long after it was due, the successor lands on the next
occurrence still in the future rather than one that is already overdue.

Month steps clamp to the end of the month, so 31 January plus one month is 28
February (29 in a leap year) rather than sliding into March.

## Daily digest

One notification a day listing what is due today and what is overdue. Since
tasks carry a due date and no time, there is no per-task moment to fire at, and
a single daily summary is much harder to start ignoring than a stream of alerts.

Settings are **per-machine, not synced** — a desktop that nags and a laptop that
stays quiet is a reasonable setup. Enable it and set the time in Settings;
default is 08:00. The Preview button sends one immediately, which is the only
way to confirm notifications reach your desktop without waiting until morning.

If the app was not running at the digest time, the digest fires the next time it
starts that day. Late beats missed.

With both machines running you will get the digest twice. That is deliberate:
coordinating through the database would mean a digest fired on a sleeping laptop
is never seen anywhere.

## Tray and start on login

The app lives in the system tray (menu bar on macOS). Closing the window hides
it rather than quitting, so background sync and the daily digest keep working —
**Quit in the tray menu is the only real exit**.

The tray menu holds Show Todo, Sync now, Start on login, and Quit. Start on
login is also in Settings, and launching that way starts the app hidden rather
than throwing a window up on every boot.

## Updates

The app checks GitHub for a newer release on launch and offers it in Settings.
It never installs or restarts on its own — a surprise restart would discard
whatever you were mid-way through typing — so installing is always an explicit
button press.

Updates are signed. The app refuses anything that does not verify against the
public key baked into `tauri.conf.json`, so a spoofed endpoint cannot push code
to your machines.

### Cutting a release

```sh
npm run release -- 0.2.0
git push && git push origin v0.2.0
```

`npm run release` bumps the version in `package.json`, `package-lock.json`,
`tauri.conf.json`, and `Cargo.toml` together, then commits and tags. They must
agree: if the tag and `tauri.conf.json` disagree, the updater ignores the
release without complaining.

Pushing the tag triggers `.github/workflows/release.yml`, which builds Windows
and a universal macOS binary, signs everything, and publishes a GitHub Release
containing `latest.json` — the file installed apps poll.

The release stays a **draft** until every platform has uploaded and a verify job
has checked the manifest, then it is promoted. Publishing per-platform meant the
manifest briefly advertised a version the slower platform had not built yet, and
clients there errored.

### Signing keys

Generated once with `npm run tauri signer generate`. The **public** key lives in
`tauri.conf.json`. The **private** key must exist in two places:

- the `TAURI_SIGNING_PRIVATE_KEY` secret in the repository's Actions settings
- a password manager

**If the private key is lost, installed apps can never be updated again** —
they only accept updates signed by the matching key, so every machine would need
a manual reinstall. `*.key` is gitignored; keep it that way.

### Development

`npm run tauri dev` runs the app without bundling and reloads on frontend
changes. Note that it runs a debug build, which is slower to start.

## Installation

**Download a build from [Releases](https://github.com/red-tn/todo/releases/latest)** —
CI publishes a Windows installer and a universal macOS disk image for every
version. No toolchain required, and updates after that are in-app.

| Platform | File |
|---|---|
| Windows | `Todo_<version>_x64-setup.exe` |
| macOS (Apple Silicon and Intel) | `Todo_<version>_universal.dmg` |

On macOS the app is unsigned, so the first launch needs **right-click → Open**
rather than a double-click. See [macOS](#macos) below.

Everything after this point is for building from source, which you only need if
you are changing the code.

---

## Building from source

> **There is no cross-compilation.** Tauri produces a native binary linked
> against each platform's SDK, so the Windows installer must be built on Windows
> and the macOS app on a Mac. That is why release builds run on CI, which has
> both.

Both platforms share the same first two steps.

### Common prerequisites

- [Rust](https://rustup.rs/) — `rustup` installs everything needed
- [Node.js](https://nodejs.org/) — used only to run the Tauri CLI

```sh
git clone https://github.com/red-tn/todo.git
cd todo
npm install
```

---

### Windows

**Prerequisites**

- **Microsoft C++ Build Tools** — install the "Desktop development with C++"
  workload from [the Visual Studio Build Tools
  installer](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
- **WebView2** — already present on Windows 10 1803+ and Windows 11. On older
  builds, install the [Evergreen
  Runtime](https://developer.microsoft.com/microsoft-edge/webview2/)

**Build**

```sh
npm run tauri build
```

**Install**

The installer lands at:

```
src-tauri/target/release/bundle/nsis/Todo_<version>_x64-setup.exe
```

Run it. It installs per-user (no admin prompt) and adds *Todo* to the Start
menu. The standalone binary is also at
`src-tauri/target/release/Todo.exe` if you would rather not install.

---

### macOS

**Prerequisites**

- **Xcode command line tools** — `xcode-select --install`

**Build**

```sh
npm run tauri build
```

On Apple Silicon this produces an arm64 build; on Intel, x86_64. For one bundle
that runs on both:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run tauri build -- --target universal-apple-darwin
```

**Install**

The disk image lands at:

```
src-tauri/target/release/bundle/dmg/Todo_<version>_universal.dmg
```

Open it and drag **Todo** to Applications. The raw `.app` is also at
`src-tauri/target/release/bundle/macos/Todo.app`.

**First launch — this step is required.** The app is not code-signed or
notarized, so double-clicking it gives *"Todo" cannot be opened because the
developer cannot be verified*. To get past it, **right-click the app and choose
Open**, then click **Open** in the dialog. macOS remembers the exception, so
this is a one-time step.

If macOS refuses even after that (Sequoia and later are stricter), clear the
quarantine flag:

```sh
xattr -dr com.apple.quarantine /Applications/Todo.app
```

---

### Connect it to the database

Same on both platforms. Open **Settings** (the gear in the title bar), paste the
Neon connection string into **Cloud sync**, and press **Save**. The string is
validated by actually connecting before it is saved, so a typo tells you
immediately.

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
  update.js             update checks and the install-with-consent flow
  archive.js            the archive panel
  render.js             sorting, due tiers, cards, tag panel
  addform.js            the add/edit form
  theme.js              light/dark/system

src-tauri/src/
  lib.rs                Tauri commands and app wiring
  model.rs              the Todo record
  config.rs             connection string and per-machine settings
  db.rs                 Neon queries and schema migrations
  sync.rs               local cache and the merge
  tray.rs               tray icon and its menu
  notify.rs             the daily digest
  recur.rs              recurrence date maths and spawning
  update.rs             checking for and installing releases
```

## Tests

```sh
cd src-tauri
cargo test                                    # merge logic, recurrence maths, digest, URL masking
TODO_TEST_DATABASE_URL=<url> cargo test -- --ignored   # round-trip against a real database
```

> **Point the integration tests at a scratch database, not your real one.**
> They only touch rows prefixed `zz-` and clean up after themselves, but "clean
> up" means a hard `delete`, and hard deletes do not propagate to clients —
> only tombstones do. If the app is running against the same database while the
> tests run, it will pull a test fixture into its local cache and then never
> learn the row was removed. The fixture sits in your list until you delete it
> by hand. Create a second Neon branch or database for testing.

## Recommended IDE setup

- [VS Code](https://code.visualstudio.com/) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer)
