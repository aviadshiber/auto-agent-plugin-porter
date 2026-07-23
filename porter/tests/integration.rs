//! End-to-end sync tests over real temp directories.

use agent_porter::agent::Agent;
use agent_porter::sync::{sync, SyncOptions};
use std::fs;
use std::path::{Path, PathBuf};

/// Create a source skill `<config>/skills/<name>/SKILL.md` (+ optional extra
/// files) and return the config dir.
fn make_skill(config: &Path, name: &str, frontmatter: &str, body: &str, extra: &[(&str, &str)]) {
    let dir = config.join("skills").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\n{frontmatter}\n---\n{body}"),
    )
    .unwrap();
    for (rel, content) in extra {
        let p = dir.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }
}

fn opts(source: Agent, target: Agent, sdir: &Path, tdir: &Path) -> SyncOptions {
    SyncOptions {
        source,
        target,
        source_dir: sdir.to_path_buf(),
        target_dir: tdir.to_path_buf(),
        dry_run: false,
        prune: true,
    }
}

fn read(p: PathBuf) -> String {
    fs::read_to_string(p).unwrap()
}

#[test]
fn codex_to_claude_creates_prefixed_mirror_with_marker() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    make_skill(
        src.path(),
        "artifacts",
        "name: artifacts\ndescription: Build dashboards",
        "# Artifacts\n\nBody here.\n",
        &[("references/guide.md", "detailed guide")],
    );

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.created, vec!["codex-artifacts".to_string()]);

    let mirror = dst.path().join("skills/codex-artifacts");
    let skill = read(mirror.join("SKILL.md"));
    assert!(skill.contains("name: codex-artifacts"));
    assert!(skill.contains("ported_by: auto-agent-plugin-porter"));
    assert!(skill.contains("source_agent: codex"));
    assert!(skill.contains("source_name: artifacts"));
    assert!(
        skill.contains("# Artifacts"),
        "body must be copied verbatim"
    );
    // Referenced files are copied too.
    assert_eq!(read(mirror.join("references/guide.md")), "detailed guide");
    // Codex→Claude must NOT emit an openai.yaml.
    assert!(!mirror.join("agents/openai.yaml").exists());
}

#[test]
fn second_run_is_unchanged_then_update_on_source_change() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    make_skill(
        src.path(),
        "foo",
        "name: foo\ndescription: d",
        "body\n",
        &[],
    );

    let r1 = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(r1.created.len(), 1);

    let r2 = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(r2.unchanged, vec!["codex-foo".to_string()]);
    assert!(r2.created.is_empty() && r2.updated.is_empty());

    // Change the source → next run updates.
    fs::write(
        src.path().join("skills/foo/SKILL.md"),
        "---\nname: foo\ndescription: d2\n---\nnew body\n",
    )
    .unwrap();
    let r3 = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(r3.updated, vec!["codex-foo".to_string()]);
    assert!(read(dst.path().join("skills/codex-foo/SKILL.md")).contains("new body"));
}

#[test]
fn loop_safety_skips_source_that_is_itself_a_mirror() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    // A "skill" in Codex that was itself ported there by the other direction.
    make_skill(
        src.path(),
        "claude-bar",
        "name: claude-bar\ndescription: d\nmetadata:\n  ported_by: auto-agent-plugin-porter\n  source_agent: claude\n  source_name: bar\n  source_hash: deadbeef",
        "body\n",
        &[],
    );

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(
        report.skipped_source_is_mirror,
        vec!["claude-bar".to_string()]
    );
    assert!(report.created.is_empty());
    assert!(
        !dst.path().join("skills/codex-claude-bar").exists(),
        "must not double-port"
    );
}

#[test]
fn non_clobber_leaves_user_authored_target_untouched() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    make_skill(
        src.path(),
        "foo",
        "name: foo\ndescription: d",
        "ported body\n",
        &[],
    );
    // A pre-existing, hand-authored target skill occupying the mirror name.
    make_skill(
        dst.path(),
        "codex-foo",
        "name: codex-foo\ndescription: mine",
        "user body — do not touch\n",
        &[],
    );

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(
        report.skipped_target_conflict,
        vec!["codex-foo".to_string()]
    );
    assert!(read(dst.path().join("skills/codex-foo/SKILL.md")).contains("do not touch"));
}

#[test]
fn prune_removes_mirror_when_source_deleted() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    make_skill(
        src.path(),
        "foo",
        "name: foo\ndescription: d",
        "body\n",
        &[],
    );
    sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert!(dst.path().join("skills/codex-foo").exists());

    // Delete the source skill, sync again → mirror is pruned.
    fs::remove_dir_all(src.path().join("skills/foo")).unwrap();
    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.pruned, vec!["codex-foo".to_string()]);
    assert!(!dst.path().join("skills/codex-foo").exists());
}

#[test]
fn claude_to_codex_emits_openai_yaml_with_policy() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    make_skill(
        src.path(),
        "manual",
        "name: manual\ndescription: A manual-only skill\ndisable-model-invocation: true",
        "body\n",
        &[],
    );

    let report = sync(&opts(Agent::Claude, Agent::Codex, src.path(), dst.path())).unwrap();
    assert_eq!(report.created, vec!["claude-manual".to_string()]);

    let mirror = dst.path().join("skills/claude-manual");
    let oy = read(mirror.join("agents/openai.yaml"));
    // disable-model-invocation: true → allow_implicit_invocation: false
    assert!(oy.contains("allow_implicit_invocation: false"));
    assert!(oy.contains("display_name: claude-manual"));
}

