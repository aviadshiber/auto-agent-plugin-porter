"""Tests for scripts/_is_claude_only.py — exit-code-as-boolean helper.
Verifies the contract:
  0 = plugin exists & claude_only is True
  1 = plugin exists & claude_only is False/missing — OR plugin not found
  2 = required env var missing (helpful error, distinct from 1)
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import pytest

_HERE = Path(__file__).resolve().parent
_SCRIPT = _HERE.parent / "_is_claude_only.py"


def _run(env: dict) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(_SCRIPT)],
        env=env,
        capture_output=True,
        text=True,
    )


def _make_registry(tmp_path: Path, plugins: list) -> Path:
    reg = tmp_path / "registry.json"
    reg.write_text(json.dumps({"plugins": plugins}))
    return reg


@pytest.mark.parametrize(
    "plugin_entry, expected_code",
    [
        ({"name": "foo", "claude_only": True}, 0),
        ({"name": "foo", "claude_only": False}, 1),
        ({"name": "foo"}, 1),  # missing field
    ],
)
def test_is_claude_only_for_listed_plugin(tmp_path, plugin_entry, expected_code):
    reg = _make_registry(tmp_path, [plugin_entry])
    proc = _run({"PATH": "/usr/bin:/bin", "REGISTRY": str(reg), "PLUGIN_NAME": "foo"})
    assert proc.returncode == expected_code, proc.stderr


def test_unknown_plugin_returns_1(tmp_path):
    reg = _make_registry(tmp_path, [{"name": "foo", "claude_only": True}])
    proc = _run({"PATH": "/usr/bin:/bin", "REGISTRY": str(reg), "PLUGIN_NAME": "bar"})
    assert proc.returncode == 1


@pytest.mark.parametrize("missing", ["REGISTRY", "PLUGIN_NAME"])
def test_missing_env_var_returns_2(tmp_path, missing):
    reg = _make_registry(tmp_path, [])
    env = {"PATH": "/usr/bin:/bin", "REGISTRY": str(reg), "PLUGIN_NAME": "foo"}
    env.pop(missing)
    proc = _run(env)
    assert proc.returncode == 2
    assert missing in proc.stderr
