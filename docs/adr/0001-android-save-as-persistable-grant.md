# Android Save As uses a custom SAF plugin command, not stock `tauri-plugin-dialog`

Save As must survive an app kill + relaunch, the same durability contract confy already
guarantees for opened files. Stock `tauri-plugin-dialog`'s Android `saveFileDialog` uses the
correct SAF action (`ACTION_CREATE_DOCUMENT`) but never calls `takePersistableUriPermission`,
so its write grant doesn't reliably survive process death. We added `create_writable` to the
existing `tauri-plugin-confy-picker` crate (mirroring its `pick_writable` command, built for
the identical gap on the read side during Mobile M1) instead of patching or forking the
upstream plugin. `pickSaveFile()` now forks to `confy-picker` on Android, matching
`pickOpenFile()`'s existing fork — desktop/iOS keep the stock plugin unchanged.
