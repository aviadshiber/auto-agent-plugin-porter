//! Content hashing for the incremental-sync fast path.
//!
//! `source_hash` records the entire raw source tree for diagnostics.
//! `render_hash` is the incremental fast-path key: it covers the translated
//! SKILL.md inputs and every copied file, so a referenced-file change triggers
//! a re-port while source edits that cannot affect generated output stay no-ops.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Deterministic diagnostic SHA-256 over every file under `dir`, keyed by
/// forward-slash relative path (so the digest is identical on Windows and
/// Unix). Directories and symlinks-to-dirs are recursed; file bytes are
/// length-prefixed to avoid boundary-collision ambiguity between adjacent
/// files.
pub fn hash_dir(dir: &Path) -> std::io::Result<String> {
    let mut rels: Vec<String> = Vec::new();
    collect(dir, dir, &mut rels)?;
    rels.sort();

    let mut hasher = Sha256::new();
    for rel in &rels {
        hasher.update(rel.as_bytes());
        hasher.update([0u8]);
        let bytes = fs::read(dir.join(rel))?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hex(&hasher.finalize()))
}

/// Deterministic SHA-256 of the effective mirror inputs.
///
/// `parts` contains the translated SKILL.md inputs (body, budgeted description,
/// policy, identity). Files copied verbatim are hashed from `dir`; source
/// `SKILL.md` and `agents/openai.yaml` are excluded because the porter
/// translates them rather than copying them. This lets a source edit that is
/// outside a truncated description's visible prefix remain a true no-op while
/// still detecting every change that affects generated output.
pub fn hash_render_plan(dir: &Path, parts: &[&str]) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"agent-porter-render-plan-v1");
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }

    let mut rels = Vec::new();
    collect(dir, dir, &mut rels)?;
    rels.retain(|rel| rel != "SKILL.md" && rel != "agents/openai.yaml");
    rels.sort();
    for rel in rels {
        hasher.update((rel.len() as u64).to_le_bytes());
        hasher.update(rel.as_bytes());
        let bytes = fs::read(dir.join(&rel))?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    Ok(hex(&hasher.finalize()))
}

fn collect(root: &Path, cur: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in fs::read_dir(cur)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect(root, &path, out)?;
        } else if file_type.is_file() {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel_to_slash(rel));
            }
        }
        // Symlinks and other node types are intentionally ignored — skills are
        // plain file trees; following symlinks would risk escaping the dir.
    }
    Ok(())
}

fn rel_to_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn hash_is_stable_and_content_sensitive() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join("SKILL.md"), "hello").unwrap();
        fs::create_dir(root.join("references")).unwrap();
        fs::write(root.join("references/a.md"), "ref").unwrap();

        let h1 = hash_dir(root).unwrap();
        let h2 = hash_dir(root).unwrap();
        assert_eq!(h1, h2, "hash must be deterministic");
        assert_eq!(h1.len(), 64, "sha-256 hex is 64 chars");

        // Change a referenced file → hash must change.
        fs::write(root.join("references/a.md"), "ref2").unwrap();
        assert_ne!(h1, hash_dir(root).unwrap());
    }

    #[test]
    fn render_hash_tracks_effective_inputs_and_copied_files() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path();
        fs::write(root.join("SKILL.md"), "ignored source frontmatter").unwrap();
        fs::write(root.join("reference.md"), "one").unwrap();

        let h1 = hash_render_plan(root, &["body", "compact description"]).unwrap();
        assert_eq!(
            h1,
            hash_render_plan(root, &["body", "compact description"]).unwrap()
        );
        assert_ne!(
            h1,
            hash_render_plan(root, &["body", "different description"]).unwrap()
        );

        // Raw SKILL.md is translated, so only its effective parts above matter.
        fs::write(root.join("SKILL.md"), "different ignored frontmatter").unwrap();
        assert_eq!(
            h1,
            hash_render_plan(root, &["body", "compact description"]).unwrap()
        );

        fs::write(root.join("reference.md"), "two").unwrap();
        assert_ne!(
            h1,
            hash_render_plan(root, &["body", "compact description"]).unwrap()
        );
    }
}
