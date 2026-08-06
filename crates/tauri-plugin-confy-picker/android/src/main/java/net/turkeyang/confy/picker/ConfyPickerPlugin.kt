package net.turkeyang.confy.picker

import android.app.Activity
import android.content.Intent
import android.provider.OpenableColumns
import androidx.activity.result.ActivityResult
import app.tauri.annotation.ActivityCallback
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

@InvokeArg
class CreateWritableArgs {
    lateinit var suggestedName: String
}

/**
 * Standalone picker for confy's mobile M1 write-in-place flow.
 *
 * tauri-plugin-dialog's Android `open()` uses `Intent.ACTION_GET_CONTENT`,
 * which never grants a persistable write URI permission — every write back
 * to a picked file fails immediately (not just after a restart). This plugin
 * uses `ACTION_OPEN_DOCUMENT` instead, which supports
 * `FLAG_GRANT_WRITE_URI_PERMISSION` + `takePersistableUriPermission`, so the
 * returned URI can be re-read/re-written after the app fully restarts.
 */
@TauriPlugin
class ConfyPickerPlugin(private val activity: Activity) : Plugin(activity) {
    @Command
    fun pickWritable(invoke: Invoke) {
        val intent = Intent(Intent.ACTION_OPEN_DOCUMENT)
        intent.addCategory(Intent.CATEGORY_OPENABLE)
        intent.type = "*/*"
        intent.addFlags(
            Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
        )
        startActivityForResult(invoke, intent, "pickWritableResult")
    }

    /**
     * `ACTION_CREATE_DOCUMENT` counterpart to [pickWritable] for confy's
     * mobile M2 Save-As flow — mirrors it exactly except for the intent
     * action/title, since stock `tauri-plugin-dialog`'s Android
     * `saveFileDialog` never calls `takePersistableUriPermission` (see
     * docs/adr/0001-android-save-as-persistable-grant.md).
     */
    @Command
    fun createWritable(invoke: Invoke) {
        val args = invoke.parseArgs(CreateWritableArgs::class.java)
        val intent = Intent(Intent.ACTION_CREATE_DOCUMENT)
        intent.addCategory(Intent.CATEGORY_OPENABLE)
        intent.type = "*/*"
        intent.putExtra(Intent.EXTRA_TITLE, args.suggestedName)
        intent.addFlags(
            Intent.FLAG_GRANT_READ_URI_PERMISSION or
                Intent.FLAG_GRANT_WRITE_URI_PERMISSION or
                Intent.FLAG_GRANT_PERSISTABLE_URI_PERMISSION,
        )
        startActivityForResult(invoke, intent, "createWritableResult")
    }

    @ActivityCallback
    fun createWritableResult(invoke: Invoke, result: ActivityResult) {
        val ret = JSObject()
        val uri = if (result.resultCode == Activity.RESULT_OK) result.data?.data else null
        if (uri != null) {
            activity.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
            ret.put("uri", uri.toString())
            ret.put("name", queryDisplayName(uri))
        } else {
            ret.put("uri", null)
            ret.put("name", null)
        }
        invoke.resolve(ret)
    }

    @ActivityCallback
    fun pickWritableResult(invoke: Invoke, result: ActivityResult) {
        val ret = JSObject()
        val uri = if (result.resultCode == Activity.RESULT_OK) result.data?.data else null
        if (uri != null) {
            activity.contentResolver.takePersistableUriPermission(
                uri,
                Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION,
            )
            ret.put("uri", uri.toString())
            // `content://` URIs are opaque (e.g. the Downloads provider hands out
            // ".../document/31" — no filename, so the caller's fallback of
            // splitting the URI on "/" silently loses the extension and every
            // format guess defaults to TOML). Query the real display name via
            // the documented SAF column instead of guessing from the URI shape.
            ret.put("name", queryDisplayName(uri))
        } else {
            ret.put("uri", null)
            ret.put("name", null)
        }
        invoke.resolve(ret)
    }

    private fun queryDisplayName(uri: android.net.Uri): String? {
        return try {
            // Null projection (not `arrayOf(DISPLAY_NAME)`) — some providers (seen live:
            // the Downloads provider's "msf:" media-store-file passthrough IDs) don't
            // honor a narrow projection and return no rows/columns at all for it.
            activity.contentResolver.query(uri, null, null, null, null)?.use { cursor ->
                if (cursor.moveToFirst()) {
                    val idx = cursor.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                    if (idx >= 0) cursor.getString(idx) else null
                } else {
                    null
                }
            }
        } catch (e: Exception) {
            android.util.Log.e("ConfyPicker", "queryDisplayName failed for $uri", e)
            null
        }
    }
}
