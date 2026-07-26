# Token Usage Dashboard

Token Usage Dashboard is a native macOS menu bar and dashboard app for inspecting local token, model, and cost usage across supported AI coding assistants. The app is built with SwiftUI and starts a bundled Rust HTTP backend on `127.0.0.1`.

The current native package targets Apple silicon and requires macOS 14.0 or later.

## What It Includes

- A SwiftUI macOS app in `macos/TokenUsageNative`.
- A Rust backend in `rust-backend` using Axum on `127.0.0.1:3456`.
- Packaging scripts in `scripts` that bundle the Swift app and Rust backend into a `.app` or `.dmg`.
- Local usage readers for supported sources such as Claude, Codex, OpenCode, Hermes, OpenClaw, Pi, and Factory Droid.

The backend incrementally imports local tool data into `~/Library/Application Support/Token Usage Dashboard/usage-ledger.sqlite`. API responses are served from that SQLite ledger, so source transcript cleanup does not remove already imported historical token counts. Set `TOKEN_USAGE_LEDGER_PATH` to use a different local ledger path for debugging or tests.

## Requirements

- macOS 14.0 or later.
- Apple silicon for the current packaged build.
- Node.js for the packaging script.
- Rust toolchain with Cargo.
- Swift toolchain from Xcode or Apple Command Line Tools.

## Install Dependencies

The root package currently has no runtime npm dependencies, but keeping the lockfile installed makes the scripts reproducible:

```bash
npm install
```

Build the Rust backend by itself:

```bash
npm run build:rust-backend
```

## Run The Backend For Debugging

Start the Rust backend on the default port:

```bash
cargo run --release --manifest-path rust-backend/Cargo.toml
```

Use a different port when comparing or when `3456` is already in use:

```bash
PORT=3457 cargo run --release --manifest-path rust-backend/Cargo.toml
```

Check that it is healthy:

```bash
curl http://127.0.0.1:3456/api/health
```

Common API routes:

```text
GET /api/health
GET /api/refresh
POST /api/sync
GET /api/today
GET /api/{source}/{view}
```

Supported `source` values include `claude`, `codex`, `opencode`, `hermes`, `openclaw`, `pi`, and `factory`. Supported `view` values include `daily`, `monthly`, `sessions`, and `blocks` where available.

## Run Backend Tests

```bash
cargo test --manifest-path rust-backend/Cargo.toml
```

## Sync Usage Across Devices With Git

Use a private Git repository as the transport for ledger records. The SQLite
ledger's usage sessions, blocks, and message-level rows are exported as a
deterministic JSONL snapshot under `.token-usage-sync/v1/devices/`. Large
device snapshots are split into 32 MiB files so they stay below Git hosting
limits. Imports write the merged records back into the local SQLite ledger and
remain compatible with the earlier single-file layout. The binary SQLite/WAL
files and the machine-local source scan watermark are not copied, so concurrent
devices remain mergeable and local source scans cannot be skipped accidentally.

Clone the private repository on every device, choose a different lowercase
device ID for each one, and make sure the current branch has an upstream.
The automatic command requires a clean working tree and performs
`pull --rebase`, SQLite import, snapshot export, commit, and push:

```bash
cargo run --release --manifest-path rust-backend/Cargo.toml -- \
  sync run --repo "$TOKEN_SYNC_REPO" --device macbook-pro
```

It never force-pushes. A concurrent non-fast-forward push is pulled and
retried once; authentication failures, conflicts, detached HEAD, missing
upstream branches, and unrelated working-tree changes are reported without
discarding data. `sync export` and `sync import` remain available for manual
workflows.

The macOS app exposes the same operation in **Settings → Sync**. Configure the
local repository path and device ID, then use **Sync Now** or enable automatic
sync every 15 minutes.

The Rust binary is also a headless, cross-platform sync client. On a non-macOS
device, build it with Cargo and point it at the SQLite ledger without starting
the HTTP server or UI. The one-shot command is suitable for cron or a systemd
timer:

