# Tauri Plugin confy-picker

First-party Tauri plugin for Android file I/O with durable write access. Stock
`tauri-plugin-dialog`'s Android picker (`ACTION_GET_CONTENT`) never calls
`takePersistableUriPermission`, so a handle opened through it loses write access once the
process is killed. This plugin exposes two commands that do:

- `pick_writable` — opens the SAF document picker (`ACTION_OPEN_DOCUMENT`) and takes a
  persistable read/write URI grant on the result.
- `create_writable` — opens the SAF "Save As" picker (`ACTION_CREATE_DOCUMENT`) and takes
  the same persistable grant on the newly created document.

Both commands grant access to exactly the *document* being opened/created, not a parent
directory — there is no way to resolve a second file relative to it. See
`docs/adr/0001-android-save-as-persistable-grant.md` for the design rationale and `TAURI.md`
for how the web layer forks to these commands on Android.
