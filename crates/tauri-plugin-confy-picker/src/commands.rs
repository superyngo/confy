use tauri::{command, AppHandle, Runtime};

use crate::models::*;
use crate::ConfyPickerExt;
use crate::Result;

#[command]
pub(crate) async fn pick_writable<R: Runtime>(app: AppHandle<R>) -> Result<PickWritableResponse> {
    app.confy_picker().pick_writable()
}
