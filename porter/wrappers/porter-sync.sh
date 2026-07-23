#!/usr/bin/env bash
#
# porter-sync.sh — session-start wrapper (Unix / macOS / Git-Bash).
#
# VENDORED, IDENTICAL in every porter plugin (synced from porter/wrappers/ by
# scripts/sync-porter.sh; CI drift-checks the copies). The sync DIRECTION is not
# baked in here — it is passed as args by the plugin's hooks.json, e.g.
#   bash "${CLAUDE_PLUGIN_ROOT}/scripts/porter-sync.sh" --source codex --target claude
#
# Ensures the binary is built (cached; see porter-build.sh) then execs it. The
# session is never blocked: any build failure exits 0.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=porter-build.sh
. "$SCRIPT_DIR/porter-build.sh"

# Plugin root: Claude sets CLAUDE_PLUGIN_ROOT, Codex sets PLUGIN_ROOT (+ a
# CLAUDE_PLUGIN_ROOT alias). Fall back to this script's parent dir.
PLUGIN_ROOT="${CLAUDE_PLUGIN_ROOT:-${PLUGIN_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}}"

if ! porter_ensure_built "$PLUGIN_ROOT"; then
  echo "agent-porter: skipping porting this session." >&2
  exit 0
fi

exec "$PORTER_BIN" sync "$@"
