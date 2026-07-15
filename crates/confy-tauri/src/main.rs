//! Desktop entry point — see `lib.rs` for everything the app actually does.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    confy_tauri_lib::run();
}
