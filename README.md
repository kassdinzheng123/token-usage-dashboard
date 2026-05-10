# Token Usage Dashboard

Token Usage Dashboard is a native macOS menu bar and dashboard app for inspecting local token, model, and cost usage across supported AI coding assistants. The app is built with SwiftUI and starts a bundled Rust HTTP backend on `127.0.0.1`.

The current native package targets Apple silicon and requires macOS 14.0 or later.

## What It Includes

- A SwiftUI macOS app in `macos/TokenUsageNative`.
- A Rust backend in `rust-backend` using Axum on `127.0.0.1:3456`.
- Packaging scripts in `scripts` that bundle the Swift app and Rust backend into a `.app` or `.dmg`.
- Local usage readers for supported sources such as Claude, Codex, OpenCode, Hermes, OpenClaw, Pi, and Factory Droid.

The backend reads local tool data from user-owned directories and writes only application cache/log data under `~/Library/Application Support/Token Usage Dashboard`.

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
GET /api/today
GET /api/{source}/{view}
```

Supported `source` values include `claude`, `codex`, `opencode`, `hermes`, `openclaw`, `pi`, and `factory`. Supported `view` values include `daily`, `monthly`, `sessions`, and `blocks` where available.

## Run Backend Tests

```bash
cargo test --manifest-path rust-backend/Cargo.toml
```

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

The backend has a cleanup command for application-owned cache data. Always inspect first:

```bash
cargo run --release --manifest-path rust-backend/Cargo.toml -- cleanup --dry-run
```

Apply only after reviewing the dry run:

```bash
cargo run --release --manifest-path rust-backend/Cargo.toml -- cleanup --apply
```

Cleanup is intended for app-owned cache data only; it should not delete source records from Claude, Codex, OpenCode, Hermes, OpenClaw, or other tool-owned directories.

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
