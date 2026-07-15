use serde::de::DeserializeOwned;
use tauri::{
    plugin::{PluginApi, PluginHandle},
    AppHandle, Runtime,
};

use crate::models::*;

// initializes the Kotlin plugin class (Android only — this plugin exists to
// work around tauri-plugin-dialog's Android picker using ACTION_GET_CONTENT,
// which never grants write access; see confy mobile M1 plan)
pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> crate::Result<ConfyPicker<R>> {
    let handle = api.register_android_plugin("net.turkeyang.confy.picker", "ConfyPickerPlugin")?;
    Ok(ConfyPicker(handle))
}

/// Access to the confy-picker APIs.
pub struct ConfyPicker<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> ConfyPicker<R> {
    /// Opens a document picker (`ACTION_OPEN_DOCUMENT`) and takes a persistable
    /// read+write URI permission grant on the result, so the returned handle
    /// can be re-read/re-written after the app fully restarts.
    pub fn pick_writable(&self) -> crate::Result<PickWritableResponse> {
        self.0
            .run_mobile_plugin("pickWritable", ())
            .map_err(Into::into)
    }
}
