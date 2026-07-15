use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

pub use models::*;

#[cfg(desktop)]
mod desktop;
#[cfg(mobile)]
mod mobile;

mod commands;
mod error;
mod models;

pub use error::{Error, Result};

#[cfg(desktop)]
use desktop::ConfyPicker;
#[cfg(mobile)]
use mobile::ConfyPicker;

/// Extensions to [`tauri::App`], [`tauri::AppHandle`] and [`tauri::Window`] to access the confy-picker APIs.
pub trait ConfyPickerExt<R: Runtime> {
    fn confy_picker(&self) -> &ConfyPicker<R>;
}

impl<R: Runtime, T: Manager<R>> crate::ConfyPickerExt<R> for T {
    fn confy_picker(&self) -> &ConfyPicker<R> {
        self.state::<ConfyPicker<R>>().inner()
    }
}

/// Initializes the plugin.
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("confy-picker")
        .invoke_handler(tauri::generate_handler![commands::pick_writable])
        .setup(|app, api| {
            #[cfg(mobile)]
            let confy_picker = mobile::init(app, api)?;
            #[cfg(desktop)]
            let confy_picker = desktop::init(app, api)?;
            app.manage(confy_picker);
            Ok(())
        })
        .build()
}
