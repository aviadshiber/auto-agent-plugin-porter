//! The one-way, hash-gated, loop-safe skill sync engine.

use crate::agent::Agent;
use crate::frontmatter;
use crate::hashing::{hash_rendered_mirror, translated_source_paths};
use serde_yaml::Value;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Default soft pressure target for generated Codex description characters.
///
/// This is deliberately not presented as Codex's real budget: Codex applies a
/// dynamic 2%-of-context token budget to the complete skill catalog (including
/// names, paths, native skills, and plugins). The porter cannot observe the
/// active model context at session-hook time, so it uses a configurable,
/// best-effort description target to reduce pressure predictably.
const DEFAULT_CODEX_DESCRIPTION_TARGET_CHARS: usize = 8_000;
const CODEX_DESCRIPTION_TARGET_ENV: &str = "AGENT_PORTER_CODEX_DESCRIPTION_TARGET_CHARS";

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
    /// Present when Claude→Codex descriptions exceeded the porter's
    /// corpus-wide budget and were compacted for model-visible discovery.
    pub description_compaction: Option<DescriptionCompaction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptionCompaction {
    pub shortened_count: usize,
    pub original_chars: usize,
    pub rendered_chars: usize,
    pub target_chars: usize,
    pub retained_chars: usize,
    pub written_count: usize,
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

    /// User-facing notice for a sync that wrote budgeted descriptions. No-op
    /// session starts stay quiet even though the stored descriptions remain
    /// compacted.
    pub fn description_compaction_notice(&self) -> Option<String> {
        let compaction = self.description_compaction.as_ref()?;
        (compaction.written_count > 0).then(|| {
            format!(
                "agent-porter: warning: {} synced Codex mirror(s) use compacted descriptions; \
                 the current implicit-discovery corpus has {} shortened descriptions \
                 ({} to {} characters; soft target: {}; retained malformed-mirror \
                 characters: {})",
                compaction.written_count,
                compaction.shortened_count,
                compaction.original_chars,
                compaction.rendered_chars,
                compaction.target_chars,
                compaction.retained_chars,
            )
        })
    }
}

struct ExistingMirror {
    marker: frontmatter::Marker,
    description_chars: usize,
}

struct SourceSkill {
    source_path: PathBuf,
    source_name: String,
    mirror_name: String,
    destination_path: PathBuf,
    body: String,
    description: String,
    implicit_allowed: bool,
    allowed_tools: Option<Value>,
    existing: Option<ExistingMirror>,
    description_compacted: bool,
}

/// Single source of truth for every output-affecting mirror field.
struct MirrorRenderPlan {
    source_path: PathBuf,
    source_name: String,
    source_agent: Agent,
    target_agent: Agent,
    mirror_name: String,
    destination_path: PathBuf,
    body: String,
    description: String,
    implicit_allowed: bool,
    allowed_tools: Option<Value>,
    existing_marker: Option<frontmatter::Marker>,
    description_compacted: bool,
}

struct RenderedMirror {
    render_hash: String,
    skill_md: String,
    openai_yaml: Option<String>,
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

    let mut entries: Vec<(PathBuf, String)> = fs::read_dir(&src_skills)?
        .filter_map(|e| e.ok())
        .filter_map(|entry| {
            let path = entry.path();
            dir_skill_name(&path).map(|name| (path, name))
        })
        .collect();
    entries.sort_by(|left, right| left.0.cmp(&right.0));
    let present_destinations: HashSet<PathBuf> = entries
        .iter()
        .map(|(_, name)| dst_skills.join(format!("{}{}", opts.source.prefix(), name)))
        .collect();
    let (mut sources, retained_chars) =
        load_source_skills(opts, &entries, &dst_skills, &mut report);
    report.description_compaction =
        apply_codex_description_budget(opts, &mut sources, retained_chars);

    for source in sources {
        let name = source.source_name.clone();
        let plan = MirrorRenderPlan::from_source(opts, source);
        if let Err(e) = execute_plan(opts, &plan, &mut report) {
            report.errors.push(format!("{name}: {e}"));
        }
    }

    if opts.prune {
        if let Err(e) = prune(
            opts,
            &dst_skills,
            &src_skills,
            &present_destinations,
            &mut report,
        ) {
            report.errors.push(format!("prune: {e}"));
        }
    }
    Ok(report)
}

