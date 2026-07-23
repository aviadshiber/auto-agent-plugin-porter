#!/usr/bin/env bash
#
# validate-json.sh — Comprehensive JSON schema and consistency validation.
#
# Validates:
#   1. JSON syntax (all .json files)
#   2. marketplace.json schema (required fields, types, valid categories,
#      unique plugin names)
#   3. plugin.json schema (required fields, valid semver)
#   4. Cross-file consistency (names, versions, sources match)
#   5. Registry schema (jsonschema)
#
# Usage:
#   ./scripts/validate-json.sh              # Validate all plugins
#   ./scripts/validate-json.sh <name>       # Validate a single plugin
#
# Exit codes:
#   0 — all checks passed
#   1 — one or more checks failed
#
# Security note: all shell variables are passed to Python via the process
# environment (os.environ), never interpolated into Python source code.
# This prevents shell-to-Python injection via malicious file/directory names.
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MARKETPLACE="$REPO_ROOT/.claude-plugin/marketplace.json"
CODEX_MARKETPLACE="$REPO_ROOT/.agents/plugins/marketplace.json"
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
header "1. JSON syntax"
# ─────────────────────────────────────────────

# marketplace.json
if VALIDATE_FILE="$MARKETPLACE" python3 -c 'import json,os; json.load(open(os.environ["VALIDATE_FILE"]))' 2>/dev/null; then
    pass "marketplace.json is valid JSON"
else
    fail "marketplace.json has invalid JSON syntax"
fi

# Each plugin.json
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    pjson="${plugin_dir}.claude-plugin/plugin.json"
    if [[ -f "$pjson" ]]; then
        if VALIDATE_FILE="$pjson" python3 -c 'import json,os; json.load(open(os.environ["VALIDATE_FILE"]))' 2>/dev/null; then
            pass "[$name] plugin.json is valid JSON"
        else
            fail "[$name] plugin.json has invalid JSON syntax"
        fi
    else
        fail "[$name] .claude-plugin/plugin.json not found"
    fi
done

# ─────────────────────────────────────────────
header "2. marketplace.json schema (incl. unique plugin names)"
# ─────────────────────────────────────────────

mp_errors=$(VALIDATE_FILE="$MARKETPLACE" python3 - 2>&1 <<'PYEOF'
import json, re, os

with open(os.environ['VALIDATE_FILE']) as f:
    data = json.load(f)

errors = []

# Top-level required fields
for field in ['name', 'plugins']:
    if field not in data:
        errors.append(f'Missing required top-level field: {field}')

# metadata
if 'metadata' in data:
    meta = data['metadata']
    for field in ['description', 'version']:
        if field not in meta:
            errors.append(f'Missing metadata.{field}')
    if 'version' in meta:
        if not re.match(r'^[0-9]+\.[0-9]+\.[0-9]+$', meta['version']):
            errors.append(f'metadata.version "{meta["version"]}" is not valid semver')
else:
    errors.append('Missing metadata section')

# plugins array
if 'plugins' in data:
    if not isinstance(data['plugins'], list):
        errors.append('plugins must be an array')
    else:
        valid_cats = {'documentation', 'debugging', 'devops', 'analytics', 'testing', 'monitoring', 'development'}
        seen_names = set()
        for i, p in enumerate(data['plugins']):
            prefix = f'plugins[{i}]'
            for field in ['name', 'source', 'description', 'version']:
                if field not in p:
                    errors.append(f'{prefix}: missing required field "{field}"')
            if 'name' in p:
                if p['name'] in seen_names:
                    errors.append(f'{prefix}: duplicate name "{p["name"]}"')
                seen_names.add(p['name'])
            if 'version' in p:
                if not re.match(r'^[0-9]+\.[0-9]+\.[0-9]+$', p['version']):
                    errors.append(f'{prefix}: version "{p["version"]}" is not valid semver')
            if 'category' in p:
                if p['category'] not in valid_cats:
                    errors.append(f'{prefix}: invalid category "{p["category"]}" (valid: {sorted(valid_cats)})')
            else:
                errors.append(f'{prefix}: missing category')
            if 'keywords' in p:
                if not isinstance(p['keywords'], list):
                    errors.append(f'{prefix}: keywords must be an array')
                elif len(p['keywords']) == 0:
                    errors.append(f'{prefix}: keywords array is empty')
            if 'source' in p:
                if not p['source'].startswith('./plugins/'):
                    errors.append(f'{prefix}: source should start with "./plugins/"')

for e in errors:
    print(e)
PYEOF
) || true

if [[ -z "$mp_errors" ]]; then
    pass "marketplace.json schema is valid"
else
    while IFS= read -r line; do
        fail "marketplace.json: $line"
    done <<< "$mp_errors"
fi

