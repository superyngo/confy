//! confy desktop — a thin Tauri shell over the existing web UI.
//!
//! Editing stays in the in-webview wasm `Session` (`dispatch` is synchronous and
//! called from ~100 keyboard handlers; moving it across an async IPC boundary
//! would buy nothing). This binary owns only the part that genuinely needs the
//! desktop: **native file I/O** — real open/save dialogs, in-place writes to an
//! arbitrary path, and opening a file passed on the command line. The frontend's
//! `web/fs.ts` routes its host I/O through these commands when running under
//! Tauri (`window.__TAURI__`), and falls back to the browser File System Access
//! API otherwise.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

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

/// Native open dialog → read the chosen file. `None` when the user cancels.
///
/// `async` is load-bearing: it moves the command off the main thread, which the
/// `blocking_*` dialog APIs must not block (on macOS the dialog would appear
/// frozen — the main run loop can't pump its events).
#[tauri::command]
async fn open_dialog(app: tauri::AppHandle) -> Option<OpenedFile> {
    let picked = app
        .dialog()
        .file()
        .add_filter("Config", &["toml", "json", "jsonc", "yaml", "yml"])
        .blocking_pick_file()?;
    let path = picked.into_path().ok()?;
    read_opened(&path).ok()
}

/// Native save dialog → the chosen destination path. `None` when cancelled.
/// `async` for the same main-thread reason as [`open_dialog`].
#[tauri::command]
async fn save_dialog(app: tauri::AppHandle, suggested: String) -> Option<String> {
    let picked = app
        .dialog()
        .file()
        .set_file_name(&suggested)
        .blocking_save_file()?;
    let path = picked.into_path().ok()?;
    Some(path.to_string_lossy().into_owned())
}

/// Re-read an already-known path (the frontend's `FsHandle.getFile` analogue).
#[tauri::command]
fn read_file_text(path: String) -> Result<String, String> {
    std::fs::read_to_string(&path).map_err(|e| format!("reading {path}: {e}"))
}

/// In-place write to a known path (the frontend's `writeFile` analogue).
#[tauri::command]
fn write_file(path: String, contents: String) -> Result<(), String> {
    std::fs::write(&path, contents).map_err(|e| format!("writing {path}: {e}"))
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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            open_dialog,
            save_dialog,
            read_file_text,
            write_file,
            startup_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running confy desktop");
}
