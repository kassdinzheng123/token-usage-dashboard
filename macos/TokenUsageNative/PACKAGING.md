# Native macOS Packaging

This package builds a SwiftUI app that starts the Rust HTTP server from the app bundle.

## Build

```bash
npm run build:native-mac
```

The output is:

```text
release/native/Token Usage Dashboard.app
```

## Bundle Layout

```text
Token Usage Dashboard.app/
  Contents/
    MacOS/
      TokenUsageNative
    Resources/
      AppIcon.icns
      Backend/
        token-usage-server
```

The Swift app starts `Resources/Backend/token-usage-server` on `127.0.0.1:3456`.

## Signing

Ad-hoc signing for local testing:

```bash
codesign --force --deep --sign - "release/native/Token Usage Dashboard.app"
```

Developer ID distribution requires a real Developer ID Application certificate and notarization:

```bash
CODESIGN_IDENTITY="Developer ID Application: Example Team (TEAMID)" \
NOTARY_PROFILE="token-usage-notary" \
npm run dist:native-mac
```

The `NOTARY_PROFILE` should be created with `xcrun notarytool store-credentials`. As an alternative, set `APPLE_ID`, `APPLE_TEAM_ID`, and `APPLE_APP_SPECIFIC_PASSWORD`.

For a local-only DMG that is not suitable for distribution:

```bash
npm run dist:native-mac:local
```

## Notes

- The app requires macOS 14.0 or later.
- The app is not sandboxed. Sandboxing would require explicit user-granted file access for `~/.claude`, `~/.codex`, `~/.opencode`, `~/.hermes`, `~/.openclaw`, `~/.factory`, and related config directories.
- The native package does not include a Node runtime or Node backend files.
