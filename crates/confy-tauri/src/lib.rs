//! confy desktop/mobile — a thin Tauri shell over the existing web UI.
//!
//! Editing stays in the in-webview wasm `Session` (`dispatch` is synchronous and
//! called from ~100 keyboard handlers; moving it across an async IPC boundary
//! would buy nothing). This crate owns only the part that genuinely needs a
//! native host: registering `tauri-plugin-dialog`/`tauri-plugin-fs` (open/save
//! dialogs, reads/writes to a picked path) plus two custom commands those
//! stock plugins can't cover — a file passed on the command line
//! (`startup_file`, desktop only) and files the OS opens the app *with*
//! (`opened_urls` + an `"opened"` event, mobile's "Open with" file-association
//! intent — Task 3). The frontend's `web/fs.ts` calls the plugins directly via
//! `window.__TAURI__.dialog` / `window.__TAURI__.fs` when running under Tauri,
//! and falls back to the browser File System Access API otherwise. On Android,
//! `startup_file`'s two stock plugins can't grant a persistable write
//! permission for a picked `content://` URI (`tauri-plugin-dialog` uses
//! `ACTION_GET_CONTENT`, which never does) — `tauri-plugin-confy-picker` is
//! the custom fix (Task 0 finding), registered only on Android. A
//! file-association open instead grants its own (non-persistable, but
//! session-long) URI permission via the launch intent itself, so
//! `opened_urls` needs no such plugin — `tauri-plugin-fs`'s
//! `readTextFile`/`writeTextFile` already handle a granted `content://` URI
//! directly, the same way they handle the picker's URIs.
//!
//! `main.rs` (desktop bin) and the generated Android/iOS entry points both
//! call [`run`] — the `#[cfg_attr(mobile, tauri::mobile_entry_point)]` split
//! is the standard Tauri v2 shape for a crate that ships both a desktop
//! binary and a mobile library.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
use tauri::{Emitter, Manager, RunEvent};

/// A file the host has read for the frontend: its absolute path (used later as
/// the opaque save "handle"), its display name, and its current text.
#[derive(Serialize)]
struct OpenedFile {
    path: String,
    name: String,
    text: String,
}

fn read_opened(path: &Path) -> Result<OpenedFile, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("reading {}: {e}", path.display()))?;
    let name = path
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    Ok(OpenedFile {
        path: path.to_string_lossy().into_owned(),
        name,
        text,
    })
}

/// A file passed on the command line (`confy-desktop some.toml`), if it exists.
#[tauri::command]
fn startup_file() -> Option<OpenedFile> {
    let arg = std::env::args().nth(1)?;
    let path = PathBuf::from(arg);
    if path.is_file() {
        read_opened(&path).ok()
    } else {
        None
    }
}

/// URLs (`content://…` on Android, `file://…` elsewhere) the OS asked us to
/// open before the frontend had registered its `"opened"` listener — a cold
/// start via a file-association "Open with". Drained on read: the frontend
/// calls this once at boot, so a later call returns nothing new. A warm app
/// gets the same URLs via the `"opened"` event instead (see [`run`]).
#[tauri::command]
fn opened_urls(state: tauri::State<OpenedUrls>) -> Vec<String> {
    std::mem::take(&mut *state.0.lock().unwrap())
}

#[derive(Default)]
struct OpenedUrls(Mutex<Vec<String>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .manage(OpenedUrls::default())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_confy_picker::init());
    let app = builder
        .invoke_handler(tauri::generate_handler![startup_file, opened_urls])
        .build(tauri::generate_context!())
        .expect("error while building confy");
    app.run(|_app_handle, _event| {
        #[cfg(any(target_os = "macos", target_os = "ios", target_os = "android"))]
        if let RunEvent::Opened { urls } = _event {
            let urls: Vec<String> = urls.into_iter().map(|u| u.to_string()).collect();
            if let Some(window) = _app_handle.get_webview_window("main") {
                let _ = window.emit("opened", &urls);
            }
            _app_handle
                .state::<OpenedUrls>()
                .0
                .lock()
                .unwrap()
                .extend(urls);
        }
    });
}
