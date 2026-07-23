#!/usr/bin/env bash
#
# bump-version.sh — Bump a plugin's version in registry/plugins.json,
# then regenerate all derived manifests.
#
# Usage:
#   ./scripts/bump-version.sh <plugin-name> <patch|minor|major>
#
# Examples:
#   ./scripts/bump-version.sh <plugin-name> patch   # 0.1.0 → 0.1.1
#   ./scripts/bump-version.sh <plugin-name> minor   # 0.1.0 → 0.2.0
#   ./scripts/bump-version.sh <plugin-name> major   # 0.1.0 → 1.0.0
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REGISTRY="$REPO_ROOT/registry/plugins.json"

green()  { printf '\033[32m%s\033[0m\n' "$*"; }
red()    { printf '\033[31m%s\033[0m\n' "$*"; }

if [[ "${1:-}" == "" || "${2:-}" == "" ]]; then
    red "Usage: $0 <plugin-name> <patch|minor|major>"
    exit 1
fi

PLUGIN_NAME="$1"
BUMP_TYPE="$2"

if [[ "$BUMP_TYPE" != "patch" && "$BUMP_TYPE" != "minor" && "$BUMP_TYPE" != "major" ]]; then
    red "Bump type must be: patch, minor, or major"
    exit 1
fi

if [[ ! -f "$REGISTRY" ]]; then
    red "Registry not found: $REGISTRY"
    exit 1
fi

# Bump in registry, then regenerate manifests.
RESULT=$(PLUGIN_NAME="$PLUGIN_NAME" BUMP_TYPE="$BUMP_TYPE" REGISTRY="$REGISTRY" \
    python3 "$REPO_ROOT/scripts/_bump_registry_version.py")

OLD_VERSION="${RESULT%%|*}"
NEW_VERSION="${RESULT##*|}"

echo "Bumping $PLUGIN_NAME in registry: $OLD_VERSION → $NEW_VERSION ($BUMP_TYPE)"
python3 "$REPO_ROOT/scripts/generate-manifests.py"

green "Updated $PLUGIN_NAME: $OLD_VERSION → $NEW_VERSION"
echo "  registry: $REGISTRY"
echo "  manifests regenerated for both Claude and Codex"