#[test]
fn missing_source_skills_dir_is_a_noop() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.changed(), 0);
    assert!(report.errors.is_empty());
}

#[test]
fn codex_openai_yaml_policy_translates_to_claude_disable() {
    // Reverse of claude_to_codex_emits_openai_yaml_with_policy: a Codex source
    // whose agents/openai.yaml disables implicit invocation must produce a
    // Claude mirror with disable-model-invocation: true.
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    make_skill(
        src.path(),
        "manual",
        "name: manual\ndescription: A manual-only Codex skill",
        "body\n",
        &[(
            "agents/openai.yaml",
            "interface:\n  display_name: Manual\npolicy:\n  allow_implicit_invocation: false\n",
        )],
    );

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.created, vec!["codex-manual".to_string()]);
    let skill = read(dst.path().join("skills/codex-manual/SKILL.md"));
    assert!(skill.contains("disable-model-invocation: true"));
    // Codex→Claude never emits an openai.yaml in the mirror.
    assert!(!dst
        .path()
        .join("skills/codex-manual/agents/openai.yaml")
        .exists());
}

#[test]
fn prune_only_touches_its_own_direction() {
    // A single Claude skills dir holds BOTH a codex-* mirror (from codex→claude)
    // and a claude-* mirror (the other direction's artifact). Pruning the
    // codex→claude direction with an empty Codex source must remove the codex-*
    // mirror but leave the claude-* mirror (different source_agent) untouched.
    let src = tempfile::tempdir().unwrap(); // empty Codex source
    let dst = tempfile::tempdir().unwrap(); // Claude target
    std::fs::create_dir_all(src.path().join("skills")).unwrap();

    // Our own codex→claude mirror (should be prunable).
    make_skill(
        dst.path(),
        "codex-gone",
        "name: codex-gone\ndescription: d\nmetadata:\n  ported_by: auto-agent-plugin-porter\n  source_agent: codex\n  source_name: gone\n  source_hash: x",
        "b\n",
        &[],
    );
    // The other direction's mirror living in the same dir (must survive).
    make_skill(
        dst.path(),
        "claude-keep",
        "name: claude-keep\ndescription: d\nmetadata:\n  ported_by: auto-agent-plugin-porter\n  source_agent: claude\n  source_name: keep\n  source_hash: y",
        "b\n",
        &[],
    );

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.pruned, vec!["codex-gone".to_string()]);
    assert!(!dst.path().join("skills/codex-gone").exists());
    assert!(
        dst.path().join("skills/claude-keep").exists(),
        "other direction's mirror must survive"
    );
}

#[test]
fn porter_version_change_forces_rerender() {
    // A mirror whose source is byte-identical but whose recorded porter_version
    // is stale must be re-rendered (so upgrades never leave stale mirrors).
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    make_skill(
        src.path(),
        "foo",
        "name: foo\ndescription: d",
        "body\n",
        &[],
    );
    sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();

    // Rewrite only the porter_version line in the mirror to an old value.
    let p = dst.path().join("skills/codex-foo/SKILL.md");
    let stale = read(p.clone()).replace(
        &format!("porter_version: {}", env!("CARGO_PKG_VERSION")),
        "porter_version: 0.0.0",
    );
    std::fs::write(&p, stale).unwrap();

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.updated, vec!["codex-foo".to_string()]);
    assert!(read(p).contains(&format!("porter_version: {}", env!("CARGO_PKG_VERSION"))));
}

#[test]
fn crlf_frontmatter_is_ported() {
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let dir = src.path().join("skills").join("winskill");
    std::fs::create_dir_all(&dir).unwrap();
    // CRLF line endings, as a Windows editor would save.
    std::fs::write(
        dir.join("SKILL.md"),
        "---\r\nname: winskill\r\ndescription: saved with CRLF\r\n---\r\n# Body\r\nhi\r\n",
    )
    .unwrap();

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.created, vec!["codex-winskill".to_string()]);
    let skill = read(dst.path().join("skills/codex-winskill/SKILL.md"));
    assert!(skill.contains("name: codex-winskill"));
    assert!(skill.contains("# Body"));
}

#[test]
fn skill_without_frontmatter_is_reported_not_aborted() {
    // A bad skill must be recorded as an error but NOT abort the whole run —
    // a good sibling skill still ports.
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let bad = src.path().join("skills").join("bad");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("SKILL.md"), "no frontmatter here\n").unwrap();
    make_skill(
        src.path(),
        "good",
        "name: good\ndescription: d",
        "body\n",
        &[],
    );

    let report = sync(&opts(Agent::Codex, Agent::Claude, src.path(), dst.path())).unwrap();
    assert_eq!(report.created, vec!["codex-good".to_string()]);
    assert!(report.errors.iter().any(|e| e.contains("bad")));
    assert!(dst.path().join("skills/codex-good").exists());
}

#[test]
fn cli_rejects_same_source_and_target() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_agent-porter"))
        .args(["sync", "--source", "claude", "--target", "claude"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("different agents"), "stderr was: {stderr}");
}
