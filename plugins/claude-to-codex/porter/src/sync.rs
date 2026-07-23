//! The one-way, hash-gated, loop-safe skill sync engine.

use crate::agent::Agent;
use crate::frontmatter;
use crate::hashing::hash_dir;
use serde_yaml::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Inputs to a single sync run (source → target).
pub struct SyncOptions {
    pub source: Agent,
    pub target: Agent,
    /// Source agent config dir (the one containing `skills/`).
    pub source_dir: PathBuf,
    /// Target agent config dir.
    pub target_dir: PathBuf,
    /// When true, compute the plan but write nothing.
    pub dry_run: bool,
    /// When true, remove target mirrors whose source skill no longer exists.
    pub prune: bool,
}

/// What a sync run did, bucketed. Per-skill failures land in `errors` and do
/// not abort the run — a session-start hook must be resilient to one bad skill.
#[derive(Default, Debug)]
pub struct SyncReport {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    /// Source was itself a porter-generated mirror → skipped (loop safety).
    pub skipped_source_is_mirror: Vec<String>,
    /// Target name exists but was not created by us → left untouched.
    pub skipped_target_conflict: Vec<String>,
    pub pruned: Vec<String>,
    pub errors: Vec<String>,
}

impl SyncReport {
    /// Number of artifacts that actually changed on disk.
    pub fn changed(&self) -> usize {
        self.created.len() + self.updated.len() + self.pruned.len()
    }

    /// One-line human summary for the session-start log.
    pub fn summary(&self, source: Agent, target: Agent) -> String {
        format!(
            "agent-porter {}→{}: {} created, {} updated, {} unchanged, {} pruned, {} skipped, {} errors",
            source.as_str(),
            target.as_str(),
            self.created.len(),
            self.updated.len(),
            self.unchanged.len(),
            self.pruned.len(),
            self.skipped_source_is_mirror.len() + self.skipped_target_conflict.len(),
            self.errors.len(),
        )
    }
}

