# <MARKETPLACE_NAME>-marketplace — Contributor Guide

Canonical practices for this repo. Read this before adding or changing a plugin.

> This is the single source of contributor practices. `CLAUDE.md` is a thin
> pointer: it contains only `@AGENTS.md` so Claude Code loads this file. Claude
> Code does **not** discover `AGENTS.md` natively (Codex CLI does), so the
> import in `CLAUDE.md` is what makes this file reach Claude. Keep the
> practices here, not in `CLAUDE.md`.

## Bootstrap check

If you have not run it in this clone yet:

```bash
./scripts/setup.sh          # activates .githooks and symlink support
```

This wires the pre-commit / pre-push / commit-msg hooks. Hooks are a local
convenience and are bypassable (`git commit --no-verify`) and absent for web
edits — the real merge gate is GitHub Actions (`.github/workflows/ci.yml`).

## What this repo is

A **dual-target plugin marketplace**: each plugin is authored once as a
tool-agnostic intermediate representation (IR) and compiled into per-target
artifacts for **both Claude Code and OpenAI Codex CLI**. The same knowledge
then serves users of either agent.

```
registry/plugins.json     ← IR: the sole source of truth (marketplace + plugins[])
registry/schema.json      ← JSON Schema (draft 2020-12) validating the IR
        │
        ▼  scripts/generate-manifests.py   (pure, idempotent, --check drift gate)
        ├─ .claude-plugin/marketplace.json            (Claude catalog, ascii)
        ├─ .agents/plugins/marketplace.json           (Codex catalog; skips claude_only)
        ├─ plugins/<name>/.claude-plugin/plugin.json  (Claude manifest; skipped if codex_only)
        ├─ plugins/<name>/.codex-plugin/plugin.json   (Codex manifest; skipped if claude_only)
        └─ stamps plugins/<name>/skills/<name>/SKILL.md → metadata.compatibility
```

Never hand-edit the generated files. Edit `registry/plugins.json` and re-run
the generator.

## Branch workflow

**No commits to `master`.** The hooks block it locally; branch permissions
enforce it on the server.

```bash
git checkout -b feat/<description>
# ... make changes, run the generator + validators ...
git commit        # conventional-commit message; hooks validate the tree
git push -u origin feat/<description>
# open a PR → OWNERS review → a human merges
```

Commit messages follow **conventional commits** (`feat:`, `fix:`, `docs:`,
`refactor:`, `chore:`, `test:`, `ci:`, `style:`, `perf:`, `revert:`), enforced
by the `commit-msg` hook.

## Plugin ownership

Each plugin has an `OWNERS` file at its root — one Bitbucket username per line.
Owners are the required reviewers for changes to that plugin. Wire them as
Code Experts / required reviewers in Bitbucket branch permissions.

```
# plugins/<name>/OWNERS
# Plugin owners — these users are required reviewers for changes to this plugin.
# Format: one Bitbucket username per line.
<OWNER_USERNAME>
```

## Repository structure

```
registry/
  plugins.json                    # IR — the only file you edit for metadata/version
  schema.json                     # JSON Schema enforcing the IR shape
scripts/
  generate-manifests.py           # IR → all manifests + catalogs (idempotent; --check)
  validate.sh                     # quality checks (paths, secrets, drift, pointer, …)
  validate-json.sh                # JSON syntax + schema + cross-file consistency + registry schema
  test.sh                         # SKILL.md frontmatter, shell syntax, links, pytest
  new-plugin.sh                   # scaffold a new plugin (see "Adding a new plugin")
  bump-version.sh                 # bump a plugin's version in the registry, then regenerate
  bootstrap-registry.py           # rebuild the registry from generated artefacts (maintenance)
  check-instructions-sync.sh      # enforce AGENTS.md canonical + CLAUDE.md pointer
  setup.sh                        # activate git hooks
  _append_to_registry.py          # helper (env-arg, injection-safe)
  _bump_registry_version.py       # helper (env-arg, injection-safe)
  _is_claude_only.py              # helper (env-arg, injection-safe)
  _is_codex_only.py               # helper (env-arg, injection-safe)
  tests/                          # pytest unit tests for the tooling
.githooks/{pre-commit,pre-push,commit-msg}
.github/workflows/ci.yml          # server-side CI merge gate (Python + Rust)
porter/                           # canonical Rust porter crate + wrappers (vendored into plugins)
scripts/sync-porter.sh            # vendor porter/ into each plugin (--check = drift gate)
plugins/<name>/
  .claude-plugin/plugin.json      # GENERATED — Claude manifest (absent if codex_only)
  .codex-plugin/plugin.json       # GENERATED — Codex manifest (absent if claude_only)
  OWNERS                          # required reviewers
  references/                     # detailed docs, loaded on demand
  skills/<name>/SKILL.md          # entry point (< 500 lines; progressive disclosure)
.claude-plugin/marketplace.json   # GENERATED — Claude catalog
.agents/plugins/marketplace.json  # GENERATED — Codex catalog
```

