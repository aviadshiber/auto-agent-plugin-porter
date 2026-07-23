#!/usr/bin/env python3
# generate-manifests.py — Build engine for the dual-format marketplace.
#
# Reads registry/plugins.json (sole source of truth) and emits four artefact
# groups:
#   1. .claude-plugin/marketplace.json                 — Claude Code catalog
#   2. .agents/plugins/marketplace.json                — Codex CLI catalog
#   3. plugins/<n>/.claude-plugin/plugin.json          — Claude Code manifests (skipped if codex_only)
#   4. plugins/<n>/.codex-plugin/plugin.json           — Codex CLI manifests (skipped if claude_only)
#
# Modes:
#   default            — write artefacts to the working tree
#   --check            — write to a tempdir; diff against working tree; exit 1
#                        on any drift. Used by pre-commit and CI.
#
# Byte-identity contract: re-running this on an unchanged registry MUST
# produce zero diff against the existing files. The acceptance gate is
# `git diff --exit-code` after the first emission.
import argparse
import filecmp
import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
REGISTRY = REPO / "registry" / "plugins.json"
NAME_RE = re.compile(r"^[a-z][a-z0-9-]*$")

# Internal registry category → Codex marketplace category (Title-Case vocabulary).
# Ground truth: OpenAI's curated Codex marketplace uses Title-Case values such as
# "Developer Tools", "Data & Analytics", "Other". The internal registry categories
# (lowercase, Claude-flavoured) must be mapped, never emitted verbatim, or Codex
# rejects the catalog/plugin manifest. Unknown categories default to "Other".
CODEX_CATEGORY = {
    "documentation": "Developer Tools",
    "debugging": "Developer Tools",
    "devops": "Developer Tools",
    "development": "Developer Tools",
    "testing": "Developer Tools",
    "monitoring": "Developer Tools",
    "analytics": "Data & Analytics",
}


def codex_category(internal: str) -> str:
    """Map an internal registry category to the Codex Title-Case vocabulary."""
    return CODEX_CATEGORY.get(internal, "Other")


def _check_plugin_name(name: str) -> None:
    """Defense-in-depth: reject names that could traverse outside plugins/.

    The JSON schema already enforces this, but scaffolders (new-plugin.sh)
    run the generator before validate-json.sh, so the generator must guard
    its own write paths.
    """
    if not isinstance(name, str) or not NAME_RE.match(name):
        raise SystemExit(f"generate-manifests: invalid plugin name in registry: {name!r}")


def _check_exclusivity(p: dict) -> None:
    """A plugin cannot be both claude_only and codex_only — that would emit no
    catalog entry and no manifest for either target (a plugin that ships
    nowhere). Fail loudly rather than silently produce a dead plugin.
    """
    if p.get("claude_only") and p.get("codex_only"):
        raise SystemExit(
            f"generate-manifests: plugin {p['name']!r} is both claude_only and "
            "codex_only — a plugin must target at least one agent."
        )


def load_registry() -> dict:
    return json.loads(REGISTRY.read_text())


def write_json(path: Path, data, *, ensure_ascii: bool):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w") as f:
        json.dump(data, f, indent=2, ensure_ascii=ensure_ascii)
        f.write("\n")


def render_claude_catalog(reg: dict) -> dict:
    plugins = []
    for p in reg["plugins"]:
        if p.get("codex_only"):
            continue
        entry = {
            "name": p["name"],
            "source": f"./plugins/{p['name']}",
            "description": p["description"],
            "version": p["version"],
            "keywords": p["keywords"],
            "category": p["category"],
        }
        if "lspServers" in p:
            entry["lspServers"] = p["lspServers"]
        plugins.append(entry)

    m = reg["marketplace"]
    return {
        "name": m["name"],
        "owner": m["owner"],
        "metadata": {
            "description": m["description"],
            "version": m["version"],
            "pluginRoot": m["pluginRoot"],
        },
        "plugins": plugins,
    }


def render_claude_plugin_manifest(p: dict) -> dict:
    desc = p.get("manifest_description", p["description"])
    out = {
        "name": p["name"],
        "description": desc,
        "version": p["version"],
    }
    lsp = p.get("manifest_lspServers", p.get("lspServers"))
    if lsp:
        out["lspServers"] = lsp
    return out


