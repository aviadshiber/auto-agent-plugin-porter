---
name: claude-to-codex
description: Mirror your Claude Code skills into OpenAI Codex CLI so they work natively here, with zero duplicate authoring. A one-time bootstrap builds a fast porter binary and installs a session-start hook (which you approve once) that keeps skills in sync every session. Use to set up Claude→Codex porting, after adding a Claude skill, to force a re-sync, or to troubleshoot. Requires the Rust toolchain. Auto-invoke on: claude skills missing in codex, port claude to codex, sync claude skills, agent porter bootstrap.
allowed-tools: Bash, Read
metadata:
  compatibility:
    - codex-cli
---

# claude-to-codex

Bring your **Claude Code** skills into **OpenAI Codex CLI** automatically. Author
a skill once in Claude; this plugin mirrors it into Codex so it is available
natively here — no second copy to maintain.

## How it works (and why it needs a one-time bootstrap)

Unlike Claude, the current Codex CLI does not load a session-start hook shipped
inside a plugin manifest, and it gates all hooks behind explicit user trust. So
enabling this is a **one-time bootstrap** that you run once:

1. Builds the `agent-porter` Rust binary (cached under the plugin data dir).
2. Registers a **user-level** `SessionStart` hook in `~/.codex/hooks.json` that
   runs the porter every session. The write is **merge-safe** — it never touches
   your other hooks.
3. Runs an initial sync so your Claude skills appear immediately.

**Codex will then ask you to _trust_ the new hook once** (on the next session, or
run `codex` and approve). This prompt is by design — the porter never bypasses
Codex's hook-trust mechanism. After that, syncing is automatic.

## Run the bootstrap

Ask the agent to run the bundled bootstrap script. If the plugin root is exposed
as `$CODEX_PLUGIN_ROOT`/`$PLUGIN_ROOT`, use it; otherwise discover the script:

```bash
BOOT="$(find "${CODEX_HOME:-$HOME/.codex}/plugins" -name porter-bootstrap.sh -path '*claude-to-codex*' 2>/dev/null | head -1)"
bash "$BOOT"
```

(On Windows, run `porter-bootstrap.ps1` the same way.)

## What the sync does

For each user skill in Claude (`$CLAUDE_CONFIG_DIR/skills/<name>/`, default
`~/.claude`), it writes a mirror into your Codex skills directory
(`$CODEX_HOME/skills`, default `~/.codex`) as `claude-<name>/` — the whole skill
directory, with the frontmatter translated to Codex's dialect. An optional
`agents/openai.yaml` is generated only when needed to preserve a non-default
invocation policy (Claude's `disable-model-invocation` becomes
`policy.allow_implicit_invocation: false`).
For large collections, generated descriptions share a fair 8,000-character
budget so they fit alongside native and plugin skills in Codex's skills-context
allocation. The sync reports compaction when it writes affected mirrors.

The sync is one-way (Claude → Codex), **hash-gated** (only effective generated
changes are rewritten), **loop-safe** (mirrors carry a
`metadata.ported_by` marker and are
never re-ported), **non-destructive** (never overwrites a Codex skill it did not
create; mirrors are `claude-` prefixed), and **self-pruning** (deleting a Claude
skill removes its Codex mirror next sync).

## Requirements

- **Rust toolchain** (`cargo`) — the binary is built once and cached; the
  steady-state hook does not invoke `cargo`. Install from <https://rustup.rs>.
- Claude Code with skills under `$CLAUDE_CONFIG_DIR/skills` (default
  `~/.claude`).

## Documentation — load on demand

| When you're… | Load |
|---|---|
| Troubleshooting, or want the full mechanism / trust / marker details | [`references/mechanism.md`](references/mechanism.md) |
