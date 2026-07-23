"""Tests for scripts/generate-manifests.py — pure-function coverage.

The end-to-end byte-identity contract is exercised by
``python3 scripts/generate-manifests.py --check`` in CI; these unit tests
lock down the smaller pieces:

- ``compute_skill_text`` idempotency and metadata-block surgery
- ``render_*`` shapes match what we ship today (incl. the verified Codex schema)
- ``codex_category`` maps the internal vocabulary to Codex Title-Case
- ``diff_trees`` flags missing-on-either-side as drift
- ``_check_plugin_name`` rejects path-traversal attempts
"""
from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

_HERE = Path(__file__).resolve().parent
_TARGET = _HERE.parent / "generate-manifests.py"

spec = importlib.util.spec_from_file_location("generate_manifests", _TARGET)
assert spec and spec.loader
gm = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gm)


# ─── compute_skill_text ────────────────────────────────────────

def test_compute_skill_text_stamps_dual_compatibility():
    src = "---\nname: foo\ndescription: bar\n---\nbody\n"
    out = gm.compute_skill_text(src, claude_only=False)
    assert "compatibility:" in out
    assert "    - claude-code" in out
    assert "    - codex-cli" in out
    assert out.endswith("body\n")


def test_compute_skill_text_claude_only_drops_codex():
    src = "---\nname: foo\ndescription: bar\n---\nbody\n"
    out = gm.compute_skill_text(src, claude_only=True)
    assert "    - claude-code" in out
    assert "codex-cli" not in out


def test_compute_skill_text_codex_only_drops_claude():
    src = "---\nname: foo\ndescription: bar\n---\nbody\n"
    out = gm.compute_skill_text(src, codex_only=True)
    assert "    - codex-cli" in out
    assert "claude-code" not in out


def test_compute_skill_text_is_idempotent():
    src = "---\nname: foo\ndescription: bar\n---\nbody\n"
    once = gm.compute_skill_text(src, claude_only=False)
    twice = gm.compute_skill_text(once, claude_only=False)
    assert once == twice


def test_compute_skill_text_codex_only_is_idempotent():
    src = "---\nname: foo\ndescription: bar\n---\nbody\n"
    once = gm.compute_skill_text(src, codex_only=True)
    twice = gm.compute_skill_text(once, codex_only=True)
    assert once == twice
    assert "claude-code" not in twice


def test_compute_skill_text_replaces_old_metadata():
    """Re-running with claude_only=True after a dual-target stamp must
    drop the codex-cli line, not duplicate the metadata block."""
    src = "---\nname: foo\ndescription: bar\n---\nbody\n"
    dual = gm.compute_skill_text(src, claude_only=False)
    claude_only = gm.compute_skill_text(dual, claude_only=True)
    assert claude_only.count("metadata:\n") == 1
    assert "codex-cli" not in claude_only


def _frontmatter(text: str) -> str:
    """Return the YAML frontmatter block (between the leading `---` fences)."""
    assert text.startswith("---\n")
    end = text.find("\n---\n", 4)
    assert end != -1
    return text[4:end]


@pytest.mark.parametrize(
    "metadata_block",
    [
        # sibling AFTER compatibility
        "metadata:\n  compatibility:\n    - claude-code\n  custom_key: value\n",
        # sibling BEFORE compatibility
        "metadata:\n  custom_key: value\n  compatibility:\n    - claude-code\n",
    ],
    ids=["sibling-after", "sibling-before"],
)
def test_compute_skill_text_preserves_user_metadata_keys(metadata_block):
    yaml = pytest.importorskip("yaml")
    src = (
        "---\n"
        "name: foo\n"
        "description: bar\n"
        + metadata_block
        + "---\n"
        "body\n"
    )
    out = gm.compute_skill_text(src, claude_only=False)

    # (a) resulting frontmatter is valid YAML
    fm = yaml.safe_load(_frontmatter(out))
    assert isinstance(fm, dict)

    # (b) custom_key is still nested under metadata (not orphaned/dropped)
    assert fm["metadata"]["custom_key"] == "value"

    # (c) compatibility is present and freshly stamped for both targets
    assert fm["metadata"]["compatibility"] == ["claude-code", "codex-cli"]

    # Top-level keys survive too.
    assert fm["name"] == "foo"
    assert fm["description"] == "bar"

    # Idempotent even with the sibling present.
    assert gm.compute_skill_text(out, claude_only=False) == out


