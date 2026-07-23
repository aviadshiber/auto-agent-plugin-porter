#!/usr/bin/env bash
#
# validate.sh — Run all marketplace quality checks.
#
# Usage:
#   ./scripts/validate.sh              # Validate all plugins
#   ./scripts/validate.sh <name>       # Validate a single plugin
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLUGINS_DIR="$REPO_ROOT/plugins"
PASS=0
FAIL=0
WARN=0

green()  { printf '\033[32m%s\033[0m\n' "$*"; }
red()    { printf '\033[31m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }

pass() { PASS=$((PASS + 1)); green "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); red   "  FAIL: $1"; }
warn() { WARN=$((WARN + 1)); yellow "  WARN: $1"; }

header() { printf '\n\033[1m=== %s ===\033[0m\n' "$1"; }

# Determine which plugins to check
if [[ "${1:-}" != "" ]]; then
    if [[ ! -d "$PLUGINS_DIR/$1" ]]; then
        red "Plugin '$1' not found in $PLUGINS_DIR/"
        exit 1
    fi
    PLUGIN_DIRS=("$PLUGINS_DIR/$1/")
else
    PLUGIN_DIRS=("$PLUGINS_DIR"/*/)
fi

# ─────────────────────────────────────────────
header "1. Personal absolute paths (/Users/...)"
# ─────────────────────────────────────────────
matches=$(grep -rn --exclude-dir='__pycache__' --exclude='*.pyc' '/Users/' "$PLUGINS_DIR" 2>/dev/null || true)
if [[ -z "$matches" ]]; then
    pass "No /Users/ absolute paths"
else
    fail "Found /Users/ paths:"
    printf '    %s\n' "${matches//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "2. Personal ~/.claude/ paths"
# ─────────────────────────────────────────────
matches=$(grep -rn "$HOME/.claude/\(skills\|issues\|designs\|memory\)/" "$PLUGINS_DIR" 2>/dev/null \
    | grep -v 'CLAUDE_PLUGIN_ROOT:-' \
    | grep -v 'permission-patterns' || true)
# shellcheck disable=SC2088
matches2=$(grep -rn '~/.claude/\(skills\|issues\|designs\|memory\)/' "$PLUGINS_DIR" 2>/dev/null \
    | grep -v 'CLAUDE_PLUGIN_ROOT:-' \
    | grep -v 'permission-patterns' || true)
combined="$matches$matches2"
if [[ -z "$combined" ]]; then
    pass "No personal ~/.claude/ paths"
else
    fail "Found personal ~/.claude/ paths:"
    _sorted=$(printf '%s\n' "$combined" | sort -u)
    printf '    %s\n' "${_sorted//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "3. Placeholder / boilerplate files"
# ─────────────────────────────────────────────
placeholders=$(find "$PLUGINS_DIR" \( -name "example*" -o -name "placeholder*" -o -name "sample_*" \) 2>/dev/null || true)
if [[ -z "$placeholders" ]]; then
    pass "No placeholder files"
else
    fail "Found placeholder files (remove them):"
    printf '    %s\n' "${placeholders//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "4. Cross-plugin path traversals (../)"
# ─────────────────────────────────────────────
traversals=$(grep -rn 'CLAUDE_PLUGIN_ROOT.*\.\.\/' "$PLUGINS_DIR" 2>/dev/null || true)
if [[ -z "$traversals" ]]; then
    pass "No cross-plugin path traversals"
else
    fail "Found ../ references (plugins are cached independently):"
    printf '    %s\n' "${traversals//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "5. Plugin self-contained check"
# ─────────────────────────────────────────────
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    # Extract ${CLAUDE_PLUGIN_ROOT}/... references, clean suffixes
    refs=$(grep -roE '\$\{CLAUDE_PLUGIN_ROOT\}/[^ )`"'"'"']+' "$plugin_dir" 2>/dev/null \
        | sed 's/.*\${CLAUDE_PLUGIN_ROOT}\///' \
        | sed 's/:.*$//' \
        | sed 's/[,;)"`\\'"'"']$//' \
        | sort -u || true)
    # Read line-by-line (glob-safe): an unquoted `for f in $refs` would glob-
    # expand any `*` in a ref against the cwd. `<<<` fires once with an empty
    # line when $refs is empty, so skip blanks to preserve the old behavior.
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        # Skip directory-only refs like "scripts/"
        [[ "$f" == */ ]] && continue
        if [[ ! -e "${plugin_dir}${f}" ]]; then
            fail "[$name] Missing referenced file: $f"
        fi
    done <<< "$refs"
done
pass "CLAUDE_PLUGIN_ROOT references checked"

# ─────────────────────────────────────────────
header "6. SKILL.md exists"
# ─────────────────────────────────────────────
# Canonical layout: plugins/<name>/skills/<name>/SKILL.md (works for both
# Claude Code auto-discovery and Codex CLI's "skills": "./skills/" loader).
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    if [[ -f "${plugin_dir}skills/${name}/SKILL.md" ]]; then
        pass "[$name] SKILL.md exists"
    else
        fail "[$name] Missing SKILL.md at skills/${name}/SKILL.md"
    fi
done

# ─────────────────────────────────────────────
header "7. Scripts executable"
# ─────────────────────────────────────────────
non_exec=$(find "$PLUGINS_DIR" -name "*.sh" ! -perm -111 2>/dev/null || true)
if [[ -z "$non_exec" ]]; then
    pass "All .sh scripts are executable"
else
    fail "Non-executable scripts (run: chmod +x <file>):"
    printf '    %s\n' "${non_exec//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "8. Secrets scan"
# ─────────────────────────────────────────────
# Look for hardcoded secret patterns (AWS keys, GitHub classic + fine-grained
# tokens, GitLab tokens, Slack tokens, OpenAI keys, private keys). Scans the
# WHOLE repo via `git ls-files` (tracked files only — .git is never listed),
# not just plugins/, so a secret committed to scripts/ or docs is caught too.
#
# The quantified suffixes (e.g. github_pat_[A-Za-z0-9_]{22,}) match real tokens
# but NOT the bare `github_pat_`/`glpat-`/`xox?-` prefixes that appear literally
# in this very pattern — otherwise the scan would self-match its own source.
SECRET_RE='(AKIA[0-9A-Z]{16}|ghp_[a-zA-Z0-9]{36}|github_pat_[A-Za-z0-9_]{22,}|glpat-[A-Za-z0-9_-]{20}|xox[baprs]-[A-Za-z0-9-]{10,}|sk-[a-zA-Z0-9]{48}|BEGIN (RSA |EC )?PRIVATE KEY)'
secrets=$(git -C "$REPO_ROOT" ls-files -z 2>/dev/null | xargs -0 grep -EnI "$SECRET_RE" 2>/dev/null || true)
if [[ -z "$secrets" ]]; then
    pass "No secrets detected"
else
    fail "Possible secrets found (review and remove):"
    printf '    %s\n' "${secrets//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "9. Environment-agnostic paths (CLAUDE_HOME/CLAUDE_GIT)"
# ─────────────────────────────────────────────
# Scripts must use ${CLAUDE_HOME:-$HOME/.claude} and ${CLAUDE_GIT:-$HOME/git}
# instead of hardcoded $HOME/.claude or ~/git.
env_issues=""
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    while IFS= read -r script; do
        rel_path="${script#"$PLUGINS_DIR"/}"
        while IFS= read -r match_line; do
            if echo "$match_line" | grep -qE 'CLAUDE_HOME|CLAUDE_GIT|^[0-9]+:[[:space:]]*#'; then
                continue
            fi
            env_issues+="$rel_path: hardcoded path — use \${CLAUDE_HOME:-\$HOME/.claude} or \${CLAUDE_GIT:-\$HOME/git}"$'\n'
            break  # One finding per file is enough
        done < <(grep -nE '\$HOME/\.claude|\$HOME/git|~/\.claude|~/git' "$script" 2>/dev/null || true)
    done < <(find "$plugin_dir" -name "*.sh" -not -path "*/tests/*" 2>/dev/null)
done
if [[ -z "$env_issues" ]]; then
    pass "All scripts use CLAUDE_HOME/CLAUDE_GIT env vars"
else
    fail "Scripts with hardcoded home paths (use \${CLAUDE_HOME:-\$HOME/.claude}):"
    printf '    %s\n' "${env_issues//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "10. hooks.json format (dormant — none today)"
# ─────────────────────────────────────────────
hooks_issues=""
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    hooks_file="${plugin_dir}hooks/hooks.json"
    [ -f "$hooks_file" ] || continue
    name="$(basename "$plugin_dir")"

    result=$(HOOKS_FILE="$hooks_file" python3 -c "
import json, os, sys
try:
    with open(os.environ['HOOKS_FILE']) as f:
        data = json.load(f)
except json.JSONDecodeError as e:
    print('hooks.json is not valid JSON:', e)
    sys.exit(0)
if not isinstance(data, dict):
    print('hooks.json must be an object, not ' + type(data).__name__)
    sys.exit(0)
if 'hooks' not in data:
    print('hooks.json missing required \"hooks\" key')
    sys.exit(0)
hooks = data['hooks']
if isinstance(hooks, list):
    print('hooks must be an object keyed by event name (e.g. {\"PostToolUse\": [...]}), not an array')
    sys.exit(0)
if not isinstance(hooks, dict):
    print('hooks must be an object keyed by event name, not ' + type(hooks).__name__)
    sys.exit(0)
for event_name, entries in hooks.items():
    if not isinstance(entries, list):
        print(f'hooks.{event_name} must be an array')
        sys.exit(0)
    for i, entry in enumerate(entries):
        if not isinstance(entry, dict):
            print(f'hooks.{event_name}[{i}] must be an object')
            sys.exit(0)
        if 'hooks' not in entry:
            print(f'hooks.{event_name}[{i}] missing required \"hooks\" array (got keys: {list(entry.keys())})')
            sys.exit(0)
        if not isinstance(entry['hooks'], list):
            print(f'hooks.{event_name}[{i}].hooks must be an array, not ' + type(entry['hooks']).__name__)
            sys.exit(0)
        for j, hook in enumerate(entry['hooks']):
            if not isinstance(hook, dict):
                print(f'hooks.{event_name}[{i}].hooks[{j}] must be an object')
                sys.exit(0)
            if 'type' not in hook:
                print(f'hooks.{event_name}[{i}].hooks[{j}] missing required \"type\" field')
                sys.exit(0)
            if 'command' not in hook:
                print(f'hooks.{event_name}[{i}].hooks[{j}] missing required \"command\" field')
                sys.exit(0)
" 2>&1 || echo "python3 error: hooks.json schema check failed unexpectedly")

    if [[ -n "$result" ]]; then
        hooks_issues+="$name: $result"$'\n'
    fi
done
if [[ -z "$hooks_issues" ]]; then
    pass "hooks.json schema valid"
else
    fail "hooks.json schema errors (required: {hooks: {EventName: [{hooks: [{type, command}]}]}}):"
    printf '    %s\n' "${hooks_issues//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "11. plugin.json lspServers format (dormant — none today)"
# ─────────────────────────────────────────────
lsp_issues=""
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    plugin_json="${plugin_dir}.claude-plugin/plugin.json"
    [ -f "$plugin_json" ] || continue
    name="$(basename "$plugin_dir")"

    result=$(PLUGIN_JSON="$plugin_json" python3 -c "
import json, os, sys
try:
    with open(os.environ['PLUGIN_JSON']) as f:
        data = json.load(f)
except json.JSONDecodeError as e:
    print('plugin.json is not valid JSON:', e)
    sys.exit(0)
lsp = data.get('lspServers')
if lsp is None:
    sys.exit(0)
if isinstance(lsp, str):
    sys.exit(0)  # Path reference — no server-level fields to validate
if isinstance(lsp, list):
    for i, item in enumerate(lsp):
        if not isinstance(item, str):
            print(f'lspServers[{i}] must be a path string when lspServers is an array, not ' + type(item).__name__)
            sys.exit(0)
    sys.exit(0)
if not isinstance(lsp, dict):
    print('lspServers must be a string path, array of paths, or object keyed by server name, not ' + type(lsp).__name__)
    sys.exit(0)
for server_name, cfg in lsp.items():
    if not isinstance(cfg, dict):
        print(f'lspServers[\"{server_name}\"] must be an object, not ' + type(cfg).__name__)
        sys.exit(0)
    cmd = cfg.get('command')
    if cmd is None:
        print(f'lspServers[\"{server_name}\"] missing required \"command\" field')
        sys.exit(0)
    if isinstance(cmd, list):
        print(f'lspServers[\"{server_name}\"].command must be a string (args go in \"args\" array), not an array')
        sys.exit(0)
    if not isinstance(cmd, str):
        print(f'lspServers[\"{server_name}\"].command must be a string, not ' + type(cmd).__name__)
        sys.exit(0)
    ext_map = cfg.get('extensionToLanguage')
    if ext_map is None:
        print(f'lspServers[\"{server_name}\"] missing required \"extensionToLanguage\" field')
        sys.exit(0)
    if not isinstance(ext_map, dict):
        print(f'lspServers[\"{server_name}\"].extensionToLanguage must be an object, not ' + type(ext_map).__name__)
        sys.exit(0)
" 2>&1 || echo "python3 error: lspServers schema check failed unexpectedly")

    if [[ -n "$result" ]]; then
        lsp_issues+="$name: $result"$'\n'
    fi
done
if [[ -z "$lsp_issues" ]]; then
    pass "lspServers schema valid"
else
    fail "lspServers schema errors (see https://code.claude.com/docs/en/plugins-reference.md#lsp-servers):"
    printf '    %s\n' "${lsp_issues//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "12. Instructions pointer (AGENTS.md canonical, CLAUDE.md → AGENTS.md)"
# ─────────────────────────────────────────────
if sync_out=$("$REPO_ROOT/scripts/check-instructions-sync.sh" 2>&1); then
    pass "AGENTS.md is canonical and CLAUDE.md points to it"
else
    fail "Instructions pointer broken:"
    printf '    %s\n' "${sync_out//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "13. Registry drift (manifests in sync with registry/plugins.json)"
# ─────────────────────────────────────────────
if drift_out=$(python3 "$REPO_ROOT/scripts/generate-manifests.py" --check 2>&1); then
    pass "Registry and generated manifests are in sync"
else
    fail "Registry drift — run: python3 scripts/generate-manifests.py"
    printf '    %s\n' "${drift_out//$'\n'/$'\n    '}"
fi

# ─────────────────────────────────────────────
header "SUMMARY"
# ─────────────────────────────────────────────
echo ""
green "  Passed: $PASS"
[[ $WARN -gt 0 ]] && yellow "  Warnings: $WARN"
[[ $FAIL -gt 0 ]] && red "  Failed: $FAIL"
echo ""

if [[ $FAIL -gt 0 ]]; then
    red "Validation FAILED — fix the issues above before committing."
    exit 1
else
    green "Validation PASSED"
    exit 0
fi
