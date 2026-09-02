use anyhow::{Context, Result};
use confy_core::model::document::DocFormat;
use std::io::Write;
use std::process::Command;

/// Open `initial` in $EDITOR (fallback $VISUAL, then vi/notepad), return edited text.
/// `format` picks the temp-file suffix so the editor applies the right syntax mode.
pub fn edit_text(initial: &str, format: DocFormat) -> Result<String> {
    let suffix = match format {
        DocFormat::Toml => ".toml",
        DocFormat::Json => ".json",
        DocFormat::Yaml => ".yaml",
    };
    let mut tmp = tempfile::Builder::new().suffix(suffix).tempfile()?;
    tmp.write_all(initial.as_bytes())?;
    let path = tmp.path().to_path_buf();
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(default_editor);
    // `EDITOR="code --wait"` / `"emacsclient -t"` are common: split the value
    // shell-style so the flags become args rather than part of the program name.
    let mut words = shell_words::split(&editor)
        .with_context(|| format!("parsing $EDITOR: {editor}"))?
        .into_iter();
    let program = words
        .next()
        .ok_or_else(|| anyhow::anyhow!("$EDITOR is empty"))?;
    let status = Command::new(&program)
        .args(words)
        .arg(&path)
        .status()
        .with_context(|| format!("launching editor: {editor}"))?;
    anyhow::ensure!(status.success(), "editor exited non-zero");
    Ok(std::fs::read_to_string(&path)?)
}

fn default_editor() -> String {
    // `vi` is POSIX-mandated, so it is present on minimal/headless Unix systems
    // where `nano` may be absent — exactly the environments confy may run in.
    if cfg!(windows) {
        "notepad".into()
    } else {
        "vi".into()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // These tests (and tui/tests.rs's editor test) mutate the process-wide `$EDITOR`; run them one at a time.
    pub(crate) static ENV_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[cfg(unix)]
    #[test]
    fn edit_text_reads_back_editor_output() {
        let _g = ENV_LOCK.lock();
        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        std::fs::write(script.path(), "#!/bin/sh\necho 'port = 9090' > \"$1\"\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(script.path(), std::fs::Permissions::from_mode(0o755))
                .unwrap();
            std::env::set_var("EDITOR", script.path());
            let out = edit_text("port = 8080\n", DocFormat::Toml).unwrap();
            assert_eq!(out.trim(), "port = 9090");
        }
    }

    #[cfg(unix)]
    #[test]
    fn edit_text_splits_editor_flags_and_uses_format_suffix() {
        // `$EDITOR` carrying flags (`code --wait` style): the flag must become
        // an argument, and the temp file must carry the document's extension.
        let _g = ENV_LOCK.lock();
        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        std::fs::write(
            script.path(),
            "#!/bin/sh\n[ \"$1\" = \"--flag\" ] || exit 3\ncase \"$2\" in *.yaml) echo \"ok: $1\" > \"$2\";; *) exit 4;; esac\n",
        )
        .unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(script.path(), std::fs::Permissions::from_mode(0o755))
                .unwrap();
            std::env::set_var("EDITOR", format!("{} --flag", script.path().display()));
            let out = edit_text("a: 1\n", DocFormat::Yaml).unwrap();
            assert_eq!(out.trim(), "ok: --flag");
        }
    }
}
