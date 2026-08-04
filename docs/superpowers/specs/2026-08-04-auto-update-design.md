# GitHub-based auto-update

**Date:** 2026-08-04
**Status:** Approved

## Problem

The app is installed on two machines and has no way to update itself. Every
change means rebuilding from source on each machine — which on macOS also means
having Rust and the Xcode command line tools installed. That is friction enough
that the two machines will drift apart.

## Scope

Auto-update and the release pipeline that feeds it. The three features chosen
alongside this — due-date notifications, recurring tasks, and tray icon with
start-on-login — are separate work with their own designs.

## Prerequisite: the repository is public

Tauri's updater fetches a manifest over plain HTTPS. GitHub release assets on a
private repository require authentication, and asset downloads redirect to
signed S3 URLs, which makes passing an auth header through unreliable. The
alternatives were embedding a GitHub token in the shipped binary (a credential
in a distributed artifact) or maintaining a second public repository for
releases.

The repository was made public instead. This also makes GitHub Actions free and
unlimited, including macOS runners, so CI can build the `.dmg` — removing the
need for a Rust toolchain on the Mac.

Before going public the repository was checked for secrets across its full
history, and two internal project names embedded in a test fixture were replaced
with generic values via a history rewrite.

## Architecture

```
git tag v0.2.0 && git push --tags
   |
   +-> GitHub Actions (windows-latest, macos-latest x2 arches)
   |      build -> sign with private key -> publish Release + latest.json
   |
   +-> installed app checks the endpoint on launch
          -> "Update available: v0.2.0  [Install and restart]"
```

Endpoint: `https://github.com/red-tn/todo/releases/latest/download/latest.json`

### Components

| Piece | Responsibility |
|---|---|
| Signing keypair | Public key in `tauri.conf.json`; private key in CI secrets and a password manager |
| `tauri-plugin-updater` | Fetch the manifest, verify the signature, download and install |
| `tauri-plugin-process` | Restart the app after install |
| `.github/workflows/release.yml` | Build, sign, and publish on a version tag |
| `src/update.js` | Check state, drive the UI, request install |
| Settings panel row | Surfaces state and carries the consent button |

### Signing

Updates are signed; the app refuses anything that does not verify against the
embedded public key. This is what stops a compromised or spoofed endpoint from
installing arbitrary code.

**Losing the private key is unrecoverable.** Installed apps only accept updates
signed by the matching key, so a lost key means every machine must be
reinstalled by hand. It is stored in two places: a GitHub Actions secret and a
password manager.

The key is generated with an empty passphrase. The passphrase would only protect
the key at rest inside an already-secret CI variable, and the added moving part
is not worth it for a personal tool. The key file itself is gitignored.

## Update UX

The app checks on launch and never installs without consent.

An unannounced restart would discard whatever the user was typing, so the flow
is deliberately explicit:

1. On launch, check the endpoint. Failures are silent — no network is a normal
   state for this app and must not produce an error dialog.
2. If an update exists, the Settings panel shows `Update available: v0.2.0`
   with an **Install and restart** button. Nothing else changes; the list stays
   usable.
3. Pressing the button downloads, installs, and restarts, showing progress
   while it downloads.

A **Check for updates** button covers checking on demand.

## Versioning

The version appears in `tauri.conf.json`, `package.json`, and
`src-tauri/Cargo.toml`, and the git tag must match. Updating four things by hand
drifts, so `npm run release -- 0.2.0` bumps all of them and creates the tag.

## Error handling

- **Offline or endpoint unreachable:** silent. The status line keeps showing the
  current version.
- **Signature mismatch:** the update is refused and an error is shown. This is a
  security stop, not a transient failure, so it must be visible rather than
  silent.
- **Download or install failure:** an error in the Settings row; the app keeps
  running on the current version.
- **No release yet:** the endpoint 404s, which is treated as "no update", not an
  error.

## macOS risk

The app is unsigned and not notarized. The updater replaces the `.app` bundle
in place, and Gatekeeper may re-apply quarantine to the replacement and refuse
to launch it. This cannot be verified from Windows.

Plan: test with a throwaway release on the Mac. If quarantine does re-apply, the
fallback is for macOS to show a notification linking to the release page instead
of installing in place — the check and the UI stay the same, only the install
step changes.

## Testing

- The workflow succeeding, with a release containing `latest.json` and signed
  artifacts for all three targets, is the CI test.
- **Signature rejection:** point the app at a manifest signed with a different
  key and confirm it refuses rather than installs. This is the security-critical
  path and is the one update behaviour worth testing directly.
- Version comparison belongs to the plugin and is not re-tested here.
- Two releases are needed to exercise anything: `v0.1.0` as the baseline the
  installed app reports, then `v0.2.0` for it to discover.
