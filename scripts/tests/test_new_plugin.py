"""Tests for scripts/new-plugin.sh — the plugin scaffolder.

Stages a minimal self-contained repo in a tmpdir (just the scripts the
scaffolder invokes + an empty registry + the schema), runs new-plugin.sh,
and asserts the scaffolded tree, registry append, generated manifests, and
idempotency. Runs the scaffolder end-to-end (it shells out to
generate-manifests.py + validate-json.sh), so this also smoke-tests those.
"""
from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_SCRIPTS = _HERE.parent
_REPO = _SCRIPTS.parent

# The scaffolder invokes these siblings; stage them all into the tmp repo.
_NEEDED_SCRIPTS = [
    "new-plugin.sh",
    "_append_to_registry.py",
    "generate-manifests.py",
    "validate-registry.py",
    "validate-json.sh",
]

EMPTY_REGISTRY = {
    "marketplace": {
        "name": "releng",
        "owner": {"name": "RelEng Team", "email": "releng@taboola.com"},
        "description": "RelEng plugins for Claude Code and Codex CLI — test fixture.",
        "version": "0.1.0",
        "pluginRoot": "./plugins",
    },
    "plugins": [],
}


def _stage_repo(tmp_path: Path) -> Path:
    (tmp_path / "scripts").mkdir()
    for s in _NEEDED_SCRIPTS:
        dst = tmp_path / "scripts" / s
        shutil.copy(_SCRIPTS / s, dst)
        dst.chmod(0o755)
    (tmp_path / "registry").mkdir()
    shutil.copy(_REPO / "registry" / "schema.json", tmp_path / "registry" / "schema.json")
    (tmp_path / "registry" / "plugins.json").write_text(json.dumps(EMPTY_REGISTRY, indent=2) + "\n")
    (tmp_path / "plugins").mkdir()
    return tmp_path


def _run_new_plugin(root: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(root / "scripts" / "new-plugin.sh"), *args],
        capture_output=True,
        text=True,
        cwd=str(root),
    )


def test_new_plugin_scaffolds_everything(tmp_path: Path):
    root = _stage_repo(tmp_path)
    proc = _run_new_plugin(
        root, "releng-testplug",
        "--category", "documentation",
        "--description", "A scaffolded plugin used in tests. Use this when testing the scaffolder.",
    )
    assert proc.returncode == 0, proc.stdout + proc.stderr

    # Scaffolded files exist.
    pdir = root / "plugins" / "releng-testplug"
    assert (pdir / "skills" / "releng-testplug" / "SKILL.md").is_file()
    assert (pdir / "OWNERS").is_file()
    assert (pdir / "references" / ".gitkeep").is_file()

    # Registry got the entry.
    reg = json.loads((root / "registry" / "plugins.json").read_text())
    names = [p["name"] for p in reg["plugins"]]
    assert names == ["releng-testplug"]
    entry = reg["plugins"][0]
    assert entry["category"] == "documentation"
    assert entry["version"] == "0.1.0"

    # Manifests were generated for both targets.
    assert (root / ".claude-plugin" / "marketplace.json").is_file()
    assert (root / ".agents" / "plugins" / "marketplace.json").is_file()
    assert (pdir / ".claude-plugin" / "plugin.json").is_file()
    codex_manifest = json.loads((pdir / ".codex-plugin" / "plugin.json").read_text())
    assert codex_manifest["skills"] == "./skills/"
    assert codex_manifest["interface"]["category"] == "Developer Tools"

    # SKILL.md was stamped with dual compatibility.
    skill = (pdir / "skills" / "releng-testplug" / "SKILL.md").read_text()
    assert "compatibility:" in skill
    assert "codex-cli" in skill

    # Codex catalog carries the verified policy.
    codex_catalog = json.loads((root / ".agents" / "plugins" / "marketplace.json").read_text())
    pol = codex_catalog["plugins"][0]["policy"]
    assert pol["authentication"] == "ON_USE"
    assert pol["products"] == ["CODEX"]


def test_new_plugin_is_idempotent(tmp_path: Path):
    root = _stage_repo(tmp_path)
    first = _run_new_plugin(root, "releng-testplug", "--description", "First run creates the plugin here.")
    assert first.returncode == 0, first.stdout + first.stderr
    reg_after_first = (root / "registry" / "plugins.json").read_text()

    second = _run_new_plugin(root, "releng-testplug", "--description", "Second run must be a no-op.")
    assert second.returncode == 0, second.stdout + second.stderr
    assert "already exists" in second.stdout
    # Registry unchanged — no duplicate entry.
    assert (root / "registry" / "plugins.json").read_text() == reg_after_first


def test_new_plugin_rejects_bad_name(tmp_path: Path):
    root = _stage_repo(tmp_path)
    proc = _run_new_plugin(root, "Bad_Name")
    assert proc.returncode == 1
    assert "Invalid plugin name" in proc.stdout + proc.stderr
    # Nothing scaffolded.
    assert not (root / "plugins" / "Bad_Name").exists()


def test_new_plugin_rejects_traversal_name(tmp_path: Path):
    """A path-traversal name must be rejected by the name regex before any
    files are written — the slash and dots fail ^[a-z][a-z0-9-]*$."""
    root = _stage_repo(tmp_path)
    proc = _run_new_plugin(root, "../../etc")
    assert proc.returncode == 1
    assert "Invalid plugin name" in proc.stdout + proc.stderr
    # No traversal escape: nothing created outside the repo.
    assert not (root.parent / "etc").exists()


def test_new_plugin_rejects_bad_category(tmp_path: Path):
    """An unknown --category fails fast (before scaffolding) and lists the
    valid categories."""
    root = _stage_repo(tmp_path)
    proc = _run_new_plugin(
        root, "releng-testplug",
        "--category", "not-a-category",
        "--description", "A valid-length description used to isolate the category check.",
    )
    assert proc.returncode == 1
    combined = proc.stdout + proc.stderr
    assert "Invalid category" in combined
    assert "documentation" in combined  # the valid list is surfaced
    # Nothing scaffolded, registry untouched.
    assert not (root / "plugins" / "releng-testplug").exists()
    reg = json.loads((root / "registry" / "plugins.json").read_text())
    assert reg["plugins"] == []
