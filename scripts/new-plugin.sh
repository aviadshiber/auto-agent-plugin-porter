#!/usr/bin/env bash
#
# new-plugin.sh — Scaffold a new dual-target plugin from the canonical layout.
#
# Given a plugin <name> (and optional category/description), this:
#   1. Creates plugins/<name>/skills/<name>/SKILL.md from a minimal
#      Agent-Skills template (name + description + body).
#   2. Creates plugins/<name>/OWNERS (seeded with the current user).
#   3. Creates plugins/<name>/references/.gitkeep.
#   4. Appends a registry entry via scripts/_append_to_registry.py.
#   5. Regenerates all manifests via scripts/generate-manifests.py.
#   6. Runs validate-json.sh to confirm the new plugin is schema-valid.
#
# Idempotent: if plugins/<name> already exists it is a no-op (exit 0), and
# _append_to_registry.py skips a name already in the registry.
#
# Usage:
#   ./scripts/new-plugin.sh <name> [--category <cat>] [--description <text>]
#
# Example:
#   ./scripts/new-plugin.sh <MARKETPLACE_NAME>-onboarding \
#       --category documentation \
#       --description "One line, >= 20 chars, behavioral (say when to use it)."
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGISTRY="$REPO_ROOT/registry/plugins.json"

green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*"; }

NAME=""
CATEGORY="documentation"
DESCRIPTION=""

# ── Parse args ───────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --category)
            CATEGORY="${2:-}"; shift 2 ;;
        --description)
            DESCRIPTION="${2:-}"; shift 2 ;;
        -h|--help)
            grep -E '^#( |$)' "$0" | sed 's/^# \{0,1\}//'; exit 0 ;;
        -*)
            red "Unknown option: $1"; exit 1 ;;
        *)
            if [[ -z "$NAME" ]]; then NAME="$1"; else red "Unexpected argument: $1"; exit 1; fi
            shift ;;
    esac
done

if [[ -z "$NAME" ]]; then
    red "Usage: $0 <name> [--category <cat>] [--description <text>]"
    exit 1
fi

# ── Validate name (defense-in-depth; the schema + generator also enforce) ──
# This also rejects path-traversal names like "../../etc" (the slash and dot
# fail the pattern) before anything is written to disk.
if ! [[ "$NAME" =~ ^[a-z][a-z0-9-]*$ ]]; then
    red "Invalid plugin name '$NAME'. Must match ^[a-z][a-z0-9-]*\$ (lowercase, digits, hyphens)."
    exit 1
fi

# ── Validate category against the schema enum (fail fast, before writing) ──
VALID_CATEGORIES="documentation debugging devops analytics testing monitoring development"
case " $VALID_CATEGORIES " in
    *" $CATEGORY "*) ;;
    *)
        red "Invalid category '$CATEGORY'. Valid categories: $VALID_CATEGORIES"
        exit 1 ;;
esac

PLUGIN_DIR="$REPO_ROOT/plugins/$NAME"
SKILL_DIR="$PLUGIN_DIR/skills/$NAME"

# ── Idempotence: bail early if the plugin dir already exists ──
if [[ -d "$PLUGIN_DIR" ]]; then
    green "Plugin '$NAME' already exists at plugins/$NAME — nothing to scaffold (idempotent no-op)."
    exit 0
fi

# Default description (kept > 20 chars and behavioral so test.sh is happy).
if [[ -z "$DESCRIPTION" ]]; then
    DESCRIPTION="Release-engineering knowledge for $NAME. This skill should be used when working with $NAME."
fi

# ── Validate description length (schema minLength: 20) — AFTER default-fill ──
# so that omitting --description still uses the valid default, but an explicit
# too-short --description fails fast before any files or registry entries.
if [[ ${#DESCRIPTION} -lt 20 ]]; then
    red "Description too short (${#DESCRIPTION} chars, min 20). Provide a fuller --description."
    exit 1
fi

OWNER="$(whoami)"

# ── Scaffold files ───────────────────────────────────────────
mkdir -p "$SKILL_DIR" "$PLUGIN_DIR/references"

cat > "$SKILL_DIR/SKILL.md" <<EOF
---
name: $NAME
description: $DESCRIPTION
allowed-tools: Read, Grep, Glob
---

# ${NAME}

<!-- Entry point. Keep this file under 500 lines; move detail into references/
     and link to it (progressive disclosure). -->

## What this covers

TODO: one paragraph on the scope of this skill.

## Documentation structure — load on demand

| When you're… | Load |
|---|---|
| TODO | [\`references/example.md\`](references/example.md) |
EOF

cat > "$PLUGIN_DIR/OWNERS" <<EOF
# Plugin owners — these users are required reviewers for changes to this plugin.
# Format: one Bitbucket username per line.
$OWNER
EOF

touch "$PLUGIN_DIR/references/.gitkeep"

green "  ✓ Scaffolded plugins/$NAME (SKILL.md, OWNERS, references/)"

# ── Append to registry (idempotent) ─────────────────────────
SKILL_NAME="$NAME" DESCRIPTION="$DESCRIPTION" CATEGORY="$CATEGORY" \
    REGISTRY="$REGISTRY" SKILL_OWNER="$OWNER" \
    python3 "$REPO_ROOT/scripts/_append_to_registry.py"

# ── Regenerate manifests for both targets ────────────────────
python3 "$REPO_ROOT/scripts/generate-manifests.py"

# ── Validate the new plugin ──────────────────────────────────
green ""
green "Running JSON/registry validation for the new plugin..."
"$REPO_ROOT/scripts/validate-json.sh" "$NAME"

green ""
green "Plugin '$NAME' scaffolded. Next steps:"
echo "  1. Edit plugins/$NAME/skills/$NAME/SKILL.md (real description + body)."
echo "  2. Add references under plugins/$NAME/references/ and link them from SKILL.md."
echo "  3. Review keywords/category/owners in registry/plugins.json."
echo "  4. Re-run: python3 scripts/generate-manifests.py && ./scripts/validate.sh && ./scripts/test.sh"
echo "  5. Commit on a feature branch and open a PR."