/// Parse each source and its existing target at most once. Invalid sources are
/// reported without aborting the session; if a porter-owned mirror already
/// exists, its current description is reserved in the pressure budget because
/// prune intentionally retains it while the source SKILL.md still exists.
fn load_source_skills(
    opts: &SyncOptions,
    entries: &[(PathBuf, String)],
    dst_skills: &Path,
    report: &mut SyncReport,
) -> (Vec<SourceSkill>, usize) {
    let mut sources = Vec::new();
    let mut retained_chars = 0usize;
    for (src_skill, name) in entries {
        let mirror_name = format!("{}{}", opts.source.prefix(), name);
        let destination_path = dst_skills.join(&mirror_name);
        let existing = existing_mirror(&destination_path);

        let text = match fs::read_to_string(src_skill.join("SKILL.md")) {
            Ok(text) => text.replace("\r\n", "\n"),
            Err(e) => {
                report.errors.push(format!("{name}: {e}"));
                retained_chars += existing
                    .as_ref()
                    .map_or(0, |mirror| mirror.description_chars);
                continue;
            }
        };
        let Some((fm_str, body)) = frontmatter::split(&text) else {
            report
                .errors
                .push(format!("{name}: SKILL.md has no usable frontmatter"));
            retained_chars += existing
                .as_ref()
                .map_or(0, |mirror| mirror.description_chars);
            continue;
        };
        let src_fm = match frontmatter::parse_mapping(fm_str) {
            Ok(frontmatter) => frontmatter,
            Err(e) => {
                report.errors.push(format!("{name}: {e}"));
                retained_chars += existing
                    .as_ref()
                    .map_or(0, |mirror| mirror.description_chars);
                continue;
            }
        };

        if frontmatter::is_ported(&src_fm) {
            report.skipped_source_is_mirror.push(name.clone());
            continue;
        }

        if destination_path.exists() && existing.is_none() {
            report.skipped_target_conflict.push(mirror_name);
            continue;
        }

        let raw_description = frontmatter::get_str(&src_fm, "description")
            .filter(|description| !description.trim().is_empty())
            .unwrap_or_else(|| format!("Ported {} skill '{}'.", opts.source.as_str(), name));
        let description = if opts.target == Agent::Codex {
            normalize_description(&raw_description)
        } else {
            raw_description
        };

        sources.push(SourceSkill {
            source_path: src_skill.clone(),
            source_name: name.clone(),
            mirror_name,
            destination_path,
            body: body.to_string(),
            description,
            implicit_allowed: compute_implicit_allowed(opts.source, &src_fm, src_skill),
            allowed_tools: src_fm.get("allowed-tools").cloned(),
            existing,
            description_compacted: false,
        });
    }
    (sources, retained_chars)
}

fn apply_codex_description_budget(
    opts: &SyncOptions,
    sources: &mut [SourceSkill],
    retained_chars: usize,
) -> Option<DescriptionCompaction> {
    if opts.target != Agent::Codex {
        return None;
    }

    let target_chars = codex_description_target();
    let available_chars = target_chars.saturating_sub(retained_chars);
    let visible_indices: Vec<usize> = sources
        .iter()
        .enumerate()
        .filter_map(|(index, source)| source.implicit_allowed.then_some(index))
        .collect();
    let original: Vec<String> = visible_indices
        .iter()
        .map(|index| sources[*index].description.clone())
        .collect();
    let allocated = allocate_description_budget(&original, available_chars);

    let shortened_count = original
        .iter()
        .zip(&allocated)
        .filter(|(before, after)| after.chars().count() < before.chars().count())
        .count();
    let original_chars = retained_chars
        + original
            .iter()
            .map(|description| description.chars().count())
            .sum::<usize>();
    let rendered_chars = retained_chars
        + allocated
            .iter()
            .map(|description| description.chars().count())
            .sum::<usize>();

    for ((index, before), description) in visible_indices.iter().zip(&original).zip(allocated) {
        let source = &mut sources[*index];
        source.description_compacted = description.chars().count() < before.chars().count();
        source.description = description;
    }

    (shortened_count > 0).then_some(DescriptionCompaction {
        shortened_count,
        original_chars,
        rendered_chars,
        target_chars,
        retained_chars,
        written_count: 0,
    })
}

fn codex_description_target() -> usize {
    std::env::var(CODEX_DESCRIPTION_TARGET_ENV)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_CODEX_DESCRIPTION_TARGET_CHARS)
}