def test_compute_skill_text_passes_through_when_no_frontmatter():
    src = "# Just a markdown header\n\nbody text\n"
    assert gm.compute_skill_text(src, claude_only=False) == src


def test_compute_skill_text_passes_through_unterminated_frontmatter():
    src = "---\nname: foo\ndescription: oops never closed\nstill body\n"
    assert gm.compute_skill_text(src, claude_only=False) == src


# ─── codex_category ────────────────────────────────────────────

@pytest.mark.parametrize(
    "internal, codex",
    [
        ("documentation", "Developer Tools"),
        ("analytics", "Data & Analytics"),
        ("devops", "Developer Tools"),
        ("debugging", "Developer Tools"),
        ("testing", "Developer Tools"),
        ("monitoring", "Developer Tools"),
        ("development", "Developer Tools"),
    ],
)
def test_codex_category_maps_known(internal, codex):
    assert gm.codex_category(internal) == codex


def test_codex_category_defaults_unknown_to_other():
    assert gm.codex_category("nonsense") == "Other"
    assert gm.codex_category("") == "Other"


# ─── _check_plugin_name ────────────────────────────────────────

@pytest.mark.parametrize(
    "good_name",
    ["a", "foo", "releng-architecture", "x9-y8-z7", "abc123"],
)
def test_check_plugin_name_accepts_valid(good_name):
    gm._check_plugin_name(good_name)  # no exception


@pytest.mark.parametrize(
    "bad_name",
    [
        "../etc/passwd",
        "Foo",          # uppercase
        "9foo",         # leading digit
        "-foo",         # leading hyphen
        "foo bar",      # whitespace
        "foo/bar",      # slash
        "foo.bar",      # dot
        "",             # empty
    ],
)
def test_check_plugin_name_rejects_invalid(bad_name):
    with pytest.raises(SystemExit):
        gm._check_plugin_name(bad_name)


def test_check_plugin_name_rejects_non_string():
    with pytest.raises(SystemExit):
        gm._check_plugin_name(42)  # type: ignore[arg-type]


# ─── render_claude_plugin_manifest ─────────────────────────────

def test_render_claude_plugin_manifest_minimal():
    p = {"name": "foo", "version": "1.0.0", "description": "d"}
    out = gm.render_claude_plugin_manifest(p)
    assert out == {"name": "foo", "description": "d", "version": "1.0.0"}


def test_render_claude_plugin_manifest_uses_override_description():
    p = {
        "name": "foo",
        "version": "1.0.0",
        "description": "for catalog",
        "manifest_description": "for plugin.json",
    }
    out = gm.render_claude_plugin_manifest(p)
    assert out["description"] == "for plugin.json"


def test_render_claude_plugin_manifest_forwards_lsp():
    p = {
        "name": "foo",
        "version": "1.0.0",
        "description": "d",
        "lspServers": {"java": {"command": "jdtls"}},
    }
    out = gm.render_claude_plugin_manifest(p)
    assert out["lspServers"] == {"java": {"command": "jdtls"}}


# ─── render_codex_plugin_manifest (verified Codex schema) ──────

def test_render_codex_plugin_manifest_shape():
    p = {
        "name": "foo",
        "version": "1.0.0",
        "description": "d",
        "keywords": ["a", "b"],
        "category": "devops",
    }
    out = gm.render_codex_plugin_manifest(p)
    assert out["name"] == "foo"
    assert out["version"] == "1.0.0"
    assert out["skills"] == "./skills/"
    # category is mapped to the Codex Title-Case vocabulary, never verbatim.
    assert out["interface"] == {"displayName": "foo", "category": "Developer Tools"}