def render_codex_catalog(reg: dict) -> dict:
    m = reg["marketplace"]
    plugins = []
    for p in reg["plugins"]:
        if p.get("claude_only"):
            continue
        plugins.append({
            "name": p["name"],
            "source": {"source": "local", "path": f"./plugins/{p['name']}"},
            # Verified against OpenAI's curated Codex marketplace (build-web-apps):
            # skills-only plugins carry authentication "ON_USE" (NOT "ON_FIRST_USE",
            # which Codex rejects as invalid) and products ["CODEX"].
            "policy": {
                "installation": "AVAILABLE",
                "authentication": "ON_USE",
                "products": ["CODEX"],
            },
            "category": codex_category(p["category"]),
        })
    return {
        "name": m["name"],
        "interface": {"displayName": m["name"]},
        "plugins": plugins,
    }


def render_codex_plugin_manifest(p: dict) -> dict:
    desc = p.get("manifest_description", p["description"])
    out = {
        "name": p["name"],
        "version": p["version"],
        "description": desc,
        "skills": "./skills/",
        "keywords": p["keywords"],
        "interface": {
            "displayName": p["name"],
            "category": codex_category(p["category"]),
        },
    }
    return out


def compute_skill_text(text: str, claude_only: bool = False, codex_only: bool = False) -> str:
    """Pure function: return desired SKILL.md text after stamping frontmatter.

    Idempotent — running twice produces the same output. SKILL.md body content
    is never altered, only the YAML frontmatter block between leading `---`.
    """
    if not text.startswith("---\n"):
        return text
    end = text.find("\n---\n", 4)
    if end == -1:
        return text
    fm = text[4:end]
    body = text[end + 5:]

    if codex_only:
        targets = ["codex-cli"]
    elif claude_only:
        targets = ["claude-code"]
    else:
        targets = ["claude-code", "codex-cli"]

    # YAML-safe metadata surgery. We must not assume `compatibility` is the sole
    # child of `metadata:` — a sibling key (e.g. `metadata.custom_key`) must
    # survive, and orphaning or dropping it would produce invalid YAML.
    #
    # Strategy (mirrors the original rstrip+append shape so the no-sibling case
    # stays byte-identical): locate the entire existing top-level `metadata:`
    # block = the `metadata:` line plus every following line indented >= 2
    # spaces (stop at the first non-indented line or EOF). Split its direct
    # children (lines indented exactly 2 spaces) into groups, drop the
    # `compatibility` group, keep the rest verbatim, then re-emit `metadata:`
    # at the end with the preserved siblings FIRST (original order/formatting)
    # followed by a freshly-stamped `compatibility:` list.
    lines = fm.split("\n")

    meta_idx = next(
        (i for i, ln in enumerate(lines) if ln.rstrip() == "metadata:"),
        None,
    )
    if meta_idx is None:
        block_body: list[str] = []
        remaining = lines
    else:
        j = meta_idx + 1
        while j < len(lines) and lines[j].startswith("  "):
            j += 1
        block_body = lines[meta_idx + 1:j]
        remaining = lines[:meta_idx] + lines[j:]

    # Group direct children of `metadata:`. A new child starts at a line
    # indented exactly 2 spaces (`^  \S`); more-indented lines belong to the
    # current child (e.g. `compatibility:`'s list items, or a nested mapping).
    kept_child_lines: list[str] = []
    keep_current = True
    for ln in block_body:
        if re.match(r"^  \S", ln):
            key = ln.strip().split(":", 1)[0].strip()
            keep_current = key != "compatibility"
        if keep_current:
            kept_child_lines.append(ln)

    rebuilt = (
        ["metadata:"]
        + kept_child_lines
        + ["  compatibility:"]
        + [f"    - {t}" for t in targets]
    )
    target_block = "\n".join(rebuilt) + "\n"

    new_fm = "\n".join(remaining).rstrip("\n") + "\n" + target_block

    return "---\n" + new_fm + "---\n" + body