```bash
cargo build --release --manifest-path rust-backend/Cargo.toml
TOKEN_USAGE_LEDGER_PATH=/path/to/usage-ledger.sqlite \
  rust-backend/target/release/token-usage-server \
  sync run --repo "$TOKEN_SYNC_REPO" --device linux-workstation
```

`TOKEN_USAGE_SYNC_DEVICE_ID=macbook-pro` can replace `--device`. The sync
commands use the normal ledger path and honor `TOKEN_USAGE_LEDGER_PATH`.
Snapshot files can contain local session identifiers and usage metadata; keep
the repository private.

## Build The Native macOS App

Build the `.app` bundle:

```bash
npm run build:native-mac
```

The output is:

```text
release/native/Token Usage Dashboard.app
```

The build script compiles:

1. `rust-backend/target/release/token-usage-server`
2. `macos/TokenUsageNative/.build/release/TokenUsageNative`
3. `release/native/Token Usage Dashboard.app`

Inside the app bundle, the Swift app launches:

```text
Contents/Resources/Backend/token-usage-server
```

## Package A Local DMG

For local testing or internal sharing, create an ad-hoc signed DMG:

```bash
npm run dist:native-mac:local
```

The output is:

```text
release/native/Token Usage Dashboard-1.0.1-native-arm64.dmg
```

This package uses ad-hoc signing. On another Mac, Gatekeeper may block it because it is not notarized. For internal testing, the recipient may need to remove quarantine after copying the app to `/Applications`:

```bash
xattr -dr com.apple.quarantine "/Applications/Token Usage Dashboard.app"
```

## Package A Notarized DMG

Public macOS distribution requires an Apple Developer ID Application certificate and notarization.

First store a notary profile:

```bash
xcrun notarytool store-credentials token-usage-notary
```

Then build, sign, submit, staple, and verify the DMG:

```bash
CODESIGN_IDENTITY="Developer ID Application: Your Team (TEAMID)" \
NOTARY_PROFILE="token-usage-notary" \
npm run dist:native-mac
```

You can also provide notarization credentials directly:

```bash
CODESIGN_IDENTITY="Developer ID Application: Your Team (TEAMID)" \
APPLE_ID="you@example.com" \
APPLE_TEAM_ID="TEAMID" \
APPLE_APP_SPECIFIC_PASSWORD="xxxx-xxxx-xxxx-xxxx" \
npm run dist:native-mac
```

The script refuses to notarize without a real signing identity so it does not accidentally produce a public-looking but rejected DMG.

## Cleanup Commands

The backend has a cleanup command for temporary application-owned files. Always inspect first:

```bash
cargo run --release --manifest-path rust-backend/Cargo.toml -- cleanup --dry-run
```

Apply only after reviewing the dry run:

```bash
cargo run --release --manifest-path rust-backend/Cargo.toml -- cleanup --apply
```

Cleanup is intended for temporary app-owned files only; it should not delete the SQLite usage ledger or source records from Claude, Codex, OpenCode, Hermes, OpenClaw, or other tool-owned directories.

## Contract Comparison

When comparing two backend implementations, run them on different ports and use:

```bash
OLD_BASE_URL=http://127.0.0.1:3456/api \
NEW_BASE_URL=http://127.0.0.1:3457/api \
node scripts/compare-rust-backend.mjs
```

Set `COMPARE_VALUES=1` to compare numeric summaries as well as response shape.

## Repository Hygiene

The repository intentionally ignores local and generated data such as:

- `node_modules/`
- `build/`, `dist/`, and `release/`
- `rust-backend/target/`
- `macos/TokenUsageNative/.build/`
- `.idea/`, `.DS_Store`, and local secret files
- `ccusage/`, which is treated as a separate local reference checkout unless imported deliberately

Do not commit user usage databases, application logs, DMGs, `.app` bundles, signing certificates, provisioning profiles, or private environment files.
