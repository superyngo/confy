use serde::de::DeserializeOwned;
use tauri::{plugin::PluginApi, AppHandle, Runtime};

use crate::models::*;

pub fn init<R: Runtime, C: DeserializeOwned>(
    app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> crate::Result<ConfyPicker<R>> {
    Ok(ConfyPicker(app.clone()))
}

/// Access to the confy-picker APIs. Desktop has no `ACTION_OPEN_DOCUMENT`
/// equivalent to shim — desktop keeps using tauri-plugin-dialog directly, so
/// this is never invoked there.
pub struct ConfyPicker<R: Runtime>(#[allow(dead_code)] AppHandle<R>);

impl<R: Runtime> ConfyPicker<R> {
    pub fn pick_writable(&self) -> crate::Result<PickWritableResponse> {
        Err(crate::Error::Unsupported)
    }
}