def emit_all(reg: dict, root: Path):
    """Write every artefact under `root` (the actual repo root in default mode,
    a tempdir in --check mode).

    SKILL.md is read from REPO and the desired (stamped) version is written
    under `root`. In default mode that overwrites the real file in place; in
    --check mode it lands in the tempdir so diff_trees can compare.
    """
    for p in reg["plugins"]:
        _check_plugin_name(p["name"])
        _check_exclusivity(p)
        src = REPO / "plugins" / p["name"] / "skills" / p["name"] / "SKILL.md"
        if not src.is_file():
            continue
        dst = root / "plugins" / p["name"] / "skills" / p["name"] / "SKILL.md"
        new_text = compute_skill_text(
            src.read_text(),
            claude_only=bool(p.get("claude_only")),
            codex_only=bool(p.get("codex_only")),
        )
        dst.parent.mkdir(parents=True, exist_ok=True)
        # Skip the write when nothing would change — avoids spurious mtime
        # bumps and avoids creating a "dirty" git state when the metadata
        # block is already correct. In --check mode dst != src so we still
        # write to the tempdir.
        if dst == src and dst.exists() and dst.read_text() == new_text:
            continue
        dst.write_text(new_text)

    write_json(
        root / ".claude-plugin" / "marketplace.json",
        render_claude_catalog(reg),
        ensure_ascii=True,
    )
    write_json(
        root / ".agents" / "plugins" / "marketplace.json",
        render_codex_catalog(reg),
        ensure_ascii=False,
    )
    for p in reg["plugins"]:
        _check_plugin_name(p["name"])
        manifest_unicode = p.get("manifest_unicode", False)
        if not p.get("codex_only"):
            write_json(
                root / "plugins" / p["name"] / ".claude-plugin" / "plugin.json",
                render_claude_plugin_manifest(p),
                ensure_ascii=not manifest_unicode,
            )
        if not p.get("claude_only"):
            write_json(
                root / "plugins" / p["name"] / ".codex-plugin" / "plugin.json",
                render_codex_plugin_manifest(p),
                ensure_ascii=False,
            )


def diff_trees(generated: Path, real: Path, reg: dict) -> list:
    """Return list of paths that differ between generated and real."""
    paths_to_check = [
        Path(".claude-plugin/marketplace.json"),
        Path(".agents/plugins/marketplace.json"),
    ]
    for p in reg["plugins"]:
        if not p.get("codex_only"):
            paths_to_check.append(Path(f"plugins/{p['name']}/.claude-plugin/plugin.json"))
        if not p.get("claude_only"):
            paths_to_check.append(Path(f"plugins/{p['name']}/.codex-plugin/plugin.json"))
        paths_to_check.append(Path(f"plugins/{p['name']}/skills/{p['name']}/SKILL.md"))

    drift = []
    for rel in paths_to_check:
        gen = generated / rel
        actual = real / rel
        gen_exists = gen.exists()
        actual_exists = actual.exists()
        # Both absent is fine — happens for SKILL.md when a plugin has no
        # source SKILL.md to stamp. emit_all() skips it in both `root`s.
        if not gen_exists and not actual_exists:
            continue
        # One side missing is drift: either the generator failed to emit a
        # file the repo has (orphan) or the repo is missing a file the
        # generator wants to produce (forgot to commit).
        if gen_exists != actual_exists:
            drift.append(str(rel))
            continue
        if not filecmp.cmp(gen, actual, shallow=False):
            drift.append(str(rel))
    return drift


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true", help="diff-only; exit 1 on drift")
    args = ap.parse_args()

    reg = load_registry()

    if args.check:
        with tempfile.TemporaryDirectory() as td:
            tmp = Path(td)
            emit_all(reg, tmp)
            drift = diff_trees(tmp, REPO, reg)
            if drift:
                print("Drift detected — registry was edited but manifests not regenerated:", file=sys.stderr)
                for d in drift:
                    print(f"  {d}", file=sys.stderr)
                print("\nRun: python3 scripts/generate-manifests.py", file=sys.stderr)
                # Show first diff for context
                first = drift[0]
                try:
                    diff = subprocess.run(
                        ["diff", "-u", str(REPO / first), str(tmp / first)],
                        capture_output=True, text=True, timeout=10,
                    )
                    print("", file=sys.stderr)
                    print(diff.stdout[:4000], file=sys.stderr)
                except Exception:
                    pass
                sys.exit(1)
            print(f"generate-manifests --check: OK ({len(reg['plugins'])} plugins, no drift)")
            return

    emit_all(reg, REPO)
    print(f"wrote manifests for {len(reg['plugins'])} plugins")


if __name__ == "__main__":
    main()
