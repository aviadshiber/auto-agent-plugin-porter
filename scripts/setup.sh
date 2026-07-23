#!/usr/bin/env bash
#
# setup.sh — One-time setup for new contributors.
#
# Activates git hooks so validation runs automatically on commit/push.
# Run this after cloning the repo.
#
# Usage:
#   ./scripts/setup.sh
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

green() { printf '\033[32m%s\033[0m\n' "$*"; }

echo "Setting up marketplace..."

# Activate custom git hooks
git -C "$REPO_ROOT" config core.hooksPath .githooks
green "  ✓ Git hooks activated (.githooks/)"

# Ensure symlinks survive checkout (in case CLAUDE.md is used in symlink form).
git -C "$REPO_ROOT" config core.symlinks true
green "  ✓ Symlink support enabled"

echo ""
green "Setup complete. The following hooks are now active:"
echo "  pre-commit  — Runs validation, blocks commits on main/master, checks SKILL.md line limits"
echo "  pre-push    — Blocks direct pushes to main/master"
echo "  commit-msg  — Enforces conventional commit format"
echo ""
echo "To bypass in emergencies: git commit --no-verify"
