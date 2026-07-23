---
name: codex-to-claude
description: Mirror your OpenAI Codex CLI skills into Claude Code so they work natively here, with zero duplicate authoring. Runs automatically on every Claude session start via a bundled hook; also invokable manually. Use when your Codex skills are missing in Claude, after adding a Codex skill, to force a re-sync, or to understand/troubleshoot the porter. Requires the Rust toolchain (a fast porter binary is built once on first run). Auto-invoke on: codex skills missing in claude, port codex to claude, sync codex skills, agent porter.
allowed-tools: Bash, Read
metadata:
  compatibility:
    - claude-code
---

# codex-to-claude

Bring your **OpenAI Codex CLI** skills into **Claude Code** automatically. You
author a skill once in Codex; this plugin mirrors it into Claude on every
session start, so it is available natively — no second copy to maintain.

## What it does

On Claude `SessionStart`, a bundled hook runs the `agent-porter` binary:

```
agent-porter sync --source codex --target claude
```

For each user skill in Codex (`$CODEX_HOME/skills/<name>/`, default `~/.codex`),
it writes a mirror into your Claude skills directory (`$CLAUDE_CONFIG_DIR/skills`,
default `~/.claude`) as `codex-<name>/` — the whole skill directory (SKILL.md +
`references/`, `scripts/`, `assets/`), with the frontmatter translated to
Claude's dialect. The Codex `agents/openai.yaml` invocation policy
(`allow_implicit_invocation`) becomes Claude's `disable-model-invocation`.

The sync is:

- **One-way** (Codex → Claude). The mirror is *generated*; never hand-edit it —
  edit the source skill in Codex and let the next session re-sync.
- **Hash-gated** — a skill is rewritten only when its source content changes, so
  the steady-state session-start cost is negligible.
- **Loop-safe** — every mirror carries a `metadata.ported_by` marker. The porter
  skips any source already carrying it, so a skill ported the other way
  (claude → codex) is never ported back into a duplicate.
- **Non-destructive** — the porter never overwrites a Claude skill it did not
  create. Your own hand-authored Claude skills are untouched. Mirrors are
  namespaced with a `codex-` prefix so they cannot collide.
- **Self-pruning** — delete a Codex skill and its Claude mirror is removed on the
  next sync (pass `--no-prune` to keep it).

## Requirements

- **Rust toolchain** (`cargo`). The porter is a small Rust binary, built once
  under the plugin data dir on first run (or when its source changes) and cached
  thereafter — the steady-state hook does not invoke `cargo`. Install from
  <https://rustup.rs>. If Rust is absent the hook prints a note and exits
  cleanly (your session is never blocked).
- OpenAI Codex CLI installed with skills under `$CODEX_HOME/skills` (default
  `~/.codex`).

## Manual use

The session-start hook is automatic, but you can drive the porter directly:

```bash
# Dry run: show what would change, write nothing
bash "${CLAUDE_PLUGIN_ROOT}/scripts/porter-sync.sh" --source codex --target claude --dry-run

# Force a sync now
bash "${CLAUDE_PLUGIN_ROOT}/scripts/porter-sync.sh" --source codex --target claude
```

## Documentation — load on demand

| When you're… | Load |
|---|---|
| Troubleshooting, or want the full mechanism / marker format / platform notes | [`references/mechanism.md`](references/mechanism.md) |