## Versioning — single source

The version lives in **one place**: the plugin's `version` in
`registry/plugins.json`. Both manifests and both catalogs are regenerated from
it, so the two-file version-sync problem does not exist. Never hand-edit a
version in a generated `plugin.json` / `marketplace.json`.

```bash
./scripts/bump-version.sh <plugin-name> <patch|minor|major>   # bumps + regenerates
```

## IR schema — `registry/plugins.json`

`registry/schema.json` (JSON Schema draft 2020-12) is the enforced source of
truth; this section is the human summary. `scripts/validate-registry.py`
validates the IR against the schema (run via `validate-json.sh` §5 and CI).

**`marketplace`** (object, required):

| Field | Required | Notes |
|---|---|---|
| `name` | ✓ | `^[a-z][a-z0-9-]*$`, ≤ 64 chars — the catalog/marketplace id |
| `owner` | ✓ | `{ name, email }` |
| `description` | ✓ | 20–1024 chars |
| `version` | ✓ | semver `X.Y.Z` |
| `pluginRoot` | ✓ | must start with `./` (conventionally `./plugins`) |

**`plugins[]`** (array, ≥ 1) — each entry:

| Field | Required | Notes |
|---|---|---|
| `name` | ✓ | `^[a-z][a-z0-9-]*$`, must equal the plugin directory name |
| `version` | ✓ | semver `X.Y.Z` |
| `description` | ✓ | 20–1536 chars — feeds both catalog entry and both plugin manifests |
| `category` | ✓ | enum: `documentation`, `debugging`, `devops`, `analytics`, `testing`, `monitoring`, `development` |
| `keywords` | ✓ | non-empty, unique strings |
| `owners` | ✓ | non-empty, unique strings (Bitbucket usernames) |
| `claude_only` | — | `true` ⇒ skip all Codex artefacts + stamp `compatibility: [claude-code]` |
| `codex_only` | — | `true` ⇒ skip all Claude artefacts + stamp `compatibility: [codex-cli]`. Mutually exclusive with `claude_only` |
| `lspServers` | — | dormant here; schema retained for extensibility |
| `manifest_description` | — | override the description used in the plugin manifests only |
| `manifest_lspServers` | — | per-manifest lspServers override |
| `manifest_unicode` | — | `true` ⇒ emit the Claude `plugin.json` with literal non-ASCII |

The `category` enum is the **internal** vocabulary. The generator maps it to
the Codex Title-Case vocabulary when emitting Codex artefacts (see
"Dual-format / Codex-compat" below) — you never write the Codex category by hand.

## Adding a new plugin

1. **Scaffold** it:

   ```bash
   ./scripts/new-plugin.sh <name> \
       --category documentation \
       --description "One line, ≥ 20 chars, behavioral (say when to use it)."
   ```

   This creates `plugins/<name>/skills/<name>/SKILL.md`, `OWNERS`, and
   `references/.gitkeep`; appends the registry entry; regenerates all
   manifests; and runs `validate-json.sh`. It is idempotent.

2. **Edit** `plugins/<name>/skills/<name>/SKILL.md` — real description + body,
   under 500 lines, with detail pushed into `references/*.md`.

3. **Review** the registry entry (`keywords`, `category`, `owners`).

4. **Regenerate and validate** (see below), then commit on a feature branch and
   open a PR.

## Validate locally (the gates CI runs)

Install the CI deps once (a venv is recommended on macOS/PEP-668 systems):

```bash
pip install 'jsonschema>=4.21,<5' 'pytest>=8,<9'   # or inside a venv
```

Then, from the repo root:

```bash
python3 scripts/generate-manifests.py            # emit artefacts
python3 scripts/generate-manifests.py --check     # exit 0 = no drift
python3 scripts/validate-registry.py              # registry ↔ schema
python3 -m pytest scripts/tests -q                # tooling unit tests
./scripts/validate.sh                             # quality checks + drift + pointer
./scripts/validate-json.sh                        # JSON schema + consistency + registry
./scripts/test.sh                                 # SKILL.md, shell, links, pytest
./scripts/check-instructions-sync.sh              # AGENTS.md canonical, CLAUDE.md → AGENTS.md
```

