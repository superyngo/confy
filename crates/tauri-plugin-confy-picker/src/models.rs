use serde::{Deserialize, Serialize};

/// The chosen document's `content://` URI (Android) or `file://` path (iOS),
/// with a persistable read+write grant already taken. `None` when the user
/// cancels the picker.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PickWritableResponse {
    pub uri: Option<String>,
    /// The real display name (Android: queried via the SAF `DISPLAY_NAME`
    /// column — `content://` URIs are opaque and don't reliably embed a
    /// filename, so this is the only way `fs.ts` can recover the real
    /// extension for format detection). Without this field declared here,
    /// serde silently drops the Kotlin plugin's `name` key when deserializing
    /// the JNI response into this struct before it's re-serialized back to JS.
    pub name: Option<String>,
}
