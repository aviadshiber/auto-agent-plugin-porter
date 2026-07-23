"""Tests for scripts/_bump_registry_version.py — pure semver-bump logic
(imported) plus subprocess-level coverage of main()'s exit codes and the
on-disk registry mutation."""
from __future__ import annotations

import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

_HERE = Path(__file__).resolve().parent
_TARGET = _HERE.parent / "_bump_registry_version.py"

# The helper has a leading underscore so it cannot be imported by name.
spec = importlib.util.spec_from_file_location("_bump_registry_version", _TARGET)
assert spec and spec.loader
_mod = importlib.util.module_from_spec(spec)
spec.loader.exec_module(_mod)
bump_semver = _mod.bump_semver


@pytest.mark.parametrize(
    "version,kind,expected",
    [
        ("0.0.0", "patch", "0.0.1"),
        ("0.0.0", "minor", "0.1.0"),
        ("0.0.0", "major", "1.0.0"),
        ("1.2.3", "patch", "1.2.4"),
        ("1.2.3", "minor", "1.3.0"),
        ("1.2.3", "major", "2.0.0"),
        ("9.9.9", "patch", "9.9.10"),
        ("9.9.9", "minor", "9.10.0"),
        ("9.9.9", "major", "10.0.0"),
    ],
)
def test_bump_semver_happy_path(version: str, kind: str, expected: str) -> None:
    assert bump_semver(version, kind) == expected


def test_minor_bump_resets_patch() -> None:
    assert bump_semver("4.5.99", "minor") == "4.6.0"


def test_major_bump_resets_minor_and_patch() -> None:
    assert bump_semver("4.5.99", "major") == "5.0.0"


@pytest.mark.parametrize(
    "bad_version",
    [
        "1.2",
        "1",
        "1.2.3.4",
        "v1.2.3",
        "",
        "1.2.x",
        "1.a.3",
        "1.2.3-rc1",
        "1.2.3+build42",
    ],
)
def test_bump_semver_rejects_non_semver(bad_version: str) -> None:
    with pytest.raises(ValueError):
        bump_semver(bad_version, "patch")


@pytest.mark.parametrize("bad_kind", ["", "PATCH", "fix", "release"])
def test_bump_semver_rejects_unknown_kind(bad_kind: str) -> None:
    with pytest.raises(ValueError):
        bump_semver("1.0.0", bad_kind)


# ─── main() via subprocess (exit codes + on-disk mutation) ─────

def _write_registry(tmp_path: Path, version: str = "1.2.3") -> Path:
    reg = {
        "marketplace": {
            "name": "releng",
            "owner": {"name": "R", "email": "r@t.com"},
            "description": "RelEng plugins for Claude Code and Codex CLI — fixture.",
            "version": "0.1.0",
            "pluginRoot": "./plugins",
        },
        "plugins": [
            {
                "name": "releng-architecture",
                "version": version,
                "description": "A fixture plugin used by the bump-version subprocess tests.",
                "category": "documentation",
                "keywords": ["releng"],
                "owners": ["r"],
            }
        ],
    }
    p = tmp_path / "plugins.json"
    p.write_text(json.dumps(reg, indent=2) + "\n")
    return p


def _run_main(registry: Path | None, plugin: str | None, bump: str | None) -> subprocess.CompletedProcess:
    env = os.environ.copy()
    # Start from a clean slate so a stray REGISTRY/etc. from the shell can't leak in.
    for var in ("REGISTRY", "PLUGIN_NAME", "BUMP_TYPE"):
        env.pop(var, None)
    if registry is not None:
        env["REGISTRY"] = str(registry)
    if plugin is not None:
        env["PLUGIN_NAME"] = plugin
    if bump is not None:
        env["BUMP_TYPE"] = bump
    return subprocess.run(
        [sys.executable, str(_TARGET)],
        env=env,
        capture_output=True,
        text=True,
    )


@pytest.mark.parametrize(
    "bump,expected_new",
    [("patch", "1.2.4"), ("minor", "1.3.0"), ("major", "2.0.0")],
)
def test_main_bumps_on_disk(tmp_path: Path, bump: str, expected_new: str) -> None:
    reg = _write_registry(tmp_path, "1.2.3")
    proc = _run_main(reg, "releng-architecture", bump)
    assert proc.returncode == 0, proc.stderr
    assert proc.stdout.strip() == f"1.2.3|{expected_new}"
    # The registry on disk was actually rewritten with the new version.
    data = json.loads(reg.read_text())
    assert data["plugins"][0]["version"] == expected_new


def test_main_plugin_not_found_exits_2(tmp_path: Path) -> None:
    reg = _write_registry(tmp_path)
    proc = _run_main(reg, "does-not-exist", "patch")
    assert proc.returncode == 2
    assert "not found" in proc.stderr.lower()
    # Registry unchanged.
    assert json.loads(reg.read_text())["plugins"][0]["version"] == "1.2.3"


@pytest.mark.parametrize("missing", ["REGISTRY", "PLUGIN_NAME", "BUMP_TYPE"])
def test_main_missing_env_exits_1(tmp_path: Path, missing: str) -> None:
    reg = _write_registry(tmp_path)
    kwargs = {"registry": reg, "plugin": "releng-architecture", "bump": "patch"}
    key = {"REGISTRY": "registry", "PLUGIN_NAME": "plugin", "BUMP_TYPE": "bump"}[missing]
    kwargs[key] = None
    proc = _run_main(**kwargs)
    assert proc.returncode == 1
    assert missing in proc.stderr