def test_render_codex_plugin_manifest_documentation_category():
    p = {
        "name": "foo",
        "version": "1.0.0",
        "description": "d",
        "keywords": ["a"],
        "category": "documentation",
    }
    out = gm.render_codex_plugin_manifest(p)
    assert out["interface"]["category"] == "Developer Tools"


# ─── render_codex_catalog (verified policy + category) ─────────

def test_render_codex_catalog_policy_and_category():
    reg = {
        "marketplace": {"name": "releng"},
        "plugins": [
            {"name": "a", "version": "1", "description": "d", "category": "documentation", "keywords": [], "owners": []},
        ],
    }
    out = gm.render_codex_catalog(reg)
    assert out["interface"] == {"displayName": "releng"}
    entry = out["plugins"][0]
    assert entry["source"] == {"source": "local", "path": "./plugins/a"}
    assert entry["policy"] == {
        "installation": "AVAILABLE",
        "authentication": "ON_USE",
        "products": ["CODEX"],
    }
    assert entry["category"] == "Developer Tools"


def test_render_codex_catalog_skips_claude_only():
    reg = {
        "marketplace": {"name": "mp"},
        "plugins": [
            {"name": "a", "version": "1", "description": "d", "category": "documentation", "keywords": [], "owners": []},
            {"name": "b", "version": "1", "description": "d", "category": "documentation", "keywords": [], "owners": [], "claude_only": True},
        ],
    }
    out = gm.render_codex_catalog(reg)
    names = [p["name"] for p in out["plugins"]]
    assert names == ["a"]


# ─── render_claude_catalog (codex_only omission) ───────────────

def test_render_claude_catalog_skips_codex_only():
    reg = {
        "marketplace": {
            "name": "mp",
            "owner": {"name": "o", "email": "o@e.com"},
            "description": "d",
            "version": "0.1.0",
            "pluginRoot": "./plugins",
        },
        "plugins": [
            {"name": "a", "version": "1", "description": "d", "category": "documentation", "keywords": [], "owners": []},
            {"name": "b", "version": "1", "description": "d", "category": "documentation", "keywords": [], "owners": [], "codex_only": True},
        ],
    }
    out = gm.render_claude_catalog(reg)
    names = [p["name"] for p in out["plugins"]]
    assert names == ["a"]


# ─── _check_exclusivity ────────────────────────────────────────

def test_check_exclusivity_rejects_both_flags():
    with pytest.raises(SystemExit):
        gm._check_exclusivity(
            {"name": "x", "claude_only": True, "codex_only": True}
        )


@pytest.mark.parametrize(
    "plugin",
    [
        {"name": "x"},
        {"name": "x", "claude_only": True},
        {"name": "x", "codex_only": True},
    ],
)
def test_check_exclusivity_allows_at_most_one(plugin):
    gm._check_exclusivity(plugin)  # no exception


# ─── diff_trees ────────────────────────────────────────────────

def test_diff_trees_reports_missing_actual(tmp_path):
    gen = tmp_path / "gen"
    actual = tmp_path / "actual"
    (gen / ".claude-plugin").mkdir(parents=True)
    (gen / ".claude-plugin" / "marketplace.json").write_text("{}")
    actual.mkdir()  # no .claude-plugin/marketplace.json
    reg = {"plugins": []}
    drift = gm.diff_trees(gen, actual, reg)
    assert ".claude-plugin/marketplace.json" in drift


def test_diff_trees_reports_missing_generated(tmp_path):
    gen = tmp_path / "gen"
    actual = tmp_path / "actual"
    gen.mkdir()
    (actual / ".agents" / "plugins").mkdir(parents=True)
    (actual / ".agents" / "plugins" / "marketplace.json").write_text("{}")
    reg = {"plugins": []}
    drift = gm.diff_trees(gen, actual, reg)
    assert ".agents/plugins/marketplace.json" in drift


