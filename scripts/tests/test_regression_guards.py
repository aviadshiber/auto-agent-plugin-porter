"""Regression guards for the PR #2 ensemble-review findings.

Each test maps to one verified finding (FIX-N) and is written to FAIL on the
pre-fix behavior. Two are behavioral (stage a mini-repo, run the real script);
three are source-assertions where a behavioral reproduction would be flaky or
environment-dependent (documented inline).

FIX-1 (YAML corruption) and FIX-2 (bootstrap claude_only round-trip) already have
dedicated behavioral coverage in test_generate_manifests.py / test_bootstrap_registry.py.
FIX-4/5/7 are covered in test_validate_registry.py / test_bump_registry_version.py /
test_validate_sh.py / test_new_plugin.py. This file adds guards for FIX-6/8/9/10
(FIX-3 retired with the move from Jenkins to GitHub Actions).
"""
from __future__ import annotations

import json
import re
import shutil
import subprocess
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_SCRIPTS = _HERE.parent
_REPO = _SCRIPTS.parent

_GOOD = "releng-good"

_REGISTRY = {
    "marketplace": {
        "name": "releng",
        "owner": {"name": "RelEng Team", "email": "releng@taboola.com"},
        "description": "RelEng plugins for Claude Code and Codex CLI — regression fixture.",
        "version": "0.1.0",
        "pluginRoot": "./plugins",
    },
    "plugins": [
        {
            "name": _GOOD,
            "version": "0.1.0",
            "description": "A valid fixture plugin used to exercise the validators end-to-end.",
            "category": "documentation",
            "keywords": ["releng"],
            "owners": ["releng"],
        }
    ],
}

_SKILL_MD = (
    "---\n"
    f"name: {_GOOD}\n"
    "description: A valid fixture plugin. This skill should be used when testing the validators.\n"
    "allowed-tools: Read, Grep, Glob\n"
    "---\n\n"
    f"# {_GOOD}\n\nFixture body.\n"
)


def _stage(tmp_path: Path, scripts: list[str]) -> Path:
    """Stage a self-consistent mini-repo with the given sibling scripts, one
    valid plugin, AGENTS.md/CLAUDE.md, and freshly generated manifests."""
    (tmp_path / "scripts").mkdir()
    # generate-manifests.py is always needed to produce the manifests the
    # validators read; add it if the caller didn't list it.
    for s in dict.fromkeys(scripts + ["generate-manifests.py"]):
        dst = tmp_path / "scripts" / s
        shutil.copy(_SCRIPTS / s, dst)
        dst.chmod(0o755)

    (tmp_path / "registry").mkdir()
    (tmp_path / "registry" / "plugins.json").write_text(json.dumps(_REGISTRY, indent=2) + "\n")
    shutil.copy(_REPO / "registry" / "schema.json", tmp_path / "registry" / "schema.json")

    (tmp_path / "AGENTS.md").write_text("# Contributor practices (fixture)\n")
    (tmp_path / "CLAUDE.md").write_text("@AGENTS.md\n")

    skill_dir = tmp_path / "plugins" / _GOOD / "skills" / _GOOD
    skill_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text(_SKILL_MD)

    gen = subprocess.run(
        ["python3", str(tmp_path / "scripts" / "generate-manifests.py")],
        capture_output=True, text=True, cwd=str(tmp_path),
    )
    assert gen.returncode == 0, gen.stdout + gen.stderr
    return tmp_path


# ── FIX-3 (Jenkins docker-agent label) retired: CI moved to GitHub Actions
#    (.github/workflows/), which has no Jenkins docker-agent label to guard. ──


# ── FIX-6 — validate.sh self-containment loop must be glob-safe ──
def test_fix6_validate_sh_ref_loop_is_glob_safe():
    # Behavioral reproduction requires a CWD file matching a `*` in a ref, which
    # is environment-dependent and flaky; guard the specific construct instead.
    txt = (_REPO / "scripts" / "validate.sh").read_text()
    # Ignore comment lines — validate.sh documents the avoided anti-pattern in a
    # comment, so a naive substring match would false-positive on that prose.
    code_lines = [ln for ln in txt.splitlines() if not ln.lstrip().startswith("#")]
    offenders = [ln for ln in code_lines if "for f in $refs" in ln]
    assert not offenders, (
        "unquoted `for f in $refs` reintroduced as CODE — undergoes pathname expansion on "
        f"refs containing shell glob metacharacters (FIX-6 regression): {offenders}"
    )
    assert re.search(r"while\s+IFS=\s*read -r f", txt), "glob-safe `while IFS= read -r f` loop missing (FIX-6)"


