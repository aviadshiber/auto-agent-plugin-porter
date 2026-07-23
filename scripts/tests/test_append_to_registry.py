"""Tests for scripts/_append_to_registry.py — invoked via subprocess so the
exit codes and stderr messages contributors actually see are exercised."""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

_HERE = Path(__file__).resolve().parent
_SCRIPT = _HERE.parent / "_append_to_registry.py"

EMPTY_REGISTRY = {
    "marketplace": {
        "name": "releng",
        "owner": {"name": "RelEng Team", "email": "releng@taboola.com"},
        "version": "0.1.0",
        "description": "RelEng plugins for Claude Code and Codex CLI.",
        "pluginRoot": "./plugins",
    },
    "plugins": [],
}


def _write_registry(path: Path, data: dict) -> None:
    path.write_text(json.dumps(data, indent=2) + "\n")


def _run(env: dict) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(_SCRIPT)],
        env=env,
        capture_output=True,
        text=True,
    )


def test_append_adds_new_plugin(tmp_path: Path):
    reg_path = tmp_path / "registry.json"
    _write_registry(reg_path, EMPTY_REGISTRY)

    proc = _run({
        "PATH": "/usr/bin:/bin",
        "REGISTRY": str(reg_path),
        "SKILL_NAME": "releng-foo",
        "DESCRIPTION": "A new plugin used in tests.",
        "SKILL_OWNER": "alice",
    })
    assert proc.returncode == 0, proc.stderr

    data = json.loads(reg_path.read_text())
    assert len(data["plugins"]) == 1
    p = data["plugins"][0]
    assert p["name"] == "releng-foo"
    assert p["version"] == "0.1.0"
    assert p["description"] == "A new plugin used in tests."
    # Defaults must be schema-valid (minItems: 1 on keywords & owners).
    assert p["keywords"] == ["releng-foo"]
    assert p["owners"] == ["alice"]
    assert p["category"] == "documentation"


def test_append_honors_category_override(tmp_path: Path):
    reg_path = tmp_path / "registry.json"
    _write_registry(reg_path, EMPTY_REGISTRY)

    proc = _run({
        "PATH": "/usr/bin:/bin",
        "REGISTRY": str(reg_path),
        "SKILL_NAME": "releng-bar",
        "DESCRIPTION": "Another plugin used in tests.",
        "SKILL_OWNER": "alice",
        "CATEGORY": "devops",
    })
    assert proc.returncode == 0, proc.stderr
    data = json.loads(reg_path.read_text())
    assert data["plugins"][0]["category"] == "devops"


def test_append_is_idempotent(tmp_path: Path):
    """Re-running with the same name is a no-op (exit 0, registry unchanged)."""
    reg_path = tmp_path / "registry.json"
    seeded = json.loads(json.dumps(EMPTY_REGISTRY))
    seeded["plugins"].append({
        "name": "releng-foo",
        "version": "9.9.9",
        "description": "already here, do not touch",
        "category": "documentation",
        "keywords": ["foo"],
        "owners": ["bob"],
    })
    _write_registry(reg_path, seeded)
    before = reg_path.read_text()

    proc = _run({
        "PATH": "/usr/bin:/bin",
        "REGISTRY": str(reg_path),
        "SKILL_NAME": "releng-foo",
        "DESCRIPTION": "would-be replacement",
        "SKILL_OWNER": "alice",
    })
    assert proc.returncode == 0, proc.stderr
    assert reg_path.read_text() == before, "registry must not be mutated on duplicate"


@pytest.mark.parametrize("missing_var", ["REGISTRY", "SKILL_NAME", "DESCRIPTION", "SKILL_OWNER"])
def test_append_requires_env(tmp_path: Path, missing_var: str):
    reg_path = tmp_path / "registry.json"
    _write_registry(reg_path, EMPTY_REGISTRY)
    env = {
        "PATH": "/usr/bin:/bin",
        "REGISTRY": str(reg_path),
        "SKILL_NAME": "releng-foo",
        "DESCRIPTION": "ok",
        "SKILL_OWNER": "alice",
    }
    env.pop(missing_var)
    proc = _run(env)
    assert proc.returncode == 1
    assert missing_var in proc.stderr


@pytest.mark.parametrize("bad_name", ["Foo", "9foo", "../etc", "foo bar", ""])
def test_append_rejects_invalid_name(tmp_path: Path, bad_name: str):
    reg_path = tmp_path / "registry.json"
    _write_registry(reg_path, EMPTY_REGISTRY)
    env = {
        "PATH": "/usr/bin:/bin",
        "REGISTRY": str(reg_path),
        "SKILL_NAME": bad_name,
        "DESCRIPTION": "x",
        "SKILL_OWNER": "alice",
    }
    proc = _run(env)
    assert proc.returncode == 1
    data = json.loads(reg_path.read_text())
    assert data["plugins"] == []
