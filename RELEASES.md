# RELEASES.md — distribution channels

Where each build of confy ships, how it gets there, and current status. Mechanics for
each channel live in the referenced doc — this is just the map.

| Platform / channel | Method | Trigger | Current version | Status |
|---|---|---|---|---|
| TUI binaries (Linux/macOS/Windows, `confy`) | GitHub Releases | `.github/workflows/release.yml`, tag `v*.*.*` | v0.16.0 | Live |
| Desktop app (macOS `.dmg`, Windows portable `.exe`) | GitHub Releases | same workflow, tag `v*.*.*` | v0.16.0 | Live — unsigned/un-notarized (see README § Desktop app) |
| Windows Microsoft Store (`.msix`) | Partner Center submission | built by `release.yml` every tag, submitted manually | v0.16.0 artifact | Not yet published — awaiting Partner Center registration |
| Android (Tauri mobile) | Sideload debug APK | manual `cargo tauri android build --debug --apk`, no CI | — | Dev/sideload only, not distributed |
| Web UI | Cloudflare Workers Builds (Git integration) | push to `main` | rolling (no version tag) | Live at <https://confy.turkeyang.net/> |
| VS Code extension | VS Marketplace + Open VSX | `.github/workflows/publish-vscode.yml`, tag `vscode-v*.*.*` | v0.3.0 | Live |

Not targeted yet: Linux/iOS desktop-app builds (Tauri), F-Droid/Play Store for Android.

## Details

- TUI + desktop + MSIX: [README.md](README.md) § Desktop app, [TAURI.md](TAURI.md), [crates/confy-tauri/msix/STORE.md](crates/confy-tauri/msix/STORE.md)
- Android: [TAURI.md](TAURI.md) § Mobile (Tauri Android)
- Web UI: [WEBUI.md](WEBUI.md) § Deployment
- VS Code extension: [VSCODE.md](VSCODE.md) § Publishing, [editors/vscode/README.md](editors/vscode/README.md) § Publishing a new version
