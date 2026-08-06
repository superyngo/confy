use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::ConfyPickerExt;
use crate::Result;

#[command]
pub(crate) async fn pick_writable<R: Runtime>(app: AppHandle<R>) -> Result<PickWritableResponse> {
    app.confy_picker().pick_writable()
}

#[command]
pub(crate) async fn create_writable<R: Runtime>(
    app: AppHandle<R>,
    suggested_name: String,
) -> Result<PickWritableResponse> {
    app.confy_picker().create_writable(&suggested_name)
}
