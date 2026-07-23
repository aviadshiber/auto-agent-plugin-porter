//! Install a user-level Codex `SessionStart` hook that runs the porter.
//!
//! Why user-level and not plugin-shipped: on the Codex CLI verified during
//! design (0.144.1) the plugin-manifest validator rejects a `hooks` key and
//! plugin-bundled hook discovery is unverified, so the reliable path is to
//! register the hook in `<CODEX_HOME>/hooks.json` — the same file Codex already
//! honors for user hooks. We MERGE into it (never clobber an unrelated hook such
//! as an existing `Stop` hook) and we NEVER touch Codex's hook-trust store:
//! Codex will prompt the user to trust the new hook on first use. That one-time
//! trust is by design and is documented for the user.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

pub struct InstallOptions {
    /// Codex config/home dir (contains `hooks.json`).
    pub codex_home: PathBuf,
    /// Absolute path to the built `agent-porter` binary.
    pub porter_bin: String,
    pub dry_run: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum InstallOutcome {
    Installed,
    Updated,
    AlreadyCurrent,
}

/// The shell command string a Codex SessionStart hook runs to import Claude
/// skills into Codex. Double-quoted so a binary path containing spaces works.
pub fn codex_session_command(porter_bin: &str) -> String {
    format!("\"{porter_bin}\" sync --source claude --target codex")
}

/// Return true if a hook command string is our porter's Claude→Codex sync,
/// regardless of the exact binary path (so we update rather than duplicate when
/// the install path changes).
fn is_our_command(cmd: &str) -> bool {
    cmd.contains("agent-porter")
        && cmd.contains("--source claude")
        && cmd.contains("--target codex")
}

/// Reject a binary path that could break out of the double-quoted hook command
/// string. `$` and backticks trigger command substitution even inside double
/// quotes; `"` closes the quote; newlines split the command. Backslash is left
/// alone — it is legitimate in Windows paths. The path is env-derived (not
/// attacker-controlled) under the normal threat model, so this is hardening.
fn is_safe_bin_path(p: &str) -> bool {
    !p.is_empty() && !p.contains(['"', '$', '`', '\n', '\r'])
}

/// Merge a `SessionStart` porter hook into `<codex_home>/hooks.json`.
pub fn install_codex_session_hook(opts: &InstallOptions) -> crate::Result<InstallOutcome> {
    if !is_safe_bin_path(&opts.porter_bin) {
        return Err(format!(
            "refusing to install hook: porter binary path contains unsafe characters: {:?}",
            opts.porter_bin
        )
        .into());
    }

    let hooks_path = opts.codex_home.join("hooks.json");
    let mut root = load_json_object(&hooks_path)?;

    let want_cmd = codex_session_command(&opts.porter_bin);

    // Ensure root.hooks is an object.
    let hooks = root
        .as_object_mut()
        .expect("load_json_object guarantees an object")
        .entry("hooks")
        .or_insert_with(|| json!({}));
    if !hooks.is_object() {
        return Err(format!("{}: `hooks` is not an object", hooks_path.display()).into());
    }
    let hooks_obj = hooks.as_object_mut().unwrap();

    // Ensure hooks.SessionStart is an array.
    let session = hooks_obj.entry("SessionStart").or_insert_with(|| json!([]));
    if !session.is_array() {
        return Err(format!(
            "{}: `hooks.SessionStart` is not an array",
            hooks_path.display()
        )
        .into());
    }
    let session_arr = session.as_array_mut().unwrap();

    // Look for an existing porter hook to update in place.
    let mut outcome = InstallOutcome::Installed;
    'search: for group in session_arr.iter_mut() {
        let Some(inner) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        for hook in inner.iter_mut() {
            let cur = hook.get("command").and_then(Value::as_str).unwrap_or("");
            if is_our_command(cur) {
                if cur == want_cmd {
                    return Ok(InstallOutcome::AlreadyCurrent);
                }
                hook["command"] = Value::String(want_cmd.clone());
                outcome = InstallOutcome::Updated;
                break 'search;
            }
        }
    }

    if outcome == InstallOutcome::Installed {
        session_arr.push(json!({
            "hooks": [
                {
                    "type": "command",
                    "command": want_cmd,
                    "statusMessage": "Porting Claude skills into Codex",
                    "timeout": 300
                }
            ]
        }));
    }

    if !opts.dry_run {
        if let Some(parent) = hooks_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut text = serde_json::to_string_pretty(&root)?;
        text.push('\n');
        write_atomic(&hooks_path, &text)?;
    }
    Ok(outcome)
}

