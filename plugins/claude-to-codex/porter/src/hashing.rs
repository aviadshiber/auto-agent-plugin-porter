//! Content hashing for the incremental-sync fast path.
//!
//! `render_hash` covers the canonical generated SKILL.md inputs, optional Codex
//! metadata, and every copied file. It is the sole fast-path key: a referenced
//! file change triggers a re-port while source edits that cannot affect
//! generated output stay no-ops.

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Deterministic SHA-256 of the effective mirror inputs.
///
/// `skill_md_without_hash` is the canonical SKILL.md rendered with an empty
/// `render_hash`; `openai_yaml` is the exact optional Codex metadata output.
/// Files copied verbatim are hashed from `dir`; source `SKILL.md` and
/// `agents/openai.yaml` are excluded because the porter translates them rather
/// than copying them.
pub fn hash_rendered_mirror(
    dir: &Path,
    skill_md_without_hash: &str,
    openai_yaml: Option<&str>,
) -> std::io::Result<String> {
    let mut hasher = Sha256::new();
    hasher.update(b"agent-porter-render-plan-v2");
    hash_labeled(&mut hasher, b"SKILL.md", skill_md_without_hash.as_bytes());
    if let Some(metadata) = openai_yaml {
        hash_labeled(&mut hasher, b"agents/openai.yaml", metadata.as_bytes());
    }

    let mut rels = Vec::new();
    collect(dir, dir, &mut rels)?;
    let translated = translated_source_paths(dir)?;
    rels.retain(|rel| !translated.contains(rel));
    rels.sort();
    for rel in rels {
        let bytes = fs::read(dir.join(&rel))?;
        hash_labeled(&mut hasher, rel.as_bytes(), &bytes);
    }
    Ok(hex(&hasher.finalize()))
}

/// Resolve the actual directory-entry spellings of files the porter
/// translates. Exact canonical names win. A case variant is selected only when
/// the canonical path itself resolves on this filesystem (macOS/Windows), so a
/// distinct lowercase supplemental file remains copyable on Linux.
pub fn translated_source_paths(dir: &Path) -> std::io::Result<HashSet<String>> {
    let mut translated = HashSet::new();
    if let Some(path) = resolve_actual_relative(dir, &["SKILL.md"])? {
        if path.is_file() {
            translated.insert(rel_to_slash(path.strip_prefix(dir).unwrap_or(&path)));
        }
    }
    if let Some(path) = resolve_actual_relative(dir, &["agents", "openai.yaml"])? {
        if path.is_file() {
            translated.insert(rel_to_slash(path.strip_prefix(dir).unwrap_or(&path)));
        }
    }
    Ok(translated)
}

fn resolve_actual_relative(root: &Path, components: &[&str]) -> std::io::Result<Option<PathBuf>> {
    let mut current = root.to_path_buf();
    for desired in components {
        let mut folded_match = None;
        let mut exact_match = None;
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        for entry in entries {
            let entry = entry?;
            let name = entry.file_name();
            let text = name.to_string_lossy();
            if text == *desired {
                exact_match = Some(entry.path());
                break;
            }
            if folded_match.is_none() && text.eq_ignore_ascii_case(desired) {
                folded_match = Some(entry.path());
            }
        }
        current = if let Some(exact) = exact_match {
            exact
        } else if current.join(desired).exists() {
            let Some(folded) = folded_match else {
                return Ok(None);
            };
            folded
        } else {
            return Ok(None);
        };
    }
    Ok(Some(current))
}

fn hash_labeled(hasher: &mut Sha256, label: &[u8], bytes: &[u8]) {
    hasher.update((label.len() as u64).to_le_bytes());
    hasher.update(label);
    hasher.update((bytes.len() as u64).to_le_bytes());
    hasher.update(bytes);
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
    fn render_hash_tracks_effective_inputs_and_copied_files() {
        let td = tempfile::tempdir().unwrap();
        let root = td.path();
        fs::write(root.join("SKILL.md"), "ignored source frontmatter").unwrap();
        fs::write(root.join("reference.md"), "one").unwrap();

        let h1 = hash_rendered_mirror(root, "body\ncompact description", None).unwrap();
        assert_eq!(
            h1,
            hash_rendered_mirror(root, "body\ncompact description", None).unwrap()
        );
        assert_ne!(
            h1,
            hash_rendered_mirror(root, "body\ndifferent description", None).unwrap()
        );
        assert_ne!(
            h1,
            hash_rendered_mirror(root, "body\ncompact description", Some("policy: off\n")).unwrap()
        );

        // Raw SKILL.md is translated, so only its effective parts above matter.
        fs::write(root.join("SKILL.md"), "different ignored frontmatter").unwrap();
        assert_eq!(
            h1,
            hash_rendered_mirror(root, "body\ncompact description", None).unwrap()
        );

        fs::write(root.join("reference.md"), "two").unwrap();
        assert_ne!(
            h1,
            hash_rendered_mirror(root, "body\ncompact description", None).unwrap()
        );
    }

    #[test]
    fn lowercase_manifest_follows_filesystem_case_semantics() {
        let td = tempfile::tempdir().unwrap();
        fs::write(td.path().join("skill.md"), "source one").unwrap();
        let canonical_resolves = td.path().join("SKILL.md").is_file();
        let before = hash_rendered_mirror(td.path(), "generated", None).unwrap();
        fs::write(td.path().join("skill.md"), "source two").unwrap();
        let after = hash_rendered_mirror(td.path(), "generated", None).unwrap();
        if canonical_resolves {
            assert_eq!(before, after);
        } else {
            assert_ne!(before, after);
        }
    }

    #[test]
    fn distinct_case_variant_is_preserved_on_case_sensitive_filesystems() {
        let td = tempfile::tempdir().unwrap();
        fs::write(td.path().join("SKILL.md"), "canonical").unwrap();
        fs::write(td.path().join("skill.md"), "supplemental").unwrap();
        let variants = fs::read_dir(td.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case("skill.md")
            })
            .count();
        if variants == 2 {
            let before = hash_rendered_mirror(td.path(), "generated", None).unwrap();
            fs::write(td.path().join("skill.md"), "changed supplemental").unwrap();
            assert_ne!(
                before,
                hash_rendered_mirror(td.path(), "generated", None).unwrap()
            );
        }
    }
}