impl MirrorRenderPlan {
    fn from_source(opts: &SyncOptions, source: SourceSkill) -> Self {
        Self {
            source_path: source.source_path,
            source_name: source.source_name,
            source_agent: opts.source,
            target_agent: opts.target,
            mirror_name: source.mirror_name,
            destination_path: source.destination_path,
            body: source.body,
            description: source.description,
            implicit_allowed: source.implicit_allowed,
            allowed_tools: source.allowed_tools,
            existing_marker: source.existing.map(|existing| existing.marker),
            description_compacted: source.description_compacted,
        }
    }

    fn skill_md(&self, render_hash: &str) -> crate::Result<String> {
        let fm = frontmatter::build_mirror_frontmatter(&frontmatter::MirrorFrontmatter {
            mirror_name: &self.mirror_name,
            description: &self.description,
            target: self.target_agent,
            implicit_allowed: self.implicit_allowed,
            allowed_tools: self.allowed_tools.as_ref(),
            source_agent: self.source_agent,
            source_name: &self.source_name,
            render_hash,
        });
        frontmatter::render(&fm, &self.body)
    }

    fn openai_yaml(&self) -> crate::Result<Option<String>> {
        if self.target_agent == Agent::Codex && !self.implicit_allowed {
            return Ok(Some(frontmatter::build_openai_yaml(
                &self.mirror_name,
                &self.description,
            )?));
        }
        Ok(None)
    }

    fn render(&self) -> crate::Result<RenderedMirror> {
        let openai_yaml = self.openai_yaml()?;
        let unhashed_skill = self.skill_md("")?;
        let render_hash =
            hash_rendered_mirror(&self.source_path, &unhashed_skill, openai_yaml.as_deref())?;
        let skill_md = self.skill_md(&render_hash)?;
        Ok(RenderedMirror {
            render_hash,
            skill_md,
            openai_yaml,
        })
    }
}

fn execute_plan(
    opts: &SyncOptions,
    plan: &MirrorRenderPlan,
    report: &mut SyncReport,
) -> crate::Result<()> {
    let rendered = plan.render()?;
    if let Some(existing) = &plan.existing_marker {
        if existing.render_hash == rendered.render_hash
            && existing.porter_version == crate::PORTER_VERSION
        {
            report.unchanged.push(plan.mirror_name.clone());
            return Ok(());
        }
        if !opts.dry_run {
            write_mirror(plan, &rendered)?;
            record_compacted_write(plan, report);
        }
        report.updated.push(plan.mirror_name.clone());
    } else {
        if !opts.dry_run {
            write_mirror(plan, &rendered)?;
            record_compacted_write(plan, report);
        }
        report.created.push(plan.mirror_name.clone());
    }
    Ok(())
}

fn record_compacted_write(plan: &MirrorRenderPlan, report: &mut SyncReport) {
    if plan.description_compacted {
        if let Some(compaction) = &mut report.description_compaction {
            compaction.written_count += 1;
        }
    }
}