# ─────────────────────────────────────────────
header "2b. Codex catalog schema (.agents/plugins/marketplace.json)"
# ─────────────────────────────────────────────
#
# The Codex catalog has a different shape from the Claude catalog. We check
# the fields Codex actually requires. NOTE: a per-plugin `description` is
# deliberately NOT required here — the Codex catalog omits it by design (the
# description lives in each plugin's .codex-plugin/plugin.json), so requiring
# it would be wrong.
if [[ -f "$CODEX_MARKETPLACE" ]]; then
    if VALIDATE_FILE="$CODEX_MARKETPLACE" python3 -c 'import json,os; json.load(open(os.environ["VALIDATE_FILE"]))' 2>/dev/null; then
        pass "Codex marketplace.json is valid JSON"
    else
        fail "Codex marketplace.json has invalid JSON syntax"
    fi

    cx_errors=$(VALIDATE_FILE="$CODEX_MARKETPLACE" python3 - 2>&1 <<'PYEOF'
import json, os

with open(os.environ['VALIDATE_FILE']) as f:
    data = json.load(f)

errors = []

# Top-level name
if 'name' not in data:
    errors.append('missing required top-level field: name')

# interface.displayName
iface = data.get('interface')
if not isinstance(iface, dict):
    errors.append('missing or non-object "interface"')
elif 'displayName' not in iface:
    errors.append('interface missing required field "displayName"')

# plugins[]
plugins = data.get('plugins')
if not isinstance(plugins, list):
    errors.append('plugins must be an array')
else:
    for i, p in enumerate(plugins):
        prefix = f'plugins[{i}]'
        if not isinstance(p, dict):
            errors.append(f'{prefix}: must be an object')
            continue
        if 'name' not in p:
            errors.append(f'{prefix}: missing required field "name"')
        # source.{source,path}
        src = p.get('source')
        if not isinstance(src, dict):
            errors.append(f'{prefix}: missing or non-object "source"')
        else:
            for sf in ('source', 'path'):
                if sf not in src:
                    errors.append(f'{prefix}.source: missing required field "{sf}"')
        # policy.{installation,authentication}
        pol = p.get('policy')
        if not isinstance(pol, dict):
            errors.append(f'{prefix}: missing or non-object "policy"')
        else:
            for pf in ('installation', 'authentication'):
                if pf not in pol:
                    errors.append(f'{prefix}.policy: missing required field "{pf}"')
        # category (required; description intentionally NOT required)
        if 'category' not in p:
            errors.append(f'{prefix}: missing required field "category"')

for e in errors:
    print(e)
PYEOF
) || true

    if [[ -z "$cx_errors" ]]; then
        pass "Codex marketplace.json schema is valid"
    else
        while IFS= read -r line; do
            fail "Codex marketplace.json: $line"
        done <<< "$cx_errors"
    fi
else
    warn "Codex marketplace.json missing — skipping Codex catalog check"
fi

# ─────────────────────────────────────────────
header "3. plugin.json schema"
# ─────────────────────────────────────────────
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    pjson="${plugin_dir}.claude-plugin/plugin.json"
    [[ -f "$pjson" ]] || continue

    pj_errors=$(VALIDATE_FILE="$pjson" PLUGIN_NAME="$name" python3 - 2>&1 <<'PYEOF'
import json, re, os

pjson_path = os.environ['VALIDATE_FILE']
name = os.environ['PLUGIN_NAME']

with open(pjson_path) as f:
    data = json.load(f)

errors = []

# Required fields
for field in ['name', 'description', 'version']:
    if field not in data:
        errors.append(f'missing required field "{field}"')

# Name must match directory
if data.get('name', '') != name:
    errors.append(f'name "{data.get("name","")}" does not match directory "{name}"')

# Version must be valid semver
v = data.get('version', '')
if v and not re.match(r'^[0-9]+\.[0-9]+\.[0-9]+$', v):
    errors.append(f'version "{v}" is not valid semver')

# Description must be non-empty
if 'description' in data and not data['description'].strip():
    errors.append('description is empty')

# No unexpected fields (warn only)
known = {'name', 'description', 'version', 'lspServers'}
extra = set(data.keys()) - known
if extra:
    print(f'WARN:unexpected fields: {sorted(extra)}')

# lspServers schema validation (per official Claude Code plugin docs)
if 'lspServers' in data:
    lsp = data['lspServers']
    valid_srv_fields = {'command', 'args', 'extensionToLanguage',
                        'restartOnCrash', 'maxRestarts', 'startupTimeout', 'env', 'initializationOptions'}
    if isinstance(lsp, str):
        pass  # Path reference to external LSP config — nothing to validate here
    elif isinstance(lsp, list):
        for i, item in enumerate(lsp):
            if not isinstance(item, str):
                errors.append(f'lspServers[{i}]: must be a path string when lspServers is an array, not {type(item).__name__}')
    elif isinstance(lsp, dict):
        for srv_name, srv_cfg in lsp.items():
            if not isinstance(srv_cfg, dict):
                errors.append(f'lspServers["{srv_name}"]: value must be an object')
                continue
            if 'command' not in srv_cfg:
                errors.append(f'lspServers["{srv_name}"]: missing required field "command"')
            elif isinstance(srv_cfg['command'], list):
                errors.append(f'lspServers["{srv_name}"]: "command" must be a string (args go in "args" array), not an array')
            elif not isinstance(srv_cfg['command'], str):
                errors.append(f'lspServers["{srv_name}"]: "command" must be a string')
            if 'extensionToLanguage' not in srv_cfg:
                errors.append(f'lspServers["{srv_name}"]: missing required field "extensionToLanguage"')
            elif not isinstance(srv_cfg['extensionToLanguage'], dict):
                errors.append(f'lspServers["{srv_name}"]: "extensionToLanguage" must be an object')
            extra = set(srv_cfg.keys()) - valid_srv_fields
            if extra:
                print(f'WARN:lspServers["{srv_name}"]: unexpected fields: {sorted(extra)}')
    else:
        errors.append(f'lspServers must be a string path, array of paths, or object keyed by server name, not {type(lsp).__name__}')

for e in errors:
    print(e)
PYEOF
) || true

    if [[ -z "$pj_errors" ]]; then
        pass "[$name] plugin.json schema is valid"
    else
        while IFS= read -r line; do
            if [[ "$line" == WARN:* ]]; then
                warn "[$name] plugin.json: ${line#WARN:}"
            else
                fail "[$name] plugin.json: $line"
            fi
        done <<< "$pj_errors"
    fi