/// Crash-safe write: keep a one-generation backup of the previous file, write
/// the new content to a sibling temp file, fsync it, then atomically rename it
/// over the target. A crash/full-disk can then only leave the intact original,
/// the intact backup, or a stray `.tmp` — never a truncated `hooks.json`. The
/// backup gives the user a recoverable copy if a later concurrent writer (rare;
/// this runs only at one-time bootstrap) races us.
fn write_atomic(path: &Path, contents: &str) -> crate::Result<()> {
    use std::io::Write;

    let dir = path
        .parent()
        .ok_or_else(|| format!("path has no parent: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("path has no file name: {}", path.display()))?;

    // Preserve a recoverable backup of the current file before replacing it.
    if path.exists() {
        let backup = dir.join(format!("{file_name}.bak"));
        let _ = fs::copy(path, &backup);
    }

    let tmp = dir.join(format!(".{file_name}.tmp.{}", std::process::id()));
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?; // durable on disk before the rename
    }
    // Atomic replace on the same filesystem.
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Load `path` as a JSON object, or return an empty object when the file is
/// absent. A present-but-non-object file is an error (we won't stomp it).
fn load_json_object(path: &Path) -> crate::Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text = fs::read_to_string(path)?;
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{}: invalid JSON: {e}", path.display()))?;
    if !value.is_object() {
        return Err(format!("{}: top-level value is not a JSON object", path.display()).into());
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn install(dir: &Path, bin: &str) -> InstallOutcome {
        install_codex_session_hook(&InstallOptions {
            codex_home: dir.to_path_buf(),
            porter_bin: bin.to_string(),
            dry_run: false,
        })
        .unwrap()
    }

    #[test]
    fn installs_then_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            install(dir.path(), "/opt/agent-porter"),
            InstallOutcome::Installed
        );
        assert_eq!(
            install(dir.path(), "/opt/agent-porter"),
            InstallOutcome::AlreadyCurrent
        );
        let text = fs::read_to_string(dir.path().join("hooks.json")).unwrap();
        // exactly one SessionStart porter hook
        assert_eq!(text.matches("--target codex").count(), 1);
    }

    #[test]
    fn updates_when_binary_path_changes() {
        let dir = tempfile::tempdir().unwrap();
        install(dir.path(), "/old/agent-porter");
        assert_eq!(
            install(dir.path(), "/new/agent-porter"),
            InstallOutcome::Updated
        );
        let text = fs::read_to_string(dir.path().join("hooks.json")).unwrap();
        assert!(text.contains("/new/agent-porter"));
        assert!(!text.contains("/old/agent-porter"));
        assert_eq!(text.matches("--target codex").count(), 1);
    }

    #[test]
    fn preserves_unrelated_hooks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("hooks.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"/usr/bin/plannotator"}]}]}}"#,
        )
        .unwrap();
        install(dir.path(), "/opt/agent-porter");
        let text = fs::read_to_string(dir.path().join("hooks.json")).unwrap();
        assert!(
            text.contains("plannotator"),
            "must not clobber the existing Stop hook"
        );
        assert!(
            text.contains("--target codex"),
            "must add the porter SessionStart hook"
        );
    }

    #[test]
    fn refuses_non_object_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("hooks.json"), "[1,2,3]").unwrap();
        let r = install_codex_session_hook(&InstallOptions {
            codex_home: dir.path().to_path_buf(),
            porter_bin: "/opt/agent-porter".into(),
            dry_run: false,
        });
        assert!(r.is_err());
    }

    #[test]
    fn refuses_unsafe_binary_path() {
        let dir = tempfile::tempdir().unwrap();
        // A path with a command substitution must be rejected, and nothing
        // must be written to hooks.json.
        let r = install_codex_session_hook(&InstallOptions {
            codex_home: dir.path().to_path_buf(),
            porter_bin: "/opt/agent-porter\"; $(rm -rf ~) #".into(),
            dry_run: false,
        });
        assert!(r.is_err());
        assert!(!dir.path().join("hooks.json").exists());
    }

    #[test]
    fn overwriting_keeps_a_backup_and_valid_json() {
        let dir = tempfile::tempdir().unwrap();
        // Seed an existing hooks.json with an unrelated hook.
        fs::write(
            dir.path().join("hooks.json"),
            r#"{"hooks":{"Stop":[{"hooks":[{"type":"command","command":"/usr/bin/x"}]}]}}"#,
        )
        .unwrap();
        install(dir.path(), "/opt/agent-porter");

        // A recoverable backup of the pre-write file exists...
        let bak = dir.path().join("hooks.json.bak");
        assert!(bak.exists(), "expected a .bak backup");
        assert!(fs::read_to_string(&bak).unwrap().contains("/usr/bin/x"));

        // ...and the live file is complete, valid JSON with both hooks.
        let text = fs::read_to_string(dir.path().join("hooks.json")).unwrap();
        let v: Value = serde_json::from_str(&text).expect("hooks.json must be valid JSON");
        assert!(v.get("hooks").and_then(|h| h.get("Stop")).is_some());
        assert!(v.get("hooks").and_then(|h| h.get("SessionStart")).is_some());
        // No stray temp file left behind.
        let strays: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(strays.is_empty(), "temp file left behind: {strays:?}");
    }

    #[test]
    fn safe_path_predicate() {
        assert!(is_safe_bin_path("/opt/agent-porter"));
        assert!(is_safe_bin_path(r"C:\Users\me\.cache\agent-porter.exe")); // backslashes OK
        assert!(!is_safe_bin_path("a$(b)"));
        assert!(!is_safe_bin_path("a`b`"));
        assert!(!is_safe_bin_path("a\"b"));
        assert!(!is_safe_bin_path(""));
    }
}
