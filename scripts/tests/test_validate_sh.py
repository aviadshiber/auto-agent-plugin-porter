"""Focused end-to-end test for scripts/validate.sh.

Stages a self-consistent mini-repo in a tmpdir and runs validate.sh against a
VALID plugin (expects rc 0) and a deliberately BROKEN plugin — one missing its
SKILL.md (expects non-zero).

Why the mini-repo must be self-consistent: validate.sh sections 12
(check-instructions-sync.sh) and 13 (generate-manifests.py --check) run over
the WHOLE repo regardless of the plugin argument. So the fixture stages
AGENTS.md + CLAUDE.md and pre-generates the manifests from the registry, and
the broken plugin is left OUT of the registry — that keeps §13 drift-free while
§6 (SKILL.md exists, which loops the plugin dirs) carries the broken signal.
"""
from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_SCRIPTS = _HERE.parent
_REPO = _SCRIPTS.parent

# validate.sh shells out to these two siblings (§12 and §13).
_NEEDED_SCRIPTS = ["validate.sh", "check-instructions-sync.sh", "generate-manifests.py"]

_GOOD = "releng-good"
_BROKEN = "releng-broken"

REGISTRY = {
    "marketplace": {
        "name": "releng",
        "owner": {"name": "RelEng Team", "email": "releng@taboola.com"},
        "description": "RelEng plugins for Claude Code and Codex CLI — validate.sh test fixture.",
        "version": "0.1.0",
        "pluginRoot": "./plugins",
    },
    "plugins": [
        {
            "name": _GOOD,
            "version": "0.1.0",
            "description": "A valid fixture plugin used to exercise validate.sh end-to-end.",
            "category": "documentation",
            "keywords": ["releng"],
            "owners": ["releng"],
        }
    ],
}

SKILL_MD = (
    "---\n"
    f"name: {_GOOD}\n"
    "description: A valid fixture plugin. This skill should be used when testing validate.sh.\n"
    "allowed-tools: Read, Grep, Glob\n"
    "---\n"
    "\n"
    f"# {_GOOD}\n"
    "\n"
    "Fixture body.\n"
)


def _stage_repo(tmp_path: Path) -> Path:
    (tmp_path / "scripts").mkdir()
    for s in _NEEDED_SCRIPTS:
        dst = tmp_path / "scripts" / s
        shutil.copy(_SCRIPTS / s, dst)
        dst.chmod(0o755)

    (tmp_path / "registry").mkdir()
    (tmp_path / "registry" / "plugins.json").write_text(json.dumps(REGISTRY, indent=2) + "\n")
    shutil.copy(_REPO / "registry" / "schema.json", tmp_path / "registry" / "schema.json")

    # §12 needs a canonical AGENTS.md and a CLAUDE.md that imports it.
    (tmp_path / "AGENTS.md").write_text("# Contributor practices (fixture)\n")
    (tmp_path / "CLAUDE.md").write_text("@AGENTS.md\n")

    # Valid plugin with a real SKILL.md.
    skill_dir = tmp_path / "plugins" / _GOOD / "skills" / _GOOD
    skill_dir.mkdir(parents=True)
    (skill_dir / "SKILL.md").write_text(SKILL_MD)

    # Pre-generate manifests so §13 (generate --check) is drift-free.
    gen = subprocess.run(
        ["python3", str(tmp_path / "scripts" / "generate-manifests.py")],
        capture_output=True, text=True, cwd=str(tmp_path),
    )
    assert gen.returncode == 0, gen.stdout + gen.stderr
    return tmp_path


def _run_validate(root: Path, plugin: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(root / "scripts" / "validate.sh"), plugin],
        capture_output=True, text=True, cwd=str(root),
    )


def test_validate_sh_passes_for_valid_plugin(tmp_path: Path):
    root = _stage_repo(tmp_path)
    proc = _run_validate(root, _GOOD)
    assert proc.returncode == 0, proc.stdout + proc.stderr
    assert "Validation PASSED" in proc.stdout


def test_validate_sh_fails_for_broken_plugin(tmp_path: Path):
    root = _stage_repo(tmp_path)
    # Broken plugin: a plugin dir with NO SKILL.md, deliberately absent from
    # the registry (so §13 stays drift-free and §6 is the failing signal).
    (root / "plugins" / _BROKEN).mkdir()
    proc = _run_validate(root, _BROKEN)
    assert proc.returncode != 0, proc.stdout + proc.stderr
    # Failing for the RIGHT reason: the missing SKILL.md.
    assert "SKILL.md" in proc.stdout
