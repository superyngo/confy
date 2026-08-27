# RELEASES.md — distribution channels

Where each build of confy ships, how it gets there, and current status. Mechanics for
each channel live in the referenced doc — this is just the map.

| Platform / channel | Method | Trigger | Current version | Status |
|---|---|---|---|---|
| TUI binaries (Linux/macOS/Windows, `confy`) | GitHub Releases | `.github/workflows/release.yml`, tag `v*.*.*` | v0.21.0 | Live |
| Desktop app (macOS `.dmg`, Windows portable `.exe`) | GitHub Releases | same workflow, tag `v*.*.*` | v0.21.0 | Live — unsigned/un-notarized (see README § Desktop app) |
| Windows Microsoft Store (`.msix`) | Partner Center Submission API (`msstore` CLI) | `.github/workflows/publish-msstore.yml`, dispatched by `publish-gate.yml` after `release.yml` succeeds on tag `v*.*.*`, gated behind its own `publish-gate-msstore` environment approval (checkable independently of other stores in the same review) | v0.21.0 | Live |
| Android (Tauri mobile) | Sideload debug APK | manual `cargo tauri android build --debug --apk`, no CI | — | Dev/sideload only, not distributed |
| Android Google Play (`.aab`) | Google Play Console | manual upload during development; CI publish (`publish-play.yml` + `publish-gate-play`) planned once account exists | — | In development — release signing (`keystore.properties`) + tag-derived `versionCode` verified end-to-end (debug + release APK build/sign/install/launch on real hardware, 2026-08-06); Save As + "Open with"/share chooser visibility fixed and verified on real hardware (M2, 2026-08-06); no Play Console account yet, no testers, `publish-play.yml` CI not built |
| Web UI | Cloudflare Workers Builds (Git integration) | push to `main` | rolling (no version tag) | Live at <https://confy.turkeyang.net/> |
| VS Code extension | VS Marketplace + Open VSX | `.github/workflows/publish-vscode.yml`, dispatched by `publish-gate.yml` after `release.yml` succeeds on tag `v*.*.*`, gated behind its own `publish-gate-vscode` environment approval (versioned in lockstep with the app) | v0.21.0 | Live |

Not targeted yet: Linux/iOS desktop-app builds (Tauri), F-Droid for Android.

All store listings' privacy policy field points to <https://confy.turkeyang.net/privacy>
(`web/privacy.html`, mirrors `PRIVACY.md`) — set manually per store dashboard, not
CI-automated.

## Details

- TUI + desktop + MSIX: [README.md](README.md) § Desktop app, [TAURI.md](docs/reference/TAURI.md), [crates/confy-tauri/msix/STORE.md](crates/confy-tauri/msix/STORE.md)|||| v0.21.0 
- Android: [TAURI.md](docs/reference/TAURI.md) § Mobile (Tauri Android)
- Web UI: [WEBUI.md](docs/reference/WEBUI.md) § Deployment
- VS Code extension: [VSCODE.md](docs/reference/VSCODE.md) § Publishing, [editors/vscode/README.md](editors/vscode/README.md) § Publishing a new version|||| v0.21.0 