# ── FIX-8 — generate-manifests.py --check diff subprocess must be bounded ──
def test_fix8_generate_manifests_diff_has_timeout():
    txt = (_REPO / "scripts" / "generate-manifests.py").read_text()
    m = re.search(r"subprocess\.run\(\s*\[[^\]]*['\"]diff['\"][^\]]*\](.*?)\)", txt, re.S)
    assert m, "diff subprocess.run(...) call not found in generate-manifests.py"
    assert re.search(r"timeout\s*=", m.group(1)), (
        "the --check diff subprocess.run has no `timeout=` — a pathological diff could hang CI (FIX-8 regression)"
    )


# ── FIX-9 — validate-json.sh must catch a malformed Codex catalog ──
def test_fix9_validate_json_catches_malformed_codex_catalog(tmp_path: Path):
    root = _stage(tmp_path, ["validate-json.sh", "validate-registry.py"])

    clean = subprocess.run(
        [str(root / "scripts" / "validate-json.sh")],
        capture_output=True, text=True, cwd=str(root),
    )
    if clean.returncode != 0 and "jsonschema" in (clean.stdout + clean.stderr).lower():
        import pytest
        pytest.skip("jsonschema not available to the subprocess in this environment")
    assert clean.returncode == 0, "clean fixture should pass validate-json.sh:\n" + clean.stdout + clean.stderr

    # Corrupt the generated Codex catalog: drop the required policy.authentication.
    cx = root / ".agents" / "plugins" / "marketplace.json"
    d = json.loads(cx.read_text())
    del d["plugins"][0]["policy"]["authentication"]
    cx.write_text(json.dumps(d, indent=2) + "\n")

    bad = subprocess.run(
        [str(root / "scripts" / "validate-json.sh")],
        capture_output=True, text=True, cwd=str(root),
    )
    out = (bad.stdout + bad.stderr).lower()
    assert bad.returncode != 0, "validate-json.sh did not fail on a malformed Codex catalog (FIX-9 regression):\n" + bad.stdout + bad.stderr
    assert "authentication" in out or "codex" in out, "failure not attributable to the Codex catalog check:\n" + bad.stdout + bad.stderr


# ── FIX-10 — secrets scan must cover the whole repo, not just plugins/ ──
def test_fix10_secrets_scan_covers_whole_repo(tmp_path: Path):
    root = _stage(tmp_path, ["validate.sh", "check-instructions-sync.sh"])
    # validate.sh §8 scans `git ls-files`, so the fixture must be a git repo.
    subprocess.run(["git", "init", "-q"], cwd=str(root), check=True)
    subprocess.run(["git", "add", "-A"], cwd=str(root), check=True)

    clean = subprocess.run(
        [str(root / "scripts" / "validate.sh"), _GOOD],
        capture_output=True, text=True, cwd=str(root),
    )
    assert clean.returncode == 0, "clean fixture should pass validate.sh:\n" + clean.stdout + clean.stderr

    # Plant a secret in a repo-ROOT file (outside plugins/) and track it.
    (root / "leak.env").write_text("AWS_SECRET=AKIA" + "A" * 16 + "\n")
    subprocess.run(["git", "add", "leak.env"], cwd=str(root), check=True)

    bad = subprocess.run(
        [str(root / "scripts" / "validate.sh"), _GOOD],
        capture_output=True, text=True, cwd=str(root),
    )
    out = (bad.stdout + bad.stderr).lower()
    assert bad.returncode != 0, "secret in a root file (outside plugins/) was not caught (FIX-10 regression):\n" + bad.stdout + bad.stderr
    assert "secret" in out, "failure not attributable to the secrets scan:\n" + bad.stdout + bad.stderr
