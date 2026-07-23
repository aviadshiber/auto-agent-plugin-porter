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
the skill directory verbatim. Codex metadata is sparse by design: ordinary
skills need only `SKILL.md`; `agents/openai.yaml` is emitted only when it must
carry a non-default invocation policy. Generated, implicitly invokable Codex
descriptions share an 8,000-character soft target, distributed fairly across
the mirrored discovery corpus, to reduce pressure on Codex's dynamic
skills-context allocation. Manual-only skills are excluded because Codex does
not expose them for implicit discovery. This is not a guarantee: Codex budgets
the complete active skill catalog as a percentage of the current model context,
which the session hook cannot observe. Override the porter target with
`AGENT_PORTER_CODEX_DESCRIPTION_TARGET_CHARS`.

The sync is safe by construction:

- **One-way & generated.** The mirror is a build artifact — never hand-edit it;
  edit the source skill and let the next session re-sync.
- **Hash-gated.** A skill is rewritten only when its effective generated output
  changes. The render hash covers copied files, the body, translated policy,
  and the budgeted description. Adding or removing a sibling skill therefore
  re-renders only mirrors whose fair share changed, while an edit beyond an
  already-compacted description prefix remains a true no-op.
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

After upgrading from an earlier release, run the `claude-to-codex` bootstrap
once more. The `0.2.0` porter re-renders existing mirrors with sparse Codex
metadata, a single output-derived hash, and configurable compacted
descriptions, reducing file-descriptor and skills-context pressure when Codex
reloads a large skill collection. When compaction causes mirror writes, the
sync warns with the number of compacted mirrors written now plus the current
corpus-wide shortened count, before/after character totals, soft target, and
any budget retained for malformed sources; unchanged session starts stay quiet.

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
