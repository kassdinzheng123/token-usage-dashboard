# Rust Backend Migration Contract

This document defines the boundary for parallel Node and Rust backend work. The Rust backend should first match the current HTTP contract and data safety behavior before it replaces the Node backend in production builds.

## Parallel Development Boundary

- Node remains the reference implementation until contract comparison passes for the agreed endpoints.
- Rust backend work owns only the new backend implementation and its internal cache format.
- Shared client behavior must not be changed to compensate for Rust differences during the contract phase.
- Swift/native launcher changes should wait until the Rust backend has a stable port, health response, and cleanup story.
- The compare helper in `scripts/compare-rust-backend.mjs` is the first-line contract check, not a replacement for end-to-end UI testing.

## API Contract

The Rust backend must preserve these routes and return JSON with the same structural shape as Node:

- `GET /api/health`
- `GET /api/claude/daily`
- `GET /api/claude/monthly`
- `GET /api/claude/sessions`
- `GET /api/claude/blocks`
- `GET /api/codex/daily`
- `GET /api/codex/monthly`
- `GET /api/codex/sessions`
- `GET /api/codex/blocks`
- `GET /api/opencode/daily`
- `GET /api/opencode/monthly`
- `GET /api/opencode/sessions`
- `GET /api/opencode/blocks`
- `GET /api/hermes/daily`
- `GET /api/hermes/monthly`
- `GET /api/hermes/sessions`
- `GET /api/hermes/blocks`
- `GET /api/openclaw/daily`
- `GET /api/openclaw/monthly`
- `GET /api/openclaw/sessions`
- `GET /api/openclaw/blocks`

Contract compatibility means:

- Same success status for normal local data reads.
- JSON parseable responses for every listed endpoint.
- Same top-level JSON type.
- Same object field paths and primitive type categories for health and sampled usage entries.
- Array endpoints may differ in item count while migration work is in progress, but item field structure should not drift.
- Unsupported `blocks` views for non-Claude sources should stay stable as empty JSON arrays unless the UI contract changes deliberately.

Query parameters used by the frontend, such as `since`, `until`, and `refresh`, should remain accepted even when a route can validly ignore them.

## Migration Order

1. Keep Node as the default backend and add Rust behind a separate local port.
2. Implement Rust health and read-only source routes without changing client code.
3. Run the compare helper against Node and Rust on representative local data.
4. Fix structural contract drift in Rust before changing launcher or packaging behavior.
5. Add cleanup commands with `--dry-run` behavior before any destructive mode exists.
6. Switch development packaging to Rust only after API comparison and cleanup dry runs are repeatable.
7. Remove Node backend packaging only after Rust has equivalent read behavior, cache migration, and recovery documentation.

## Transitional Legacy Bridge

The current Rust backend owns the public Swift-facing HTTP port, but Claude, Codex, and OpenCode are still delegated to the bundled Node backend during the transition. Rust starts that legacy backend on `127.0.0.1:3458` by default and proxies those source reads internally. Hermes and OpenClaw are read by Rust directly.

The bridge is intentionally temporary:

- `TOKEN_USAGE_LEGACY_BACKEND_DIR` can point Rust at a bundled backend directory when the current working directory is not `Contents/Resources/Backend`.
- `TOKEN_USAGE_LEGACY_PORT` can override the internal Node port.
- Swift and the UI should keep talking only to Rust on the public `/api` contract.
- Once Claude, Codex, and OpenCode are native Rust sources, remove the bridge and Node packaging together.

## Data Cleanup Strategy

User original data is read-only migration input. Cleanup code must never delete or mutate source records under Claude, Codex, OpenCode, Hermes, OpenClaw, or similar tool-owned directories.

Cache cleanup is limited to application-owned cache files:

- Rust writes a cache v2 format under a Rust-owned cache path.
- Old Node cache files should be backed up before removal or conversion.
- Existing cache files should be treated as disposable derived data, but still backed up during migration to aid rollback.
- Backup names should include a timestamp and the source cache version when known.
- Cache cleanup should be idempotent: running it twice should not corrupt backups or remove user originals.

Cleanup commands must follow a two-mode rule:

- `cleanup --dry-run` lists planned actions, affected paths, byte counts when available, and backup destinations. It must not delete, move, truncate, or rewrite files.
- `cleanup --apply` performs only the actions printed by a dry run from the same cleanup planner logic. It must refuse ambiguous targets and should log completed backup and removal steps.

If cleanup cannot prove a path is application-owned cache data, it should skip the path and report why.

## Contract Check

Run Node and Rust on separate ports, then compare:

```bash
OLD_BASE_URL=http://127.0.0.1:3456/api \
NEW_BASE_URL=http://127.0.0.1:3457/api \
node scripts/compare-rust-backend.mjs
```

The helper prints one line per endpoint and exits with a non-zero status when it finds status, parse, or JSON structure differences.
