# Neon-backed cross-machine sync

**Date:** 2026-08-03
**Status:** Approved

## Problem

The todo app stores its list in a single JSON file in the per-machine app data
directory. The list exists only on the machine that created it. Running the app
on a second machine (a Mac) means a second, unrelated list.

The goal is one list, shared by a Windows desktop and a Mac, backed by a Neon
Postgres database, without making the app useless when the network is down.

## Scope

One user, one shared list, two desktop machines. No authentication, no
multi-user access control, no mobile client, no real-time collaboration. Anyone
holding the connection string sees the same list — acceptable for a personal
tool, and the reason the credential never reaches the webview.

## Architecture

Rust owns the database and all sync. The webview never sees a credential and
never makes a network call.

```
src/store.js --invoke--> Tauri commands --> sync.rs --> todos.json
                                              |         (local cache; the UI reads only this)
                                              +-------> db.rs --sqlx/rustls--> Neon
```

Local-first. Every mutation writes `todos.json` and returns immediately, so the
UI never blocks on, or fails because of, the network. A background push mirrors
the change to Neon. A pull on launch and on window focus merges remote changes
back.

### Module layout

Today's `lib.rs` is 50 lines holding file I/O, a URL opener, and the Tauri
builder. Adding sync to it would produce a single unreadable file, so the Rust
side splits by responsibility:

| File | Responsibility | Depends on |
|---|---|---|
| `config.rs` | Read/write `config.json`; `TODO_DATABASE_URL` env override | filesystem |
| `db.rs` | sqlx pool, schema bootstrap, pull/push SQL | `config.rs` |
| `sync.rs` | Local file I/O, merge, dirty tracking, status | `db.rs` |
| `lib.rs` | Tauri command definitions and builder wiring | all of the above |

Each module is independently testable. `sync.rs`'s merge is a pure function over
two lists and needs no database to test.

## Data model

```sql
create table if not exists todos (
  id         text primary key,
  title      text not null,
  note       text not null default '',
  link       text,
  due        date,
  priority   text,
  done       boolean not null default false,
  tags       text[] not null default '{}',
  refs       text[] not null default '{}',
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz
);
create index if not exists todos_updated_at_idx on todos (updated_at);
```

`id` is `text`, not `uuid`. Existing ids are UUIDs, but `store.js`'s `newId()`
falls back to `"id-<base36>-<base36>"` when `crypto.randomUUID` is unavailable,
which is not a valid UUID. Typing the column as `uuid` would reject those rows.

`priority` is unconstrained text holding `low` / `med` / `high` / null, matching
what the frontend already writes.

The schema is created idempotently at app startup, so a fresh machine needs no
manual database setup.

## Sync

### Clock authority

Ordering uses Neon's clock, never a machine's. Push sets `updated_at = now()`
server-side; pull adopts that value into the local cache. Two laptops with
drifting clocks therefore cannot corrupt the merge. This is what makes
last-write-wins safe here — with client-supplied timestamps, a machine whose
clock is minutes fast would silently win every conflict.

The semantics are last-*push*-wins rather than last-*edit*-wins. For a single
user moving between machines these are the same thing in practice, and the rule
is predictable: whichever machine you touched last, wins.

### Push

```sql
insert into todos (...) values (...)
on conflict (id) do update set ..., updated_at = now();
```

An earlier draft guarded this with `where todos.updated_at <= excluded.updated_at`.
That was wrong: `excluded.updated_at` is the *client's* timestamp, so the guard
would have reintroduced exactly the clock-skew dependence the design set out to
eliminate — a machine running fast would win every conflict.

Safety comes from ordering instead. A sync always pulls and merges before it
pushes, and the merge lets an unpushed local edit win. So anything that reaches
the push is what this machine intends to be current, and overwriting is correct.

### Pull

```sql
select * from todos where updated_at > $last_sync;
```

### Merge

Each local row carries a `dirty` flag, set on mutation and cleared on successful
push. It is persisted, so edits made offline still push after a restart.

For each remote row:

- Local row absent -> take the remote row.
- Local row not dirty and remote `updated_at` is newer -> take the remote row.
- Local row dirty -> keep the local value; it pushes on the next sync.

### Deletes