def test_diff_trees_no_drift_when_both_absent(tmp_path):
    gen = tmp_path / "gen"
    actual = tmp_path / "actual"
    gen.mkdir()
    actual.mkdir()
    for root in (gen, actual):
        (root / ".claude-plugin").mkdir(parents=True)
        (root / ".claude-plugin" / "marketplace.json").write_text("x")
        (root / ".agents" / "plugins").mkdir(parents=True)
        (root / ".agents" / "plugins" / "marketplace.json").write_text("x")
        (root / "plugins" / "foo" / ".claude-plugin").mkdir(parents=True)
        (root / "plugins" / "foo" / ".claude-plugin" / "plugin.json").write_text("y")
        (root / "plugins" / "foo" / ".codex-plugin").mkdir(parents=True)
        (root / "plugins" / "foo" / ".codex-plugin" / "plugin.json").write_text("z")
    reg = {"plugins": [{"name": "foo"}]}
    drift = gm.diff_trees(gen, actual, reg)
    assert drift == []


def test_diff_trees_skips_claude_manifest_for_codex_only(tmp_path):
    """A codex_only plugin ships no .claude-plugin/plugin.json, so its absence
    on both sides must not be flagged as drift — and a stray one is irrelevant."""
    gen = tmp_path / "gen"
    actual = tmp_path / "actual"
    for root in (gen, actual):
        (root / ".claude-plugin").mkdir(parents=True)
        (root / ".claude-plugin" / "marketplace.json").write_text("x")
        (root / ".agents" / "plugins").mkdir(parents=True)
        (root / ".agents" / "plugins" / "marketplace.json").write_text("x")
        (root / "plugins" / "foo" / ".codex-plugin").mkdir(parents=True)
        (root / "plugins" / "foo" / ".codex-plugin" / "plugin.json").write_text("z")
    # Only the generated tree has a .claude-plugin/plugin.json — if diff_trees
    # were still checking it for a codex_only plugin, this would be drift.
    (gen / "plugins" / "foo" / ".claude-plugin").mkdir(parents=True)
    (gen / "plugins" / "foo" / ".claude-plugin" / "plugin.json").write_text("y")
    reg = {"plugins": [{"name": "foo", "codex_only": True}]}
    drift = gm.diff_trees(gen, actual, reg)
    assert drift == []


def test_diff_trees_skips_codex_manifest_for_claude_only(tmp_path):
    """Mirror of the codex_only case: a claude_only plugin ships no
    .codex-plugin/plugin.json, so a stray generated one must not be flagged."""
    gen = tmp_path / "gen"
    actual = tmp_path / "actual"
    for root in (gen, actual):
        (root / ".claude-plugin").mkdir(parents=True)
        (root / ".claude-plugin" / "marketplace.json").write_text("x")
        (root / ".agents" / "plugins").mkdir(parents=True)
        (root / ".agents" / "plugins" / "marketplace.json").write_text("x")
        (root / "plugins" / "foo" / ".claude-plugin").mkdir(parents=True)
        (root / "plugins" / "foo" / ".claude-plugin" / "plugin.json").write_text("y")
    # Only the generated tree has a .codex-plugin/plugin.json — must be ignored
    # for a claude_only plugin.
    (gen / "plugins" / "foo" / ".codex-plugin").mkdir(parents=True)
    (gen / "plugins" / "foo" / ".codex-plugin" / "plugin.json").write_text("z")
    reg = {"plugins": [{"name": "foo", "claude_only": True}]}
    drift = gm.diff_trees(gen, actual, reg)
    assert drift == []


def test_diff_trees_reports_content_diff(tmp_path):
    gen = tmp_path / "gen"
    actual = tmp_path / "actual"
    (gen / ".claude-plugin").mkdir(parents=True)
    (actual / ".claude-plugin").mkdir(parents=True)
    (gen / ".claude-plugin" / "marketplace.json").write_text('{"a":1}')
    (actual / ".claude-plugin" / "marketplace.json").write_text('{"a":2}')
    reg = {"plugins": []}
    drift = gm.diff_trees(gen, actual, reg)
    assert ".claude-plugin/marketplace.json" in drift
