# claude-to-codex — mechanism, trust, markers, troubleshooting

## Why a bootstrap instead of a plugin hook

On the Codex CLI verified during design (0.144.1):

- The plugin-manifest validator **rejects** a `hooks` key in
  `.codex-plugin/plugin.json`, and plugin-bundled hook auto-discovery is
  unverified.
- Hooks are gated behind **explicit trust** (a persisted hash). Installing or
  enabling a plugin does **not** trust its hooks.

So the reliable, honest path is a one-time bootstrap that registers a
**user-level** `SessionStart` hook in `~/.codex/hooks.json` (the same file Codex
already honors for user hooks) and lets Codex prompt you to trust it. The porter
**never** passes `--dangerously-bypass-hook-trust` or writes the trust store —
that would be silent arbitrary-code execution.

## Pipeline

```
Bootstrap (once):  scripts/porter-bootstrap.sh
  → scripts/porter-build.sh                      (build + cache the binary)
  → agent-porter install-codex-hook --porter-bin <cached binary>
        → merge a SessionStart entry into ~/.codex/hooks.json:
            "<cached binary>" sync --source claude --target codex
  → agent-porter sync --source claude --target codex   (initial sync)

Every session after trust:  the user-level hook runs the cached binary directly.
```

The hook points at the **cached binary** (a stable path under the plugin data
dir), not at a versioned plugin script — so it keeps working across sessions.
After you **upgrade the plugin**, re-run the bootstrap to rebuild the binary and
refresh the hook.

## Config-dir resolution (cross-platform)

| Agent | Env override | Default (macOS/Linux) | Default (Windows) |
|---|---|---|---|
| Claude | `CLAUDE_CONFIG_DIR` | `~/.claude` | `%USERPROFILE%\.claude` |
| Codex | `CODEX_HOME` | `~/.codex` | `%USERPROFILE%\.codex` |

## Identity marker

Every generated Codex mirror carries this in its SKILL.md frontmatter:

```yaml
metadata:
  ported_by: auto-agent-plugin-porter
  porter_version: <crate version>
  source_agent: claude
  source_name: <original skill dir name>
  source_hash: <sha-256 of the entire source skill directory>
```

`source_hash` drives the incremental fast path; `ported_by` drives loop-safety
(skip already-ported sources) and non-clobber (never overwrite a Codex skill we
did not create). A Claude skill `foo` becomes the Codex skill `claude-foo`.

## Frontmatter translation

| Concept | Claude source | Codex mirror |
|---|---|---|
| identity | `name`, `description` | `name` (prefixed), `description` |
| auto-invocation off | `disable-model-invocation: true` | `agents/openai.yaml` → `policy.allow_implicit_invocation: false` |
| body + `references/` etc. | copied verbatim | copied verbatim |

## Troubleshooting

- **Nothing syncs after install** — you must run the bootstrap once (see
  SKILL.md), then approve the hook when Codex prompts.
- **"Rust toolchain not found"** — install from <https://rustup.rs> and re-run
  the bootstrap.
- **Hook not firing** — confirm `~/.codex/hooks.json` has a `SessionStart` entry
  whose command ends in `--source claude --target codex`, and that you approved
  the trust prompt.
- **After a plugin upgrade** — re-run the bootstrap to rebuild + refresh.
- **Remove the hook** — edit `~/.codex/hooks.json` and delete the porter
  `SessionStart` entry.

## Scope (this release)

- Ports **user-level skills** (`<config>/skills/`). Plugin-bundled skills and
  full plugin components (commands, agents, MCP servers) are **not** ported yet.
- Does **not** yet port hooks across agents.
- These are tracked as follow-ups; the engine is structured to add them.
