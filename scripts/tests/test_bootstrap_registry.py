"""Tests for scripts/bootstrap-registry.py — the inverse of
generate-manifests.py (reconstruct registry/plugins.json from the generated
artefacts).

The critical property is the round-trip: a plugin that is claude_only (present
in the Claude catalog, absent from the Codex catalog) must be reconstructed
WITH claude_only=true, so that a subsequent `generate-manifests.py` reproduces
the same absence from the Codex catalog.

Both scripts resolve their paths relative to __file__, so we stage a
self-contained mini-repo in a tmpdir (copying both scripts into tmp/scripts)
and run them there — exactly the pattern test_new_plugin.py uses.
"""
from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_SCRIPTS = _HERE.parent

_NEEDED_SCRIPTS = ["bootstrap-registry.py", "generate-manifests.py"]

# One plugin lives in both catalogs; the other is claude_only (Claude catalog
# only). render_codex_catalog() skips claude_only plugins, so that is exactly
# how a claude_only plugin looks on disk after a real generate.
CLAUDE_CATALOG = {
    "name": "releng",
    "owner": {"name": "RelEng Team", "email": "releng@taboola.com"},
    "metadata": {
        "description": "RelEng plugins for Claude Code and Codex CLI — test fixture.",
        "version": "0.1.0",
        "pluginRoot": "./plugins",
    },
    "plugins": [
        {
            "name": "releng-both",
            "source": "./plugins/releng-both",
            "description": "A dual-target plugin present in both catalogs for the round-trip test.",
            "version": "0.1.0",
            "keywords": ["releng"],
            "category": "documentation",
        },
        {
            "name": "releng-claudeonly",
            "source": "./plugins/releng-claudeonly",
            "description": "A Claude-only plugin absent from the Codex catalog for the round-trip test.",
            "version": "0.1.0",
            "keywords": ["releng"],
            "category": "documentation",
        },
    ],
}

# Codex catalog omits releng-claudeonly (that is what claude_only means).
CODEX_CATALOG = {
    "name": "releng",
    "interface": {"displayName": "releng"},
    "plugins": [
        {
            "name": "releng-both",
            "source": {"source": "local", "path": "./plugins/releng-both"},
            "policy": {"installation": "AVAILABLE", "authentication": "ON_USE", "products": ["CODEX"]},
            "category": "Developer Tools",
        }
    ],
}


def _stage_repo(tmp_path: Path) -> Path:
    (tmp_path / "scripts").mkdir()
    for s in _NEEDED_SCRIPTS:
        dst = tmp_path / "scripts" / s
        shutil.copy(_SCRIPTS / s, dst)
        dst.chmod(0o755)

    (tmp_path / ".claude-plugin").mkdir()
    (tmp_path / ".claude-plugin" / "marketplace.json").write_text(
        json.dumps(CLAUDE_CATALOG, indent=2) + "\n"
    )
    (tmp_path / ".agents" / "plugins").mkdir(parents=True)
    (tmp_path / ".agents" / "plugins" / "marketplace.json").write_text(
        json.dumps(CODEX_CATALOG, indent=2) + "\n"
    )

    for entry in CLAUDE_CATALOG["plugins"]:
        name = entry["name"]
        pdir = tmp_path / "plugins" / name
        (pdir / ".claude-plugin").mkdir(parents=True)
        (pdir / ".claude-plugin" / "plugin.json").write_text(
            json.dumps(
                {"name": name, "description": entry["description"], "version": entry["version"]},
                indent=2,
            )
            + "\n"
        )
        (pdir / "OWNERS").write_text("releng\n")

    (tmp_path / "registry").mkdir()
    return tmp_path


def _run(root: Path, script: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(root / "scripts" / script)],
        capture_output=True,
        text=True,
        cwd=str(root),
    )


def test_bootstrap_marks_claude_only(tmp_path: Path):
    root = _stage_repo(tmp_path)
    proc = _run(root, "bootstrap-registry.py")
    assert proc.returncode == 0, proc.stdout + proc.stderr

    reg = json.loads((root / "registry" / "plugins.json").read_text())
    by_name = {p["name"]: p for p in reg["plugins"]}
    # The dual-target plugin is NOT flagged.
    assert "claude_only" not in by_name["releng-both"]
    # The Claude-only plugin is flagged.
    assert by_name["releng-claudeonly"]["claude_only"] is True


