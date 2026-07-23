# codex-to-claude — mechanism, markers, troubleshooting

## Pipeline

```
Claude SessionStart hook  (hooks/hooks.json)
  → bash scripts/porter-sync.sh --source codex --target claude
      → scripts/porter-build.sh   (ensure the binary is built + cached)
          → cargo build --release   (ONLY on first run or crate-source change)
      → <data>/bin/agent-porter sync --source codex --target claude
```

`<data>` is the first set of `CLAUDE_PLUGIN_DATA`, `PLUGIN_DATA`,
`XDG_CACHE_HOME`, or `$HOME/.cache`, plus `/auto-agent-plugin-porter`. The plugin
install directory itself is treated as read-only, so the binary is never built
there.

## Config-dir resolution (cross-platform)

| Agent | Env override | Default (macOS/Linux) | Default (Windows) |
|---|---|---|---|
| Claude | `CLAUDE_CONFIG_DIR` | `~/.claude` | `%USERPROFILE%\.claude` |
| Codex | `CODEX_HOME` | `~/.codex` | `%USERPROFILE%\.codex` |

Skills are read from `<config>/skills/<name>/SKILL.md`.

## Identity marker

Every generated mirror carries this in its SKILL.md frontmatter:

```yaml
metadata:
  ported_by: auto-agent-plugin-porter
  porter_version: <crate version>
  source_agent: codex
  source_name: <original skill dir name>
  source_hash: <sha-256 of the entire source skill directory>
```

- **`source_hash`** drives the incremental fast path: unchanged hash → skip.
- **`ported_by`** drives loop-safety and non-clobber: the enumerator skips any
  *source* carrying it (so a claude→codex mirror is never re-ported), and the
  writer refuses to overwrite a *target* that lacks it (so your own skills are
  safe).

## Naming

A Codex skill `foo` becomes the Claude skill `codex-foo` (invoked `/codex-foo`).
The prefix guarantees no collision with a native Claude `foo` and makes the
provenance obvious.

## Frontmatter translation

| Concept | Codex source | Claude mirror |
|---|---|---|
| identity | `name`, `description` | `name` (prefixed), `description` |
| auto-invocation off | `agents/openai.yaml` → `policy.allow_implicit_invocation: false` (or `disable-model-invocation: true`) | `disable-model-invocation: true` |
| body + `references/` etc. | copied verbatim | copied verbatim |

## Troubleshooting

- **"Rust toolchain not found"** — install from <https://rustup.rs>, then start a
  new session (or run the manual command in SKILL.md).
- **A skill didn't appear** — confirm it lives at `~/.codex/skills/<name>/SKILL.md`
  with valid `---`-fenced frontmatter. Run the `--dry-run` manual command to see
  the plan and any per-skill warnings.
- **Force a full rebuild of the binary** — delete
  `<data>/auto-agent-plugin-porter/bin/` and start a new session.
- **Stop a skill from being ported** — delete it in Codex (its mirror is pruned),
  or move it out of `~/.codex/skills/`.
- **A mirror you edited reverted** — expected: mirrors are generated. Edit the
  Codex source instead.

## Scope (this release)

- Ports **user-level skills** (`<config>/skills/`). Plugin-bundled skills and
  full plugin components (commands, agents, MCP servers) are **not** ported yet.
- Does **not** yet port hooks across agents.
- These are tracked as follow-ups; the engine is structured to add them.