done

# ─────────────────────────────────────────────
header "4. Cross-file consistency"
# ─────────────────────────────────────────────
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    pjson="${plugin_dir}.claude-plugin/plugin.json"
    [[ -f "$pjson" ]] || continue

    consistency_errors=$(PLUGIN_JSON="$pjson" MARKETPLACE_JSON="$MARKETPLACE" PLUGIN_NAME="$name" python3 - 2>&1 <<'PYEOF'
import json, os

with open(os.environ['PLUGIN_JSON']) as f:
    pdata = json.load(f)

with open(os.environ['MARKETPLACE_JSON']) as f:
    mdata = json.load(f)

name = os.environ['PLUGIN_NAME']
errors = []
mp_entry = None
for p in mdata.get('plugins', []):
    if p.get('name') == name:
        mp_entry = p
        break

if mp_entry is None:
    errors.append('not found in marketplace.json')
else:
    # Version match
    pv = pdata.get('version', '')
    mv = mp_entry.get('version', '')
    if pv != mv:
        errors.append(f'version mismatch: plugin.json={pv}, marketplace.json={mv}')

    # Source path check
    expected_source = f'./plugins/{name}'
    if mp_entry.get('source', '') != expected_source:
        errors.append(f'source should be "{expected_source}", got "{mp_entry.get("source","")}"')

    # lspServers drift check: if marketplace.json declares lspServers, plugin.json must too
    mp_has_lsp = 'lspServers' in mp_entry
    pj_has_lsp = 'lspServers' in pdata
    if mp_has_lsp and not pj_has_lsp:
        errors.append('marketplace.json declares lspServers but plugin.json does not — add lspServers to plugin.json so the installed plugin auto-starts the server')
    elif pj_has_lsp and not mp_has_lsp:
        print('WARN:plugin.json has lspServers but marketplace.json entry does not — consider adding lspServers to marketplace.json for consistency')

for e in errors:
    print(e)
PYEOF
) || true

    if [[ -z "$consistency_errors" ]]; then
        pass "[$name] plugin.json ↔ marketplace.json consistent"
    else
        while IFS= read -r line; do
            if [[ "$line" == WARN:* ]]; then
                warn "[$name] ${line#WARN:}"
            else
                fail "[$name] $line"
            fi
        done <<< "$consistency_errors"
    fi
done

# ─────────────────────────────────────────────
header "5. Registry schema (jsonschema)"
# ─────────────────────────────────────────────
#
# Validates registry/plugins.json against registry/schema.json using JSON
# Schema Draft 2020-12.
#
# Exit codes from validate-registry.py:
#   0 = OK            → pass
#   1 = schema errors → fail (errors printed to stderr)
#   2 = jsonschema not installed → warn (CI installs it; locally optional)

if [[ -f "$REPO_ROOT/registry/plugins.json" && -f "$REPO_ROOT/registry/schema.json" ]]; then
    set +e
    reg_output=$(python3 "$REPO_ROOT/scripts/validate-registry.py" 2>&1)
    reg_rc=$?
    set -e
    case "$reg_rc" in
        0)
            pass "registry/plugins.json matches schema"
            ;;
        1)
            while IFS= read -r line; do
                [[ -n "$line" ]] && fail "registry: ${line## }"
            done <<< "$reg_output"
            ;;
        2)
            warn "registry/plugins.json: jsonschema not installed locally; CI will validate"
            ;;
        *)
            fail "registry: validate-registry.py exited $reg_rc — $reg_output"
            ;;
    esac
else
    warn "registry/plugins.json or registry/schema.json missing — skipping schema check"
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
    red "JSON validation FAILED"
    exit 1
else
    green "JSON validation PASSED"
    exit 0
fi
