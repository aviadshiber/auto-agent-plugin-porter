# auto-agent-plugin-porter

**Author a skill once — use it in both Claude Code and OpenAI Codex CLI.**

This marketplace ships two porter plugins that keep your agent skills in sync
across both CLIs automatically, from a single source of truth, with no duplicate
authoring:

| Plugin | Install it in… | On session start it… |
|---|---|---|
| **`codex-to-claude`** | Claude Code | mirrors your **Codex** skills into Claude |
| **`claude-to-codex`** | OpenAI Codex CLI | mirrors your **Claude** skills into Codex |

Each porter runs the *other* agent's skills into the one you're using, so you
edit a skill in one place and it shows up natively in both. You typically install
only the porter for the agent you use most (or both, one in each CLI — the sync
is loop-safe, see below).

## Install

**Claude Code** — bring your Codex skills in:

```
/plugin marketplace add aviadshiber/auto-agent-plugin-porter
/plugin install codex-to-claude@auto-agent-plugin-porter
```

**OpenAI Codex CLI** — bring your Claude skills in:

```
codex plugin marketplace add aviadshiber/auto-agent-plugin-porter
codex plugin add claude-to-codex@auto-agent-plugin-porter
```

Then, inside Codex, run the skill once (`claude-to-codex`) to bootstrap — it
builds the porter and installs a session-start hook you approve once (see below).

## Two things to know before you install

1. **The Rust toolchain is required.** The porter is a small Rust binary that
   runs at every session start, so it must be fast — it is built **once** on
   first run (cached thereafter; the steady-state path never invokes `cargo`).
   Install from <https://rustup.rs>. If Rust is missing, the porter prints a note
   and exits cleanly — your session is never blocked.
2. **The Codex direction needs a one-time trust.** Codex gates hooks behind
   explicit user trust and (on current versions) does not load a hook shipped
   inside a plugin manifest. So `claude-to-codex` registers a **user-level**
   session-start hook and Codex prompts you to **trust it once**. The porter
   never bypasses that trust. The Claude direction (`codex-to-claude`) is
   seamless — a bundled hook runs automatically with no extra step.

## How it works

Each porter runs `agent-porter sync --source <a> --target <b>` at session start.
For every user skill in the source agent (`<config>/skills/<name>/`) it writes a
mirror into the target agent, translating the frontmatter to the target's
dialect (Claude `disable-model-invocation` ⇄ Codex
`agents/openai.yaml: policy.allow_implicit_invocation`) and copying the rest of
the skill directory verbatim.

The sync is safe by construction:

- **One-way & generated.** The mirror is a build artifact — never hand-edit it;
  edit the source skill and let the next session re-sync.
- **Hash-gated.** A skill is rewritten only when its source content changes, so
  the steady-state session-start cost is negligible.
- **Loop-safe.** Every mirror carries a `metadata.ported_by` marker; the porter
  skips any source already carrying it, so installing *both* directions never
  creates an A→B→A duplicate spiral.
- **Non-destructive.** The porter never overwrites a skill it did not create;
  your own hand-authored skills are untouched. Mirrors are namespaced with a
  `codex-` / `claude-` prefix so they cannot collide.
- **Self-pruning.** Delete a source skill and its mirror is removed next sync.
- **Cross-platform.** Config dirs are resolved from `CLAUDE_CONFIG_DIR` /
  `CODEX_HOME` (falling back to `~/.claude` / `~/.codex`, or `%USERPROFILE%` on
  Windows).

## Scope of this release

- Ports **user-level skills**. Plugin-bundled skills and full plugin components
  (commands, agents, MCP servers) and **hook porting** are planned follow-ups;
  the engine is structured to add them.
- Windows: the Rust binary and PowerShell wrappers are cross-platform; the
  Claude hook wiring is verified on macOS/Linux and uses the bash wrapper (Git
  Bash on Windows).

## For contributors

This is a **dual-target marketplace**: every plugin is authored once as a
tool-agnostic IR (`registry/plugins.json`) and compiled into per-target
artifacts for both CLIs. The Rust porter lives once at `porter/` and is vendored
into each plugin by `scripts/sync-porter.sh`. See [`AGENTS.md`](AGENTS.md) for
the full contributor guide, and the per-plugin `references/mechanism.md` for the
porter internals. CI (`.github/workflows/ci.yml`) runs the marketplace gates
plus the Rust gates (`fmt`, `clippy -D warnings`, `test`, `build`).

## License

MIT — see [`LICENSE`](LICENSE).
