"""Tests for scripts/check-instructions-sync.sh — the FLIPPED contract.

AGENTS.md is the canonical real file; CLAUDE.md points to it, either as a
symlink → AGENTS.md or as a file whose first non-empty line is `@AGENTS.md`.
This is the opposite of the deeperdive prototype (which made CLAUDE.md
canonical), so every assertion here is the inverse of that prototype's tests.

Each test stages a fresh tiny tree, invokes the script with cwd pointing at
the tree, and asserts on exit code + stderr/stdout content.
"""
from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path

_HERE = Path(__file__).resolve().parent
_SCRIPT = _HERE.parent / "check-instructions-sync.sh"


def _stage(tmp_path: Path) -> Path:
    (tmp_path / "scripts").mkdir()
    dst = tmp_path / "scripts" / "check-instructions-sync.sh"
    shutil.copy(_SCRIPT, dst)
    dst.chmod(0o755)
    return tmp_path


def _run(repo_root: Path) -> subprocess.CompletedProcess:
    return subprocess.run(
        [str(repo_root / "scripts" / "check-instructions-sync.sh")],
        capture_output=True,
        text=True,
        cwd=str(repo_root),
    )


def test_happy_path_import_form(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "AGENTS.md").write_text("# canonical practices\n")
    (root / "CLAUDE.md").write_text("@AGENTS.md\n")
    proc = _run(root)
    assert proc.returncode == 0, proc.stderr
    assert "OK" in proc.stdout


def test_happy_path_import_form_with_leading_blank_line(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "AGENTS.md").write_text("# canonical practices\n")
    (root / "CLAUDE.md").write_text("\n@AGENTS.md\n")
    proc = _run(root)
    assert proc.returncode == 0, proc.stderr


def test_happy_path_symlink_form(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "AGENTS.md").write_text("# canonical practices\n")
    os.symlink("AGENTS.md", root / "CLAUDE.md")
    proc = _run(root)
    assert proc.returncode == 0, proc.stderr
    assert "OK" in proc.stdout


def test_agents_md_missing(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "CLAUDE.md").write_text("@AGENTS.md\n")
    proc = _run(root)
    assert proc.returncode == 1
    assert "AGENTS.md is missing" in proc.stderr


def test_agents_md_is_a_symlink_is_rejected(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "real.md").write_text("# canonical\n")
    os.symlink("real.md", root / "AGENTS.md")
    (root / "CLAUDE.md").write_text("@AGENTS.md\n")
    proc = _run(root)
    assert proc.returncode == 1
    assert "AGENTS.md must be a regular file" in proc.stderr


def test_claude_md_missing(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "AGENTS.md").write_text("# canonical\n")
    proc = _run(root)
    assert proc.returncode == 1
    assert "CLAUDE.md is missing" in proc.stderr


def test_claude_md_import_wrong_content(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "AGENTS.md").write_text("# canonical\n")
    (root / "CLAUDE.md").write_text("# some other content\n@AGENTS.md\n")
    proc = _run(root)
    assert proc.returncode == 1
    assert "first non-empty line must be the import '@AGENTS.md'" in proc.stderr


def test_claude_md_symlink_wrong_target(tmp_path: Path):
    root = _stage(tmp_path)
    (root / "AGENTS.md").write_text("# canonical\n")
    (root / "OTHER.md").write_text("# wrong\n")
    os.symlink("OTHER.md", root / "CLAUDE.md")
    proc = _run(root)
    assert proc.returncode == 1
    assert "expected 'AGENTS.md'" in proc.stderr