def test_bootstrap_generate_round_trip_reproduces_codex_absence(tmp_path: Path):
    """bootstrap → generate must reproduce the claude_only plugin's absence
    from the Codex catalog."""
    root = _stage_repo(tmp_path)

    boot = _run(root, "bootstrap-registry.py")
    assert boot.returncode == 0, boot.stdout + boot.stderr

    gen = _run(root, "generate-manifests.py")
    assert gen.returncode == 0, gen.stdout + gen.stderr

    codex = json.loads((root / ".agents" / "plugins" / "marketplace.json").read_text())
    names = [p["name"] for p in codex["plugins"]]
    assert names == ["releng-both"]
    assert "releng-claudeonly" not in names


# ── codex_only mirror: present in Codex catalog, absent from Claude catalog ──

def _stage_codex_only_repo(tmp_path: Path) -> Path:
    """Stage a repo whose sole plugin is codex_only: it appears in the Codex
    catalog + has a .codex-plugin/plugin.json, but is absent from the Claude
    catalog and has no .claude-plugin/plugin.json (exactly how generate emits a
    codex_only plugin)."""
    (tmp_path / "scripts").mkdir()
    for s in _NEEDED_SCRIPTS:
        dst = tmp_path / "scripts" / s
        shutil.copy(_SCRIPTS / s, dst)
        dst.chmod(0o755)

    claude_catalog = {
        "name": "releng",
        "owner": {"name": "RelEng Team", "email": "releng@taboola.com"},
        "metadata": {
            "description": "RelEng plugins for Claude Code and Codex CLI — test fixture.",
            "version": "0.1.0",
            "pluginRoot": "./plugins",
        },
        "plugins": [],
    }
    codex_catalog = {
        "name": "releng",
        "interface": {"displayName": "releng"},
        "plugins": [
            {
                "name": "releng-codexonly",
                "source": {"source": "local", "path": "./plugins/releng-codexonly"},
                "policy": {"installation": "AVAILABLE", "authentication": "ON_USE", "products": ["CODEX"]},
                "category": "Data & Analytics",
            }
        ],
    }
    (tmp_path / ".claude-plugin").mkdir()
    (tmp_path / ".claude-plugin" / "marketplace.json").write_text(json.dumps(claude_catalog, indent=2) + "\n")
    (tmp_path / ".agents" / "plugins").mkdir(parents=True)
    (tmp_path / ".agents" / "plugins" / "marketplace.json").write_text(json.dumps(codex_catalog, indent=2) + "\n")

    pdir = tmp_path / "plugins" / "releng-codexonly"
    (pdir / ".codex-plugin").mkdir(parents=True)
    (pdir / ".codex-plugin" / "plugin.json").write_text(
        json.dumps(
            {
                "name": "releng-codexonly",
                "version": "0.2.0",
                "description": "A Codex-only plugin absent from the Claude catalog for the round-trip test.",
                "skills": "./skills/",
                "keywords": ["releng", "analytics"],
                "interface": {"displayName": "releng-codexonly", "category": "Data & Analytics"},
            },
            indent=2,
        )
        + "\n"
    )
    (pdir / "OWNERS").write_text("releng\n")
    (tmp_path / "registry").mkdir()
    return tmp_path


def test_bootstrap_marks_codex_only(tmp_path: Path):
    root = _stage_codex_only_repo(tmp_path)
    proc = _run(root, "bootstrap-registry.py")
    assert proc.returncode == 0, proc.stdout + proc.stderr

    reg = json.loads((root / "registry" / "plugins.json").read_text())
    by_name = {p["name"]: p for p in reg["plugins"]}
    entry = by_name["releng-codexonly"]
    assert entry["codex_only"] is True
    assert entry["version"] == "0.2.0"          # recovered from .codex-plugin/plugin.json
    assert entry["category"] == "analytics"      # "Data & Analytics" reverse-maps cleanly
    assert entry["keywords"] == ["releng", "analytics"]


def test_bootstrap_generate_round_trip_reproduces_claude_absence(tmp_path: Path):
    """bootstrap → generate must reproduce the codex_only plugin's absence from
    the Claude catalog (and its presence in the Codex catalog)."""
    root = _stage_codex_only_repo(tmp_path)

    boot = _run(root, "bootstrap-registry.py")
    assert boot.returncode == 0, boot.stdout + boot.stderr

    gen = _run(root, "generate-manifests.py")
    assert gen.returncode == 0, gen.stdout + gen.stderr

    claude = json.loads((root / ".claude-plugin" / "marketplace.json").read_text())
    assert [p["name"] for p in claude["plugins"]] == []

    codex = json.loads((root / ".agents" / "plugins" / "marketplace.json").read_text())
    assert [p["name"] for p in codex["plugins"]] == ["releng-codexonly"]
