# Plan: Multi-stage analysis of ~/CodeSpace/token-usage repository

**Source:** self-plan (planner-spawn fallback)
**TaskType:** analysis
**Overall Difficulty:** high

## Step N1: Core purpose and boundary analysis

- **Task:** Read README.md, LICENSE, CLAUDE.md, AGENTS.md, docs/ (rust-backend-migration.md, performance-audit.md, performance-audit-plan.md, performance-review-v2.md). Identify: what the app does (token usage dashboard for AI coding assistants), what it does NOT do, the supported sources (claude, codex, opencode, hermes, openclaw, pi, factory), the migration from Electron to native, and any stated design principles or constraints.
- **Depends on:** none
- **Done when:** A structured summary of core purpose, supported sources, boundaries, and key project decisions is produced.
- **Difficulty:** low

## Step N2: Directory structure and module responsibility mapping

- **Task:** Explore the full file tree excluding node_modules/target/.build/release/.git/ccusage. Map each directory and key file to its role: rust-backend/src/ (server modules), macos/TokenUsageNative/Sources/ (SwiftUI app), scripts/ (build/packaging), docs/ (project docs), build/ and dist/ (legacy Electron artifacts), index.mjs (bundled JS), root-level audit/report files. Identify orphaned or unclear directories.
- **Depends on:** none
- **Done when:** A complete annotated directory tree is produced showing the purpose of every major directory and file.
- **Difficulty:** low

## Step N3: Tech stack, dependencies, build system, and testing

- **Task:** Analyze package.json (scripts, dependencies), rust-backend/Cargo.toml (axum, tokio, rusqlite, chrono, reqwest, etc.), macos/TokenUsageNative/Package.swift, tsconfig.json, scripts/build-native-mac.mjs, and index.mjs. Document: Rust build pipeline, Swift build pipeline, Node.js build/packaging scripts, DMG creation + notarization, Electron legacy build (if any), test infrastructure (cargo test, compare-rust-backend.mjs). Note any dependency version risks or missing deps.
- **Depends on:** none
- **Done when:** A complete tech stack inventory and build/run/test command reference is produced.
- **Difficulty:** medium

## Step N4: Data flow, API surface, and execution paths

- **Task:** Read rust-backend/src/main.rs, server.rs, protocol.rs, cache.rs, cleanup.rs, lib.rs, sources/mod.rs, and 2-3 source readers (claude.rs, openclaw.rs, codex.rs). Trace: HTTP request → route handler → source reader dispatch → SQLite/file I/O → cache → JSON response. Map all API routes (health, refresh, today, /api/{source}/{view}). Read Swift frontend: TokenUsageAPIClient.swift, LiveTokenUsageDashboardStore.swift, LocalServerProcess.swift to understand the frontend→backend data consumption pattern.
- **Depends on:** N2
- **Done when:** A text-based data flow diagram showing the full request lifecycle from HTTP endpoint to response, including source reader dispatch, SQLite access, and caching, is produced.
- **Difficulty:** high

## Step N5: Top 3-5 critical code modules — deep dive

- **Task:** Select the 3-5 most critical/interesting modules based on N2+N4 findings and analyze deeply: sources/mod.rs (trait/registry pattern), server.rs (route architecture), protocol.rs (data models/serialization), cache.rs (caching strategy), cleanup.rs (safety model), LocalServerProcess.swift (backend process lifecycle). For each: purpose, design pattern, coupling, extensibility, strengths, weaknesses.
- **Depends on:** N4
- **Done when:** Detailed analysis of 3-5 modules is produced with design patterns identified and trade-offs evaluated.
- **Difficulty:** high

## Step N6: Structural issues, risks, and design smells

- **Task:** Identify problems: legacy Electron artifacts (build/, dist/, index.mjs) coexisting with native macOS app, data path assumptions (hardcoded user directories), cache invalidation strategy, cleanup safety (dry-run vs apply), error handling completeness, security audit findings (security-audit.json, audit-report.json, audit-rust-backend.json), test coverage gaps, dependency risks, packaging/deployment risks (ad-hoc signing, Gatekeeper issues), and the ccusage/ directory status.
- **Depends on:** N3, N4, N5
- **Done when:** A prioritized risk list (critical/high/medium) with specific file references is produced.
- **Difficulty:** medium

## Step N7: Development recommendations and next steps

- **Task:** Synthesize N1-N6 into prioritized recommendations: quick wins, architectural improvements, risk mitigations, feature priorities, CI/testing gaps. Rank by priority and estimated effort.
- **Depends on:** N1, N2, N3, N4, N5, N6
- **Done when:** A ranked recommendation list with 5-8 items is produced, each with priority, effort estimate, and rationale.
- **Difficulty:** medium