Deleting sets `deleted_at` and keeps a local tombstone, so the delete propagates
rather than being resurrected by the other machine's copy. `load_todos` filters
tombstones out, so `render.js` and `addform.js` never see them. Tombstones older
than 30 days are purged locally and remotely.

### Refresh triggers

Launch, a 5-second poll, and window focus. The poll means a window left open on
one machine reflects the other machine's edits without being touched; focus
covers the case of returning to a machine that has been idle.

Overlapping syncs are skipped rather than queued — on a slow connection a sync
can outlast the interval, and stacking them would only pile up work.

Trade-off: a 5-second poll keeps Neon's compute from auto-suspending, so an open
window consumes compute hours continuously. The interval is a single constant
(`POLL_MS` in `src/sync.js`) so it is cheap to raise if usage becomes a concern.

Because most polls are no-ops, two things are throttled so the interval does not
turn idle time into constant work:

- The tombstone sweep is a write on both sides, so it runs at most hourly rather
  than on every poll.
- The cache file is rewritten only when the list actually changed. The watermark
  file is tiny and is written every time; losing it would only cause a wider
  re-pull, which the merge absorbs harmlessly.

## Command surface

`store.js` currently calls `save_todos` with the entire array on every mutation.
That is incompatible with row-level merge — a whole-array write cannot express
which row changed. Mutations become per-row:

| Before | After |
|---|---|
| `load_todos()` | `load_todos()` — live rows only, unchanged shape |
| `save_todos(entire array)` | `upsert_todo(todo)` / `delete_todo(id)` |
| — | `sync_now()` |
| — | `get_sync_status()` |
| — | `set_db_url(url)` |
| `open_link(url)` | unchanged |

The four mutators exported by `store.js` (`addTodo`, `updateTodo`, `toggleDone`,
`deleteTodo`) keep their exact signatures, so `main.js`, `render.js`, and
`addform.js` require no changes. The blast radius of this refactor is one file.

## Credentials

`config.json` in the app data dir holds `{"databaseUrl": "..."}`, written `0600`
on Unix. `TODO_DATABASE_URL` overrides it when set.

A password-type field in the existing Settings panel calls `set_db_url`, which
**validates by opening a connection before saving** — a bad string is rejected
with a visible error rather than silently breaking sync.

`get_sync_status()` returns a masked host and a state string. The full
credential is never returned to the frontend; there is no command that reads it
back.

## Settings UI

One new row beneath Theme in the existing settings panel: the connection-string
field plus a status line reading `Synced · 2m ago`, `Offline · 3 pending`, or
`Not configured`.

## macOS

`bundle.targets` becomes `["nsis", "dmg"]`. Tauri builds only the current
platform's targets, so a single config serves both machines.

The Mac needs Rust and the Xcode command line tools. The app is unsigned, so the
first launch requires right-click -> Open. `decorations: false` and
`transparent: true` behave correctly on macOS, and the window already draws its
own minimize and close buttons.

## Seeding and first connect

The schema is created and the 30 existing todos are pushed from the Windows
machine as part of implementation.

On any machine's first connect, the app copies `todos.json` to
`todos.local-backup-<timestamp>.json` and adopts the remote list wholesale. This
avoids duplicate-looking entries from a stale local file while destroying
nothing.

## Error handling

- **No connection string configured:** the app works as a purely local todo
  list. Status reads `Not configured`.
- **Network or database unreachable:** mutations still succeed locally and are
  marked dirty. Status reads `Offline · N pending`. The next successful sync
  flushes them.
- **Invalid credential:** rejected at `set_db_url` time. An
  already-saved-but-now-invalid credential surfaces as an error state in the
  status line rather than data loss.
- **Corrupt local `todos.json`:** treated as an empty list, matching today's
  behaviour; the next pull repopulates it from Neon.

## Testing

- Unit tests on the merge function, table-driven: remote-newer, local-dirty,
  tombstone-vs-edit, first-connect adoption, unknown-remote-row. Pure function,
  no database required.
- An integration test against Neon covering schema bootstrap and a push/pull
  round-trip.
- Manual UAT: edit while offline, reconnect, confirm propagation; delete on one
  machine and confirm the tombstone lands on the other.

## Security note

The connection string was shared in a chat transcript during design. It should
be rotated in the Neon console once sync is working, with the replacement pasted
into Settings.
