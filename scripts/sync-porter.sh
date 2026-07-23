#!/usr/bin/env bash
#
# sync-porter.sh — vendor the canonical Rust porter crate + wrappers into each
# porter plugin.
#
# Self-containment rule: an installed plugin is cached in isolation and may not
# reference files outside its own directory (validate.sh forbids `../`). So the
# porter crate and its wrappers must physically live inside each plugin. To
# avoid maintaining N copies by hand we keep ONE canonical source at repo-root
# `porter/` and generate the per-plugin copies here — the same "one source of
# truth → generated artifacts" model the manifest generator uses.
#
#   porter/{Cargo.toml,Cargo.lock,src/}  →  plugins/<p>/porter/
#   porter/wrappers/*.{sh,ps1}           →  plugins/<p>/scripts/
#
# Modes:
#   (default)   write the vendored copies
#   --check     exit 1 if any vendored copy differs from canonical (CI gate)
#
# Never vendors tests/ or target/ (kept lean; build --release ignores them).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CANON="$REPO_ROOT/porter"
WRAPPERS="$CANON/wrappers"

# Plugins that ship the porter engine.
PORTER_PLUGINS=(codex-to-claude claude-to-codex)

CHECK=0
[[ "${1:-}" == "--check" ]] && CHECK=1

green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*"; }

# Render the desired vendored tree for one plugin into $1 (a fresh dir).
render_into() {
    local out="$1"
    mkdir -p "$out/porter/src" "$out/scripts"
    cp "$CANON/Cargo.toml" "$CANON/Cargo.lock" "$out/porter/"
    cp -R "$CANON/src/." "$out/porter/src/"
    cp "$WRAPPERS"/*.sh "$WRAPPERS"/*.ps1 "$out/scripts/"
    chmod +x "$out/scripts/"*.sh
}

drift=0
for plugin in "${PORTER_PLUGINS[@]}"; do
    pdir="$REPO_ROOT/plugins/$plugin"
    if [[ ! -d "$pdir" ]]; then
        red "sync-porter: plugin dir missing: plugins/$plugin"
        exit 1
    fi

    if [[ "$CHECK" -eq 1 ]]; then
        tmp="$(mktemp -d)"
        trap 'rm -rf "$tmp"' EXIT
        render_into "$tmp"
        # Compare the two generated component trees against what is on disk.
        for sub in porter/Cargo.toml porter/Cargo.lock porter/src scripts; do
            if ! diff -r "$tmp/$sub" "$pdir/$sub" >/dev/null 2>&1; then
                red "sync-porter: DRIFT in plugins/$plugin/$sub — run: ./scripts/sync-porter.sh"
                drift=1
            fi
        done
        rm -rf "$tmp"
        trap - EXIT
    else
        rm -rf "$pdir/porter"
        # Remove only the vendored wrapper scripts, leaving room for none other
        # (all scripts in a porter plugin are vendored).
        rm -f "$pdir/scripts/"*.sh "$pdir/scripts/"*.ps1 2>/dev/null || true
        render_into "$pdir"
        green "  ✓ vendored porter crate + wrappers → plugins/$plugin"
    fi
done

if [[ "$CHECK" -eq 1 ]]; then
    if [[ "$drift" -eq 1 ]]; then
        exit 1
    fi
    green "sync-porter --check: OK (vendored copies match canonical)"
fi
