//! Content hashing for the incremental-sync fast path.
//!
//! The source of truth for "did this skill change?" is a hash over the *entire*
//! skill directory (SKILL.md + references/ + scripts/ + assets/), not just
//! SKILL.md — a change to a referenced file must also trigger a re-port. The
//! hash is stored in the generated mirror's `metadata.source_hash`; on the next
//! run we recompute and compare, and skip the write when they match.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

/// Deterministic SHA-256 over every file under `dir`, keyed by forward-slash
/// relative path (so the digest is identical on Windows and Unix). Directories
/// and symlinks-to-dirs are recursed; file bytes are length-prefixed to avoid
/// boundary-collision ambiguity between adjacent files.
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
}
