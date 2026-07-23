#!/usr/bin/env bash
# NOTE: direction flipped vs. the usual convention — AGENTS.md is canonical, CLAUDE.md is a thin @AGENTS.md pointer.
#
# check-instructions-sync.sh — Enforce that AGENTS.md is the single canonical
# contributor-practices doc and CLAUDE.md merely points to it.
#
# Contract (the OPPOSITE of the prototype, which made CLAUDE.md canonical):
#   - AGENTS.md MUST be a real regular file (Codex reads it natively).
#   - CLAUDE.md MUST point to AGENTS.md, in one of two accepted forms:
#       (a) a symlink whose target is AGENTS.md, OR
#       (b) a regular file whose first non-empty line is the import `@AGENTS.md`
#           (the cross-platform-preferred form; Claude Code does not discover
#            AGENTS.md natively, so this import is what loads the practices).
#
# Wired into pre-commit and validate.sh — runs on every commit.
set -euo pipefail
cd "$(dirname "$0")/.."

# ── AGENTS.md must be a real regular file (the canonical doc) ──────────────
if [[ -L AGENTS.md ]]; then
  echo "ERROR: AGENTS.md must be a regular file (the canonical doc), not a symlink." >&2
  echo "       CLAUDE.md is the pointer → AGENTS.md, never the other way around." >&2
  exit 1
fi
if [[ ! -f AGENTS.md ]]; then
  echo "ERROR: AGENTS.md is missing — it is the canonical contributor-practices doc." >&2
  exit 1
fi

# ── CLAUDE.md must point to AGENTS.md ─────────────────────────────────────
if [[ ! -e CLAUDE.md && ! -L CLAUDE.md ]]; then
  echo "ERROR: CLAUDE.md is missing — it must point to AGENTS.md." >&2
  echo "Restore with one of:" >&2
  echo "  printf '@AGENTS.md\\n' > CLAUDE.md            # import form (preferred)" >&2
  echo "  ln -s AGENTS.md CLAUDE.md                     # symlink form" >&2
  exit 1
fi

if [[ -L CLAUDE.md ]]; then
  # Symlink form: target must be AGENTS.md.
  target=$(readlink CLAUDE.md)
  if [[ "$target" != "AGENTS.md" ]]; then
    echo "ERROR: CLAUDE.md symlink points to '$target', expected 'AGENTS.md'." >&2
    echo "Restore with: rm CLAUDE.md && ln -s AGENTS.md CLAUDE.md" >&2
    exit 1
  fi
  if [[ ! -e CLAUDE.md ]]; then
    echo "ERROR: CLAUDE.md → AGENTS.md is a broken symlink." >&2
    exit 1
  fi
  echo "instructions-sync: OK (CLAUDE.md is a symlink → AGENTS.md)"
  exit 0
fi

# Import form: first non-empty line must be exactly `@AGENTS.md`.
first_line=$(grep -m1 -vE '^[[:space:]]*$' CLAUDE.md || true)
# Trim leading/trailing whitespace.
first_line="${first_line#"${first_line%%[![:space:]]*}"}"
first_line="${first_line%"${first_line##*[![:space:]]}"}"
if [[ "$first_line" != "@AGENTS.md" ]]; then
  echo "ERROR: CLAUDE.md must point to AGENTS.md." >&2
  echo "       Its first non-empty line must be the import '@AGENTS.md' (got: '$first_line')." >&2
  echo "Restore with: printf '@AGENTS.md\\n' > CLAUDE.md" >&2
  exit 1
fi

echo "instructions-sync: OK (CLAUDE.md imports @AGENTS.md)"
exit 0
