"""Tests for scripts/validate-registry.py — covers all three exit codes.

Run via subprocess so the contributor-facing stderr messages are exercised.
"""
from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

_HERE = Path(__file__).resolve().parent
_REPO = _HERE.parent.parent
_SCRIPT = _REPO / "scripts" / "validate-registry.py"


def _run(env: dict | None = None) -> subprocess.CompletedProcess:
    full_env = os.environ.copy()
    if env:
        full_env.update(env)
    return subprocess.run(
        [sys.executable, str(_SCRIPT)],
        env=full_env,
        capture_output=True,
        text=True,
    )


@pytest.fixture
def valid_registry_present():
    proc = _run()
    if proc.returncode == 2:
        pytest.skip("jsonschema not installed in this environment")
    assert proc.returncode == 0, f"real registry/plugins.json does not validate: {proc.stderr}"


@pytest.mark.usefixtures("valid_registry_present")
def test_real_registry_validates() -> None:
    pass  # the fixture is the assertion


def test_jsonschema_missing_returns_2(tmp_path: Path):
    """Simulate jsonschema being absent by running with a PYTHONPATH that
    contains a fake `jsonschema` package which raises ImportError."""
    fake = tmp_path / "fake"
    fake.mkdir()
    (fake / "jsonschema.py").write_text("raise ImportError('simulated for test')\n")
    proc = _run({"PYTHONPATH": str(fake)})
    assert proc.returncode == 2
    assert "jsonschema not installed" in proc.stderr


def test_invalid_registry_returns_1(tmp_path: Path):
    """Copy the real registry + schema into a tmpdir, write an invalid
    registry there (missing required fields), and validate the COPY via the
    REGISTRY/SCHEMA env overrides — the real registry/plugins.json is never
    touched."""
    schema_copy = tmp_path / "schema.json"
    registry_copy = tmp_path / "plugins.json"
    schema_copy.write_text((_REPO / "registry" / "schema.json").read_text())
    bad = {
        "marketplace": {
            "name": "releng",
            "version": "0.1.0",
            "description": "RelEng plugins for Claude Code and Codex CLI.",
            "owner": {"name": "X", "email": "x@y.com"},
            "repository": "https://example.com",
        },
        "plugins": [
            {  # missing "version", "description", "category", "owners"
                "name": "broken",
                "keywords": ["x"],
            }
        ],
    }
    registry_copy.write_text(json.dumps(bad, indent=2) + "\n")
    proc = _run({"REGISTRY": str(registry_copy), "SCHEMA": str(schema_copy)})
    if proc.returncode == 2:
        pytest.skip("jsonschema not installed in this environment")
    assert proc.returncode == 1, f"expected exit 1, got {proc.returncode}: {proc.stderr}"
    assert "ERROR" in proc.stderr


def test_schema_rejects_both_only_flags(tmp_path: Path):
    """The registry schema's mutual-exclusion constraint must reject a plugin
    that sets BOTH claude_only and codex_only (a plugin targeting no agent).
    This guards the schema itself, independent of the generator's runtime
    _check_exclusivity."""
    schema_copy = tmp_path / "schema.json"
    registry_copy = tmp_path / "plugins.json"
    schema_copy.write_text((_REPO / "registry" / "schema.json").read_text())
    bad = {
        "marketplace": {
            "name": "releng",
            "version": "0.1.0",
            "description": "RelEng plugins for Claude Code and Codex CLI.",
            "owner": {"name": "X", "email": "x@y.com"},
            "pluginRoot": "./plugins",
        },
        "plugins": [
            {
                "name": "impossible",
                "version": "0.1.0",
                "description": "A plugin that wrongly targets neither agent.",
                "category": "development",
                "keywords": ["x"],
                "owners": ["x"],
                "claude_only": True,
                "codex_only": True,
            }
        ],
    }
    registry_copy.write_text(json.dumps(bad, indent=2) + "\n")
    proc = _run({"REGISTRY": str(registry_copy), "SCHEMA": str(schema_copy)})
    if proc.returncode == 2:
        pytest.skip("jsonschema not installed in this environment")
    assert proc.returncode == 1, f"schema must reject both-only: {proc.stdout}{proc.stderr}"
