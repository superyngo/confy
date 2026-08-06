const COMMANDS: &[&str] = &["pick_writable", "create_writable"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .build();
}