`git diff --exit-code` after a second `generate-manifests.py` run must be clean
(idempotence contract).

## SKILL.md authoring rules

- **Size / structure:** keep `SKILL.md` under 500 lines (pre-commit enforces).
  Use three-tier progressive disclosure — entry point in `SKILL.md`, detail in
  `references/*.md`, load on demand.
- **Frontmatter:** `name` (must match the directory), `description`. The
  description is behavioral (say *when* to use the skill / "Auto-invoke on: …")
  and 20–1536 chars. The generator manages `metadata.compatibility` — do not
  hand-edit it.
- **Least privilege:** set `allowed-tools` to the minimum the skill needs
  (e.g. `Read, Grep, Glob` for a docs skill).
- **Env-agnostic paths:** never hardcode `/Users/...`, `~/.claude/...`, or
  `~/git/...`. Use `${CLAUDE_PLUGIN_ROOT}/...` for in-plugin files and
  `${CLAUDE_HOME:-$HOME/.claude}` / `${CLAUDE_GIT:-$HOME/git}` in scripts.
- **Breadcrumbs over hardcoding:** point at discovery commands / sources of
  truth for volatile facts rather than pasting values that will drift.

## Plugins must be self-contained

A plugin may only reference files inside its own directory (no cross-plugin
`../` traversal — plugins are cached independently). `validate.sh` checks that
every `${CLAUDE_PLUGIN_ROOT}/...` reference resolves and that no `../`
traversal escapes the plugin. External tool dependencies must be documented in
the SKILL.md, not assumed.

## Dual-format / Codex compatibility

**Portable core:** both Claude Code and Codex consume the open Agent-Skills
`SKILL.md` standard (`name` + `description` + body) plus MCP. Compilation is
mostly manifest generation + per-target serialization.

- **Compiles to both targets** by default: skills, MCP.
- **Claude-only** (set `claude_only: true`): subagents, `lspServers`, monitors,
  themes, output-styles. The generator then skips the Codex catalog entry and
  `.codex-plugin/plugin.json`, and stamps the SKILL.md as `[claude-code]` only.
- **Codex-only** (set `codex_only: true`): a plugin that only makes sense inside
  Codex CLI — e.g. the `claude-to-codex` porter, which runs *in Codex* to import
  Claude's skills. The generator then skips the Claude catalog entry and
  `.claude-plugin/plugin.json`, and stamps the SKILL.md as `[codex-cli]` only.
  `claude_only` and `codex_only` are mutually exclusive (the generator and the
  registry schema both reject a plugin that sets both).

**Codex schema specifics** the generator handles for you (verified against
OpenAI's curated Codex marketplace — do not change without re-verifying via
`codex plugin marketplace add`):

- Catalog entry `policy` = `{ installation: "AVAILABLE", authentication:
  "ON_USE", products: ["CODEX"] }`. `ON_FIRST_USE` is **invalid** and Codex
  rejects it (the valid enum is `ON_USE` / `ON_INSTALL`).
- Category is emitted in Codex's **Title-Case** vocabulary. The internal→Codex
  map lives in `generate-manifests.py` (`CODEX_CATEGORY`): `analytics` →
  `Data & Analytics`, every other known category → `Developer Tools`, unknown
  → `Other`. Applied to both the catalog entry `category` and the plugin
  manifest `interface.category`.
- The Codex marketplace manifest is `.agents/plugins/marketplace.json`; each
  per-plugin Codex manifest uses `skills: "./skills/"` and
  `interface: { displayName, category }`. Real Codex catalog entries omit a
  per-entry `description` — the generator matches that.

**`metadata.compatibility` stamp:** the generator writes
`metadata.compatibility` into each SKILL.md frontmatter (`[claude-code,
codex-cli]`, or `[claude-code]` when `claude_only`). This is generator-managed;
never hand-edit it.

### Verifying a plugin in Codex (optional local check)

```bash
codex plugin marketplace add "$PWD"                                  # register this repo (local path)
codex plugin list --available --json --marketplace <MARKETPLACE_NAME> # confirm the plugin parses
codex plugin marketplace remove <MARKETPLACE_NAME>                    # clean up afterwards
```

## Keep README.md in sync

`README.md` is the human-facing front door (install command + plugin table).
When you add/remove a plugin, update the table there too.
