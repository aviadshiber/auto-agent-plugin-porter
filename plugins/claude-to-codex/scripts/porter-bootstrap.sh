#!/usr/bin/env bash
#
# porter-bootstrap.sh — one-time setup for the claude-to-codex direction (Unix).
#
# VENDORED, IDENTICAL in every porter plugin. Run once (by the claude-to-codex
# skill, or manually) from INSIDE Codex to enable automatic porting of Claude
# skills into Codex:
#   1. Build the porter binary (cached under the plugin data dir).
#   2. Register a user-level SessionStart hook in ~/.codex/hooks.json that runs
#      the (stable, cached) binary each session. Merge-safe; never clobbers
#      other hooks. Codex will ask you to TRUST the hook once — that prompt is
#      by design; the porter never bypasses hook trust.
#   3. Do an initial sync now so Claude's skills appear immediately.
#
# Self-locating: derives its own plugin root from $BASH_SOURCE, so it works no
# matter where it is invoked from.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=porter-build.sh
. "$SCRIPT_DIR/porter-build.sh"

PLUGIN_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

if ! porter_ensure_built "$PLUGIN_ROOT"; then
  echo "agent-porter: cannot bootstrap without a built binary (install Rust from https://rustup.rs)." >&2
  exit 1
fi

echo "agent-porter: registering the Codex session-start hook…"
"$PORTER_BIN" install-codex-hook --porter-bin "$PORTER_BIN"

echo "agent-porter: running an initial Claude → Codex sync…"
"$PORTER_BIN" sync --source claude --target codex

cat >&2 <<'EOF'

Bootstrap complete. Claude Code skills are now mirrored into Codex, and a
session-start hook will keep them in sync automatically.

One-time step: Codex will prompt you to TRUST the new hook on your next session
(or run `codex` and approve it). This is expected — the porter never bypasses
Codex's hook-trust mechanism. After you upgrade this plugin, re-run this
bootstrap to rebuild the binary and refresh the hook.
EOF