/// Resolve `path` to an absolute, symlink-free form for containment checks,
/// tolerating a not-yet-existing tail: canonicalize the longest existing
/// prefix, then re-append the remaining components. Falls back to the input on
/// total failure (e.g. no existing ancestor).
fn resolve_for_containment(path: &Path) -> PathBuf {
    let mut acc: Vec<std::ffi::OsString> = Vec::new();
    let mut cur = path.to_path_buf();
    loop {
        if let Ok(c) = fs::canonicalize(&cur) {
            let mut out = c;
            for comp in acc.iter().rev() {
                out.push(comp);
            }
            return out;
        }
        match (cur.file_name(), cur.parent()) {
            (Some(name), Some(parent)) => {
                acc.push(name.to_os_string());
                cur = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// Run one source → target sync.
pub fn sync(opts: &SyncOptions) -> crate::Result<SyncReport> {
    let mut report = SyncReport::default();
    let src_skills = opts.source_dir.join("skills");
    let dst_skills = opts.target_dir.join("skills");

    if !src_skills.is_dir() {
        return Ok(report); // no source skills → nothing to port
    }

    // Refuse overlapping source/target trees BEFORE any write. If the target
    // skills dir were equal to, or nested inside, the source (or vice versa) —
    // reachable via misconfigured CLI/env overrides — write_mirror would create
    // a destination the copy walk then recurses into, self-copying until disk
    // or path limits are hit, and mutating the source tree in the process.
    let cs = resolve_for_containment(&src_skills);
    let cd = resolve_for_containment(&dst_skills);
    if cs == cd || cs.starts_with(&cd) || cd.starts_with(&cs) {
        return Err(format!(
            "source and target skills directories overlap (source={}, target={}); \
             they must be entirely separate",
            src_skills.display(),
            dst_skills.display()
        )
        .into());
    }

    let mut entries: Vec<PathBuf> = fs::read_dir(&src_skills)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();

    let mut generated: HashSet<String> = HashSet::new();
    for src_skill in &entries {
        let name = match dir_skill_name(src_skill) {
            Some(n) => n,
            None => continue,
        };
        if let Err(e) = port_one(
            opts,
            src_skill,
            &name,
            &dst_skills,
            &mut report,
            &mut generated,
        ) {
            report.errors.push(format!("{name}: {e}"));
        }
    }

    if opts.prune {
        if let Err(e) = prune(opts, &dst_skills, &src_skills, &mut report) {
            report.errors.push(format!("prune: {e}"));
        }
    }
    Ok(report)
}

fn port_one(
    opts: &SyncOptions,
    src_skill: &Path,
    name: &str,
    dst_skills: &Path,
    report: &mut SyncReport,
    generated: &mut HashSet<String>,
) -> crate::Result<()> {
    // Normalize CRLF → LF so a SKILL.md saved by a Windows editor still parses
    // (the fence detection is LF-based) and the mirror body is clean LF.
    let text = fs::read_to_string(src_skill.join("SKILL.md"))?.replace("\r\n", "\n");
    let (fm_str, body) = match frontmatter::split(&text) {
        Some(v) => v,
        None => {
            report
                .errors
                .push(format!("{name}: SKILL.md has no usable frontmatter"));
            return Ok(());
        }
    };
    let src_fm = frontmatter::parse_mapping(fm_str)?;

    // Loop safety: a skill we generated carries our marker — never re-port it.
    if frontmatter::is_ported(&src_fm) {
        report.skipped_source_is_mirror.push(name.to_string());
        return Ok(());
    }

    let description = frontmatter::get_str(&src_fm, "description")
        .unwrap_or_else(|| format!("Ported {} skill '{}'.", opts.source.as_str(), name));
    let implicit_allowed = compute_implicit_allowed(opts.source, &src_fm, src_skill);
    let allowed_tools = src_fm.get("allowed-tools").cloned();
    let source_hash = hash_dir(src_skill)?;

    let mirror_name = format!("{}{}", opts.source.prefix(), name);
    let dst_skill = dst_skills.join(&mirror_name);
    generated.insert(mirror_name.clone());

    if dst_skill.exists() {
        match existing_marker(&dst_skill) {
            // Target exists but is not ours → protect the user's own skill.
            None => {
                report.skipped_target_conflict.push(mirror_name);
                return Ok(());
            }
            // Unchanged only if BOTH the source content and the porter version
            // match — a porter upgrade that changes how mirrors are rendered
            // must force one re-render even when the source is byte-identical,
            // otherwise mirrors keep stale rendering / porter_version forever.
            Some(mk)
                if mk.source_hash == source_hash && mk.porter_version == crate::PORTER_VERSION =>
            {
                report.unchanged.push(mirror_name);
                return Ok(());
            }
            Some(_) => {
                if !opts.dry_run {
                    write_mirror(
                        opts,
                        src_skill,
                        body,
                        &description,
                        implicit_allowed,
                        allowed_tools,
                        name,
                        &source_hash,
                        &mirror_name,
                        &dst_skill,
                    )?;
                }
                report.updated.push(mirror_name);
            }
        }
    } else {
        if !opts.dry_run {
            write_mirror(
                opts,
                src_skill,
                body,
                &description,
                implicit_allowed,
                allowed_tools,
                name,
                &source_hash,
                &mirror_name,
                &dst_skill,
            )?;
        }
        report.created.push(mirror_name);
    }
    Ok(())
}

/// The porter marker of an existing target skill, or `None` when the target is
/// unreadable, has no frontmatter, or was not generated by this porter.
fn existing_marker(dst_skill: &Path) -> Option<frontmatter::Marker> {
    let text = fs::read_to_string(dst_skill.join("SKILL.md")).ok()?;
    let (fm_str, _) = frontmatter::split(&text)?;
    let fm = frontmatter::parse_mapping(fm_str).ok()?;
    let mk = frontmatter::marker(&fm)?;
    (mk.ported_by == crate::PORTER_ID).then_some(mk)
}

/// Normalize the cross-agent "may the model auto-invoke this?" policy from
/// whatever the source encodes it as.
fn compute_implicit_allowed(source: Agent, src_fm: &serde_yaml::Mapping, src_skill: &Path) -> bool {
    if frontmatter::get_bool(src_fm, "disable-model-invocation") == Some(true) {
        return false;
    }
    if source == Agent::Codex {
        let oy = src_skill.join("agents").join("openai.yaml");
        if let Ok(txt) = fs::read_to_string(&oy) {
            if let Ok(Value::Mapping(root)) = serde_yaml::from_str::<Value>(&txt) {
                if let Some(policy) = root.get("policy").and_then(Value::as_mapping) {
                    if policy
                        .get("allow_implicit_invocation")
                        .and_then(Value::as_bool)
                        == Some(false)
                    {
                        return false;
                    }
                }
            }
        }
    }
    true
}

#[allow(clippy::too_many_arguments)]
fn write_mirror(
    opts: &SyncOptions,
    src_skill: &Path,
    body: &str,
    description: &str,
    implicit_allowed: bool,
    allowed_tools: Option<Value>,
    source_name: &str,
    source_hash: &str,
    mirror_name: &str,
    dst_skill: &Path,
) -> crate::Result<()> {
    // Defense-in-depth (CWE-59): never follow a symlink — only replace a real
    // directory. A symlinked target could redirect the swap outside the tree.
    if dst_skill.exists() && fs::symlink_metadata(dst_skill)?.file_type().is_symlink() {
        return Err(format!(
            "refusing to replace symlinked target: {}",
            dst_skill.display()
        )
        .into());
    }

    // Transactional replace so a failure mid-write can never strand a corrupted
    // mirror (which a later run would misread as a user conflict or as
    // "unchanged"): build the COMPLETE mirror in a sibling staging dir, validate
    // its required outputs, then swap it into place while preserving the old
    // mirror until the rename commits. `.`-prefixed staging/backup names are
    // skipped by dir_skill_name and prune, so they are never treated as skills.
    let parent = dst_skill
        .parent()
        .ok_or_else(|| format!("mirror path has no parent: {}", dst_skill.display()))?;
    // process id makes the staging name unique across concurrent porter runs.
    let tag = format!(".{}.porter-staging.{}", mirror_name, std::process::id());
    let staging = parent.join(&tag);
    let backup = parent.join(format!(
        ".{}.porter-old.{}",
        mirror_name,
        std::process::id()
    ));

    // Best-effort cleanup of a leftover staging dir from a prior crash.
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging)?;

    // Build fully into staging; on ANY error, remove staging and bail with the
    // real mirror untouched.
    let build = (|| -> crate::Result<()> {
        copy_tree_except(src_skill, src_skill, &staging)?;
        let fm = frontmatter::build_mirror_frontmatter(
            mirror_name,
            description,
            opts.target,
            implicit_allowed,
            allowed_tools,
            opts.source,
            source_name,
            source_hash,
        );
        fs::write(staging.join("SKILL.md"), frontmatter::render(&fm, body)?)?;
        if opts.target == Agent::Codex {
            let agents_dir = staging.join("agents");
            fs::create_dir_all(&agents_dir)?;
            let oy = frontmatter::build_openai_yaml(mirror_name, description, implicit_allowed)?;
            fs::write(agents_dir.join("openai.yaml"), oy)?;
        }
        // Validate required outputs before we touch the live mirror.
        if !staging.join("SKILL.md").is_file() {
            return Err("staged mirror missing SKILL.md".into());
        }
        if opts.target == Agent::Codex && !staging.join("agents/openai.yaml").is_file() {
            return Err("staged Codex mirror missing agents/openai.yaml".into());
        }
        Ok(())
    })();
    if let Err(e) = build {
        let _ = fs::remove_dir_all(&staging);
        return Err(e);
    }

    // Commit: move any existing valid mirror aside, then rename staging into
    // place. rename(2) into a now-absent target is atomic on the same
    // filesystem. If the final rename fails, roll the old mirror back so we
    // never leave the target missing.
    let had_old = dst_skill.exists();
    if had_old {
        let _ = fs::remove_dir_all(&backup);
        fs::rename(dst_skill, &backup)?;
    }
    match fs::rename(&staging, dst_skill) {
        Ok(()) => {
            if had_old {
                let _ = fs::remove_dir_all(&backup); // commit succeeded
            }
            Ok(())
        }
        Err(e) => {
            if had_old {
                let _ = fs::rename(&backup, dst_skill); // roll back
            }
            let _ = fs::remove_dir_all(&staging);
            Err(format!("failed to install mirror {mirror_name}: {e}").into())
        }
    }
}

fn copy_tree_except(root: &Path, cur: &Path, dst_root: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(cur)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let rel = match path.strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_slash = rel_to_slash(rel);
        if rel_slash == "SKILL.md" || rel_slash == "agents/openai.yaml" {
            continue;
        }
        // Defense-in-depth: never descend into the destination itself. The
        // containment check in sync() already rejects overlapping trees, so
        // this only fires under a pathological layout — but it guarantees the
        // copy can never recurse into its own output.
        if path == dst_root {
            continue;
        }
        if file_type.is_dir() {
            copy_tree_except(root, &path, dst_root)?;
        } else if file_type.is_file() {
            let dst = dst_root.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(&path, &dst)?;
        }
    }
    Ok(())
}

fn prune(
    opts: &SyncOptions,
    dst_skills: &Path,
    src_skills: &Path,
    report: &mut SyncReport,
) -> crate::Result<()> {
    if !dst_skills.is_dir() {
        return Ok(());
    }
    let mut entries: Vec<PathBuf> = fs::read_dir(dst_skills)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .collect();
    entries.sort();

    for dir in entries {
        if !dir.is_dir() {
            continue;
        }
        let mk = match existing_marker(&dir) {
            Some(m) => m,
            None => continue, // not ours (or not a skill) → never prune
        };
        if mk.source_agent != opts.source.as_str() {
            continue; // ported from the other direction — not ours to prune
        }
        // Defense-in-depth (CWE-22): source_name comes from the mirror's own
        // (user-tamperable) frontmatter. It must be a single path component;
        // a value with separators or ".." could turn the existence probe below
        // into an arbitrary-path check. A legitimate marker never has these.
        if mk.source_name.is_empty()
            || mk.source_name.contains('/')
            || mk.source_name.contains('\\')
            || mk.source_name.contains("..")
        {
            continue;
        }
        let source_still_present = src_skills.join(&mk.source_name).join("SKILL.md").is_file();
        if !source_still_present {
            let display = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            if !opts.dry_run {
                fs::remove_dir_all(&dir)?;
            }
            report.pruned.push(display);
        }
    }
    Ok(())
}

/// `Some(name)` iff `path` is a non-hidden directory containing a `SKILL.md`.
fn dir_skill_name(path: &Path) -> Option<String> {
    if !path.is_dir() {
        return None;
    }
    let name = path.file_name()?.to_str()?.to_string();
    if name.starts_with('.') {
        return None;
    }
    if !path.join("SKILL.md").is_file() {
        return None;
    }
    Some(name)
}

fn rel_to_slash(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}
