#!/usr/bin/env python3
# bootstrap-registry.py — Maintenance tool: (re)build registry/plugins.json
# from the current .claude-plugin/marketplace.json + per-plugin plugin.json
# files + OWNERS. Useful when reconstructing the registry from generated
# artefacts (the inverse of generate-manifests.py).
#
# The registry must round-trip: bootstrap → generate must reproduce the
# existing generated files.
#
# Symmetry note: a claude_only plugin appears in the Claude catalog but not the
# Codex catalog; a codex_only plugin appears in the Codex catalog but not the
# Claude catalog. This tool reconstructs both. Best-effort caveat for codex_only
# plugins: their internal `category` is recovered from the Codex Title-Case
# vocabulary via CODEX_CATEGORY_REVERSE, which is lossy — the six internal
# categories that all map to "Developer Tools" collapse to "development" on the
# way back. Fix the category by hand after bootstrapping a codex_only plugin.
import json
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CATALOG = REPO / ".claude-plugin" / "marketplace.json"
CODEX_CATALOG = REPO / ".agents" / "plugins" / "marketplace.json"
PLUGINS = REPO / "plugins"
REGISTRY = REPO / "registry" / "plugins.json"

# Best-effort inverse of generate-manifests.CODEX_CATEGORY. Lossy: the several
# internal categories that map to "Developer Tools" cannot be distinguished on
# the way back, so they all recover as "development".
CODEX_CATEGORY_REVERSE = {
    "Data & Analytics": "analytics",
    "Developer Tools": "development",
    "Other": "documentation",
}


def load_codex_plugin_names() -> set | None:
    """Return the set of plugin names present in the Codex catalog, or None
    when the catalog is absent.

    A plugin that appears in the Claude catalog but NOT here was emitted with
    `claude_only: true` (render_codex_catalog skips claude_only plugins), so
    reconstructing the registry must restore that flag to round-trip cleanly.
    When the Codex catalog is missing we return None and skip claude_only
    detection — an absent catalog is not evidence that every plugin is
    Claude-only.
    """
    if not CODEX_CATALOG.exists():
        return None
    codex = json.loads(CODEX_CATALOG.read_text())
    return {p["name"] for p in codex.get("plugins", [])}


def detect_manifest_unicode(plugin_json_path: Path) -> bool:
    """Return True if the existing plugin.json was written with ensure_ascii=False
    (literal non-ASCII characters). Detected by absence of \\u escapes when raw
    bytes contain non-ASCII codepoints."""
    raw = plugin_json_path.read_bytes()
    has_literal_non_ascii = any(b > 0x7F for b in raw)
    has_unicode_escape = b"\\u" in raw
    if has_literal_non_ascii and not has_unicode_escape:
        return True
    if has_unicode_escape and not has_literal_non_ascii:
        return False
    return False


def reconstruct_codex_only(entry: dict) -> dict:
    """Reconstruct a registry entry for a codex_only plugin — one present in the
    Codex catalog but absent from the Claude catalog. Version/description/
    keywords come from the plugin's .codex-plugin/plugin.json (the Codex catalog
    entry carries none of them); category is reverse-mapped best-effort."""
    name = entry["name"]
    plugin_dir = PLUGINS / name
    codex_manifest_path = plugin_dir / ".codex-plugin" / "plugin.json"
    if not codex_manifest_path.exists():
        raise SystemExit(f"missing .codex-plugin/plugin.json for codex_only plugin {name}")
    manifest = json.loads(codex_manifest_path.read_text())

    category = CODEX_CATEGORY_REVERSE.get(entry.get("category", "Other"), "documentation")
    out = {
        "name": name,
        "version": manifest["version"],
        "description": manifest["description"],
        "category": category,
        "keywords": manifest.get("keywords", []),
    }
    owners = read_owners(plugin_dir)
    if owners:
        out["owners"] = owners
    out["codex_only"] = True
    return out


def read_owners(plugin_dir: Path) -> list:
    f = plugin_dir / "OWNERS"
    if not f.exists():
        return []
    out = []
    for line in f.read_text().splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        out.append(s)
    return out


def main():
    catalog = json.loads(CATALOG.read_text())
    codex_names = load_codex_plugin_names()
    plugins_out = []

    for entry in catalog["plugins"]:
        name = entry["name"]
        plugin_dir = PLUGINS / name
        plugin_json_path = plugin_dir / ".claude-plugin" / "plugin.json"
        if not plugin_json_path.exists():
            raise SystemExit(f"missing plugin.json for {name}")
        manifest = json.loads(plugin_json_path.read_text())

        out = {
            "name": name,
            "version": entry["version"],
            "description": entry["description"],
        }
        if manifest.get("description") and manifest["description"] != entry["description"]:
            out["manifest_description"] = manifest["description"]
        out["category"] = entry["category"]
        out["keywords"] = entry["keywords"]
        owners = read_owners(plugin_dir)
        if owners:
            out["owners"] = owners
        # Present in the Claude catalog but absent from the Codex catalog →
        # this plugin was emitted claude_only; restore the flag so a
        # subsequent generate reproduces the same absence.
        if codex_names is not None and name not in codex_names:
            out["claude_only"] = True
        if "lspServers" in entry:
            out["lspServers"] = entry["lspServers"]
        if "lspServers" in manifest and manifest.get("lspServers") != entry.get("lspServers"):
            out["manifest_lspServers"] = manifest["lspServers"]
        if detect_manifest_unicode(plugin_json_path):
            out["manifest_unicode"] = True
        plugins_out.append(out)

    # Recover codex_only plugins: present in the Codex catalog, absent from the
    # Claude catalog (the mirror image of the claude_only case above). Appended
    # after the Claude-derived entries in Codex-catalog order — bootstrap cannot
    # recover the original interleaved registry order, only per-catalog order.
    claude_names = {entry["name"] for entry in catalog["plugins"]}
    if CODEX_CATALOG.exists():
        codex = json.loads(CODEX_CATALOG.read_text())
        for entry in codex.get("plugins", []):
            if entry["name"] not in claude_names:
                plugins_out.append(reconstruct_codex_only(entry))

    registry = {
        "marketplace": {
            "name": catalog["name"],
            "owner": catalog["owner"],
            "description": catalog["metadata"]["description"],
            "version": catalog["metadata"]["version"],
            "pluginRoot": catalog["metadata"]["pluginRoot"],
        },
        "plugins": plugins_out,
    }

    REGISTRY.parent.mkdir(parents=True, exist_ok=True)
    with REGISTRY.open("w") as f:
        json.dump(registry, f, indent=2, ensure_ascii=False)
        f.write("\n")
    print(f"wrote {REGISTRY} ({len(plugins_out)} plugins)")


if __name__ == "__main__":
    main()
