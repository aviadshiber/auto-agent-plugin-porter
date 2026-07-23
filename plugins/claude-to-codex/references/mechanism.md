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
  render_hash: <sha-256 of the effective generated mirror inputs>
```

`render_hash` is the sole incremental fast-path key and covers the copied
files, body, translated policy, and compacted description. It is computed from
the same typed render plan used by the writer, so there is no separate hash
contract that can drift from generated output. A change beyond a compacted
description's visible prefix is a true no-op, while any effective output change
forces a rewrite. `ported_by` drives loop-safety (skip already-ported sources)
and non-clobber (never overwrite a Codex skill we did not create). A Claude
skill `foo` becomes the Codex skill `claude-foo`.

## Frontmatter translation

| Concept | Claude source | Codex mirror |
|---|---|---|
| identity | `name`, `description` | `name` (prefixed), `description` |
| auto-invocation off | `disable-model-invocation: true` | `agents/openai.yaml` → `policy.allow_implicit_invocation: false` |
| body + `references/` etc. | copied verbatim | copied verbatim |

`agents/openai.yaml` is optional and is emitted only for the non-default
auto-invocation-off case. Ordinary Codex mirrors use `SKILL.md` alone. Keeping
this metadata sparse reduces the number of files Codex opens concurrently when
it hot-reloads a large user skill collection.

Codex exposes complete skill entries under a dynamic percentage of the active
model context. The porter cannot observe that model, context window, or the
native/plugin skills sharing the allocation from a session hook. It therefore
uses a best-effort 8,000-character **soft target for generated, implicitly
invokable descriptions**, configurable with the positive-integer environment variable
`AGENT_PORTER_CODEX_DESCRIPTION_TARGET_CHARS`. This reduces pressure but does
not guarantee that Codex will avoid its own shortening warning. Manual-only
skills are excluded because Codex does not put them in the implicit-discovery
catalog.

The target is distributed fairly: short descriptions keep their full text,
while longer descriptions receive equal remaining shares. At least one
model-visible character is retained per valid skill, so a corpus larger than
the configured target may exceed that soft target. If a source is malformed,
the description size of its retained porter-owned mirror is reserved before
allocating the rest. The compacted description participates in `render_hash`,
so adding or removing a source skill updates only existing mirrors whose
effective allocation changes. When sync writes a compacted mirror, it reports
how many compacted mirrors were written now plus the current corpus-wide
shortened count, before/after totals, soft target, and retained malformed-mirror
characters; ordinary no-op session starts remain quiet.

## Troubleshooting

- **Nothing syncs after install** — you must run the bootstrap once (see
  SKILL.md), then approve the hook when Codex prompts.
- **"Rust toolchain not found"** — install from <https://rustup.rs> and re-run
  the bootstrap.
- **Hook not firing** — confirm `~/.codex/hooks.json` has a `SessionStart` entry
  whose command ends in `--source claude --target codex`, and that you approved
  the trust prompt.
- **After a plugin upgrade** — re-run the bootstrap to rebuild + refresh.
- **Codex reports `Too many open files` or shortened skill descriptions after
  upgrading from an earlier release** — run the bootstrap again so porter `0.2.0+`
  rebuilds the mirrors with sparse Codex metadata and budgeted descriptions,
  then restart Codex once. If Codex still shortens the complete catalog, lower
  `AGENT_PORTER_CODEX_DESCRIPTION_TARGET_CHARS` and bootstrap again.
- **Remove the hook** — edit `~/.codex/hooks.json` and delete the porter
  `SessionStart` entry.

## Scope (this release)

- Ports **user-level skills** (`<config>/skills/`). Plugin-bundled skills and
  full plugin components (commands, agents, MCP servers) are **not** ported yet.
- Does **not** yet port hooks across agents.
- These are tracked as follow-ups; the engine is structured to add them.