fn normalize_description(description: &str) -> String {
    description.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Deterministic water-filling allocation: short descriptions keep their full
/// length and return unused share to longer descriptions. The target is soft:
/// when there are more descriptions than available characters, retain one
/// model-visible character per skill instead of making descriptions empty.
fn allocate_description_budget(descriptions: &[String], budget: usize) -> Vec<String> {
    if descriptions.is_empty() {
        return Vec::new();
    }

    let char_counts: Vec<usize> = descriptions
        .iter()
        .map(|description| description.chars().count())
        .collect();
    let total: usize = char_counts.iter().sum();
    if total <= budget {
        return descriptions.to_vec();
    }

    let target = budget.max(descriptions.len()).min(total);
    let max_len = char_counts.iter().copied().max().unwrap_or(0);
    let mut low = 0usize;
    let mut high = max_len;
    while low < high {
        let mid = low + (high - low).div_ceil(2);
        let used: usize = char_counts.iter().map(|count| (*count).min(mid)).sum();
        if used <= target {
            low = mid;
        } else {
            high = mid - 1;
        }
    }

    let mut allocations: Vec<usize> = char_counts.iter().map(|count| (*count).min(low)).collect();
    let mut remaining = target - allocations.iter().sum::<usize>();
    for (allocation, count) in allocations.iter_mut().zip(&char_counts) {
        if remaining == 0 {
            break;
        }
        if *allocation < *count {
            *allocation += 1;
            remaining -= 1;
        }
    }

    descriptions
        .iter()
        .zip(allocations)
        .map(|(description, allocation)| truncate_description(description, allocation))
        .collect()
}

fn truncate_description(description: &str, max_chars: usize) -> String {
    let count = description.chars().count();
    if count <= max_chars {
        return description.to_string();
    }
    if max_chars <= 1 {
        return "…".to_string();
    }
    let prefix: String = description.chars().take(max_chars - 1).collect();
    format!("{prefix}…")
}

/// Read an existing porter-owned mirror once, including the description size
/// needed when an invalid source forces us to retain that mirror.
fn existing_mirror(dst_skill: &Path) -> Option<ExistingMirror> {
    let text = fs::read_to_string(dst_skill.join("SKILL.md")).ok()?;
    let (fm_str, _) = frontmatter::split(&text)?;
    let fm = frontmatter::parse_mapping(fm_str).ok()?;
    let mk = frontmatter::marker(&fm)?;
    let description_chars = if frontmatter::get_bool(&fm, "disable-model-invocation") == Some(true)
        || codex_openai_disables_implicit(dst_skill)
    {
        0
    } else {
        frontmatter::get_str(&fm, "description")
            .unwrap_or_default()
            .chars()
            .count()
    };
    (mk.ported_by == crate::PORTER_ID).then_some(ExistingMirror {
        marker: mk,
        description_chars,
    })
}

fn existing_marker(dst_skill: &Path) -> Option<frontmatter::Marker> {
    existing_mirror(dst_skill).map(|mirror| mirror.marker)
}

/// Normalize the cross-agent "may the model auto-invoke this?" policy from
/// whatever the source encodes it as.
fn compute_implicit_allowed(source: Agent, src_fm: &serde_yaml::Mapping, src_skill: &Path) -> bool {
    if frontmatter::get_bool(src_fm, "disable-model-invocation") == Some(true) {
        return false;
    }
    if source == Agent::Codex && codex_openai_disables_implicit(src_skill) {
        return false;
    }
    true
}

fn codex_openai_disables_implicit(skill: &Path) -> bool {
    let oy = skill.join("agents").join("openai.yaml");
    let Ok(txt) = fs::read_to_string(oy) else {
        return false;
    };
    let Ok(Value::Mapping(root)) = serde_yaml::from_str::<Value>(&txt) else {
        return false;
    };
    root.get("policy")
        .and_then(Value::as_mapping)
        .and_then(|policy| policy.get("allow_implicit_invocation"))
        .and_then(Value::as_bool)
        == Some(false)
}

fn write_mirror(plan: &MirrorRenderPlan, rendered: &RenderedMirror) -> crate::Result<()> {
    let dst_skill = &plan.destination_path;
    let mirror_name = &plan.mirror_name;
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
        copy_tree_except(&plan.source_path, &plan.source_path, &staging)?;
        fs::write(staging.join("SKILL.md"), &rendered.skill_md)?;
        // `agents/openai.yaml` is optional in Codex. Emit it only when it
        // carries non-default behavior; writing one for every ordinary skill
        // doubles the manifest files Codex opens during a hot reload and can
        // exhaust a low process descriptor limit on a large skill corpus.
        if let Some(openai_yaml) = &rendered.openai_yaml {
            let agents_dir = staging.join("agents");
            fs::create_dir_all(&agents_dir)?;
            fs::write(agents_dir.join("openai.yaml"), openai_yaml)?;
        }
        // Validate required outputs before we touch the live mirror.
        if !staging.join("SKILL.md").is_file() {
            return Err("staged mirror missing SKILL.md".into());
        }
        let has_openai_yaml = staging.join("agents/openai.yaml").is_file();
        if has_openai_yaml != rendered.openai_yaml.is_some() {
            return Err("staged Codex policy metadata does not match render plan".into());
        }
        // The source tree can change between the initial render hash and this
        // copy. Verify the exact staged snapshot before committing so a mirror
        // can never carry copied bytes that disagree with its marker.
        verify_staged_snapshot(plan, rendered, &staging)?;
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

fn verify_staged_snapshot(
    plan: &MirrorRenderPlan,
    rendered: &RenderedMirror,
    staging: &Path,
) -> crate::Result<()> {
    let staged_hash = hash_rendered_mirror(
        staging,
        &plan.skill_md("")?,
        rendered.openai_yaml.as_deref(),
    )?;
    if staged_hash != rendered.render_hash {
        return Err("source changed while the mirror was being staged; retry sync".into());
    }
    Ok(())
}

fn copy_tree_except(root: &Path, cur: &Path, dst_root: &Path) -> std::io::Result<()> {
    let translated = translated_source_paths(root)?;
    copy_tree_except_paths(root, cur, dst_root, &translated)
}

fn copy_tree_except_paths(
    root: &Path,
    cur: &Path,
    dst_root: &Path,
    translated: &HashSet<String>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(cur)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        let rel = match path.strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        let rel_slash = rel_to_slash(rel);
        if translated.contains(&rel_slash) {
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
            copy_tree_except_paths(root, &path, dst_root, translated)?;
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
    present_destinations: &HashSet<PathBuf>,
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
        // Source discovery already proved this mirror's source SKILL.md is
        // present, so it cannot be pruned. Avoid reopening and reparsing target
        // frontmatter that load_source_skills already inspected.
        if present_destinations.contains(&dir) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocator_splits_equal_descriptions_exactly() {
        let descriptions = vec!["abcdefghij".to_string(), "klmnopqrst".to_string()];
        let allocated = allocate_description_budget(&descriptions, 8);
        assert_eq!(allocated, vec!["abc…", "klm…"]);
        assert_eq!(
            allocated.iter().map(|s| s.chars().count()).sum::<usize>(),
            8
        );
    }

    #[test]
    fn allocator_redistributes_unused_share() {
        let descriptions = vec!["a".to_string(), "bcdefghijk".to_string()];
        let allocated = allocate_description_budget(&descriptions, 6);
        assert_eq!(allocated, vec!["a", "bcde…"]);
        assert_eq!(
            allocated.iter().map(|s| s.chars().count()).sum::<usize>(),
            6
        );
    }

    #[test]
    fn allocator_counts_unicode_characters() {
        let descriptions = vec!["😀אבגד".to_string(), "éèêëē".to_string()];
        let allocated = allocate_description_budget(&descriptions, 6);
        assert_eq!(allocated, vec!["😀א…", "éè…"]);
        assert_eq!(
            allocated.iter().map(|s| s.chars().count()).sum::<usize>(),
            6
        );
    }

    #[test]
    fn allocator_treats_target_as_soft_when_skill_count_exceeds_it() {
        let descriptions = vec![
            "alpha".to_string(),
            "bravo".to_string(),
            "charlie".to_string(),
        ];
        let allocated = allocate_description_budget(&descriptions, 2);
        assert_eq!(allocated, vec!["…", "…", "…"]);
    }

    #[test]
    fn staged_snapshot_must_match_render_hash() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("reference.md"), "original").unwrap();
        let destination = tempfile::tempdir().unwrap();
        let plan = MirrorRenderPlan {
            source_path: source.path().to_path_buf(),
            source_name: "source".to_string(),
            source_agent: Agent::Claude,
            target_agent: Agent::Codex,
            mirror_name: "claude-source".to_string(),
            destination_path: destination.path().join("claude-source"),
            body: "body\n".to_string(),
            description: "description".to_string(),
            implicit_allowed: true,
            allowed_tools: None,
            existing_marker: None,
            description_compacted: false,
        };
        let rendered = plan.render().unwrap();
        let staging = tempfile::tempdir().unwrap();
        copy_tree_except(source.path(), source.path(), staging.path()).unwrap();
        assert!(verify_staged_snapshot(&plan, &rendered, staging.path()).is_ok());

        fs::write(staging.path().join("reference.md"), "changed during copy").unwrap();
        let error = verify_staged_snapshot(&plan, &rendered, staging.path()).unwrap_err();
        assert!(error.to_string().contains("source changed"));
    }

    #[test]
    fn copier_follows_filesystem_case_semantics_for_translated_files() {
        let source = tempfile::tempdir().unwrap();
        fs::write(source.path().join("skill.md"), "source manifest").unwrap();
        fs::create_dir_all(source.path().join("agents")).unwrap();
        fs::write(source.path().join("agents/OpenAI.yaml"), "source metadata").unwrap();
        fs::write(source.path().join("reference.md"), "copied").unwrap();
        let skill_is_manifest = source.path().join("SKILL.md").is_file();
        let metadata_is_translated = source.path().join("agents/openai.yaml").is_file();
        let destination = tempfile::tempdir().unwrap();

        copy_tree_except(source.path(), source.path(), destination.path()).unwrap();
        assert_eq!(
            destination.path().join("skill.md").exists(),
            !skill_is_manifest
        );
        assert_eq!(
            destination.path().join("agents/OpenAI.yaml").exists(),
            !metadata_is_translated
        );
        assert_eq!(
            fs::read_to_string(destination.path().join("reference.md")).unwrap(),
            "copied"
        );
    }
}
