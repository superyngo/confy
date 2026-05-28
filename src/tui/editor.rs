use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;

/// Open `initial` in $EDITOR (fallback $VISUAL, then nano/vi/notepad), return edited text.
pub fn edit_text(initial: &str) -> Result<String> {
    let mut tmp = tempfile::Builder::new().suffix(".toml").tempfile()?;
    tmp.write_all(initial.as_bytes())?;
    let path = tmp.path().to_path_buf();
    let editor = std::env::var("EDITOR")
        .or_else(|_| std::env::var("VISUAL"))
        .unwrap_or_else(|_| default_editor());
    let status = Command::new(&editor).arg(&path).status()
        .with_context(|| format!("launching editor: {editor}"))?;
    anyhow::ensure!(status.success(), "editor exited non-zero");
    Ok(std::fs::read_to_string(&path)?)
}

fn default_editor() -> String {
    // `vi` is POSIX-mandated, so it is present on minimal/headless Unix systems
    // where `nano` may be absent — exactly the environments confy may run in.
    if cfg!(windows) { "notepad".into() } else { "vi".into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn edit_text_reads_back_editor_output() {
        let script = tempfile::Builder::new().suffix(".sh").tempfile().unwrap();
        std::fs::write(script.path(), "#!/bin/sh\necho 'port = 9090' > \"$1\"\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(script.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
            std::env::set_var("EDITOR", script.path());
            let out = edit_text("port = 8080\n").unwrap();
            assert_eq!(out.trim(), "port = 9090");
        }
    }
}
