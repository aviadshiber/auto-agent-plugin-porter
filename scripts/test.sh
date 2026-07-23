#!/usr/bin/env bash
#
# test.sh — Run tests for marketplace plugins.
#
# Tests:
#   1. SKILL.md frontmatter (required fields: name, description)
#   2. Shell script syntax (bash -n)
#   3. ShellCheck lint (if installed)
#   4. Internal markdown links resolve
#   5. Frontmatter name matches directory name
#   6. Description is behavioral (contains trigger phrases)
#
# Usage:
#   ./scripts/test.sh              # Test all plugins
#   ./scripts/test.sh <name>       # Test a single plugin
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PLUGINS_DIR="$REPO_ROOT/plugins"
PASS=0
FAIL=0
WARN=0

# Skill-description budget thresholds (mirrors Claude Code Tier 1 loading).
# Anthropic's per-skill cap is 1536 chars (descriptor truncated past this).
# The default SLASH_COMMAND_TOOL_CHAR_BUDGET aggregate is 8000 chars.
SKILL_DESC_PER_ITEM_CAP=1536
SKILL_DESC_PER_ITEM_WARN=1200
SKILL_DESC_AGGREGATE_FAIL=8000
SKILL_DESC_AGGREGATE_WARN=6000

green()  { printf '\033[32m%s\033[0m\n' "$*"; }
red()    { printf '\033[31m%s\033[0m\n' "$*"; }
yellow() { printf '\033[33m%s\033[0m\n' "$*"; }

pass() { PASS=$((PASS + 1)); green "  PASS: $1"; }
fail() { FAIL=$((FAIL + 1)); red   "  FAIL: $1"; }
warn() { WARN=$((WARN + 1)); yellow "  WARN: $1"; }

header() { printf '\n\033[1m=== %s ===\033[0m\n' "$1"; }

# Determine which plugins to test
if [[ "${1:-}" != "" ]]; then
    if [[ ! -d "$PLUGINS_DIR/$1" ]]; then
        red "Plugin '$1' not found in $PLUGINS_DIR/"
        exit 1
    fi
    PLUGIN_DIRS=("$PLUGINS_DIR/$1")
else
    PLUGIN_DIRS=("$PLUGINS_DIR"/*/)
fi

# ─────────────────────────────────────────────
header "1. SKILL.md frontmatter"
# ─────────────────────────────────────────────
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    skill="$plugin_dir/skills/$name/SKILL.md"
    [ -f "$skill" ] || continue

    # Check frontmatter exists (starts with ---)
    if ! head -1 "$skill" | grep -q '^---$'; then
        fail "[$name] SKILL.md missing frontmatter (must start with ---)"
        continue
    fi

    # Extract frontmatter
    frontmatter=$(awk 'NR==1{next} /^---$/{exit} {print}' "$skill")

    # Required: name field
    fm_name=$(echo "$frontmatter" | grep -E '^name:' | sed 's/^name: *//' || true)
    if [[ -z "$fm_name" ]]; then
        fail "[$name] SKILL.md missing 'name' in frontmatter"
    elif [[ "$fm_name" != "$name" ]]; then
        fail "[$name] frontmatter name '$fm_name' doesn't match directory name '$name'"
    else
        pass "[$name] frontmatter name matches directory"
    fi

    # Required: description field
    fm_desc=$(echo "$frontmatter" | grep -E '^description:' | sed 's/^description: *//' || true)
    fm_dmi=$(echo "$frontmatter" | grep -E '^disable-model-invocation:' | sed 's/^disable-model-invocation: *//' | tr -d '"' || true)
    desc_len=${#fm_desc}
    if [[ -z "$fm_desc" ]]; then
        fail "[$name] SKILL.md missing 'description' in frontmatter"
    elif [[ $desc_len -lt 20 ]]; then
        fail "[$name] description too short ($desc_len chars, min 20)"
    elif [[ "$fm_dmi" != "true" && $desc_len -gt $SKILL_DESC_PER_ITEM_CAP ]]; then
        fail "[$name] description too long ($desc_len chars, max $SKILL_DESC_PER_ITEM_CAP — Claude Code truncates the descriptor past this)"
    else
        if [[ "$fm_dmi" != "true" && $desc_len -ge $SKILL_DESC_PER_ITEM_WARN ]]; then
            warn "[$name] description approaching cap ($desc_len chars, warn at $SKILL_DESC_PER_ITEM_WARN, hard cap $SKILL_DESC_PER_ITEM_CAP)"
        fi
        pass "[$name] description present ($desc_len chars)"
    fi

    # Check description is behavioral (should contain trigger phrases)
    if [[ -n "$fm_desc" ]]; then
        if echo "$fm_desc" | grep -qiE '(should be used when|use this|auto-invoke on|invoke when|this skill)'; then
            pass "[$name] description contains invocation triggers"
        else
            warn "[$name] description lacks invocation triggers (add 'This skill should be used when...' or 'Auto-invoke on:')"
        fi
    fi
done

# ─────────────────────────────────────────────
header "1b. Skill description budget (aggregate)"
# ─────────────────────────────────────────────
total_chars=0
counted=0
skipped_dmi=0
for plugin_dir in "$PLUGINS_DIR"/*/; do
    name=$(basename "$plugin_dir")
    skill="$plugin_dir/skills/$name/SKILL.md"
    [ -f "$skill" ] || continue
    fm=$(awk 'NR==1{next} /^---$/{exit} {print}' "$skill")
    desc=$(echo "$fm" | grep -E '^description:' | sed 's/^description: *//' || true)
    [ -z "$desc" ] && continue
    dmi=$(echo "$fm" | grep -E '^disable-model-invocation:' | sed 's/^disable-model-invocation: *//' | tr -d '"' || true)
    if [[ "$dmi" == "true" ]]; then
        skipped_dmi=$((skipped_dmi + 1))
        continue
    fi
    total_chars=$((total_chars + ${#desc}))
    counted=$((counted + 1))
done

if (( total_chars > SKILL_DESC_AGGREGATE_FAIL )); then
    fail "marketplace description total $total_chars chars exceeds SLASH_COMMAND_TOOL_CHAR_BUDGET=$SKILL_DESC_AGGREGATE_FAIL ($counted plugins counted, $skipped_dmi DMI-skipped)"
elif (( total_chars > SKILL_DESC_AGGREGATE_WARN )); then
    warn "marketplace description total $total_chars chars approaching SLASH_COMMAND_TOOL_CHAR_BUDGET=$SKILL_DESC_AGGREGATE_FAIL (warn at $SKILL_DESC_AGGREGATE_WARN, $counted plugins counted, $skipped_dmi DMI-skipped)"
else
    pass "marketplace description total $total_chars chars within budget ($counted plugins, $skipped_dmi DMI-skipped, warn=$SKILL_DESC_AGGREGATE_WARN/fail=$SKILL_DESC_AGGREGATE_FAIL)"
fi

# ─────────────────────────────────────────────
header "2. Shell script syntax (bash -n)"
# ─────────────────────────────────────────────
script_count=0
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    while IFS= read -r script; do
        script_count=$((script_count + 1))
        if bash -n "$script" 2>/dev/null; then
            pass "[$name] $(basename "$script") syntax OK"
        else
            fail "[$name] $(basename "$script") has syntax errors"
            bash -n "$script" 2>&1 | sed 's/^/    /'
        fi
    done < <(find "$plugin_dir" -name "*.sh" -type f 2>/dev/null)
done
if [[ $script_count -eq 0 ]]; then
    pass "No shell scripts to check"
fi

# ─────────────────────────────────────────────
header "3. ShellCheck lint"
# ─────────────────────────────────────────────
if command -v shellcheck &>/dev/null; then
    for plugin_dir in "${PLUGIN_DIRS[@]}"; do
        name=$(basename "$plugin_dir")
        while IFS= read -r script; do
            sc_out=$(shellcheck -e SC1091,SC2034,SC2086 -S warning -f gcc "$script" 2>/dev/null || true)
            if [[ -z "$sc_out" ]]; then
                pass "[$name] $(basename "$script") shellcheck clean"
            else
                sc_count=$(echo "$sc_out" | wc -l | tr -d ' ')
                warn "[$name] $(basename "$script") shellcheck: $sc_count warning(s)"
            fi
        done < <(find "$plugin_dir" -name "*.sh" -type f 2>/dev/null)
    done
else
    warn "shellcheck not installed — skipping lint (brew install shellcheck)"
fi

# ─────────────────────────────────────────────
header "4. Internal markdown links"
# ─────────────────────────────────────────────
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    while IFS= read -r mdfile; do
        # shellcheck disable=SC2016
        while IFS= read -r ref; do
            # Skip glob patterns
            [[ "$ref" == *"*"* ]] && continue
            if [[ ! -e "${plugin_dir}/${ref}" ]]; then
                fail "[$name] $(basename "$mdfile") references missing file: $ref"
            fi
        done < <(grep -oE '`(references/[^`]+|scripts/[^`]+)`' "$mdfile" 2>/dev/null | tr -d '`' | sort -u || true)
    done < <(find "$plugin_dir" -name "*.md" -type f 2>/dev/null)
done
pass "Internal link check complete"

# ─────────────────────────────────────────────
header "5. Plugin.json consistency"
# ─────────────────────────────────────────────
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    pjson="${plugin_dir}.claude-plugin/plugin.json"
    [ -f "$pjson" ] || continue

    # Name in plugin.json must match directory
    pj_name=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('name',''))" "$pjson" 2>/dev/null || echo "")
    if [[ "$pj_name" == "$name" ]]; then
        pass "[$name] plugin.json name matches directory"
    else
        fail "[$name] plugin.json name '$pj_name' doesn't match directory '$name'"
    fi

    # Description must exist
    pj_desc=$(python3 -c "import json,sys; print(json.load(open(sys.argv[1])).get('description',''))" "$pjson" 2>/dev/null || echo "")
    if [[ -n "$pj_desc" ]]; then
        pass "[$name] plugin.json has description"
    else
        fail "[$name] plugin.json missing description"
    fi
done

# ─────────────────────────────────────────────
header "6. Hooks.json validation"
# ─────────────────────────────────────────────
hooks_count=0
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    hooks_file="${plugin_dir}hooks/hooks.json"
    [ -f "$hooks_file" ] || continue
    hooks_count=$((hooks_count + 1))

    # Must be valid JSON
    if ! python3 -m json.tool "$hooks_file" &>/dev/null; then
        fail "[$name] hooks.json is not valid JSON"
        continue
    fi

    validation=$(python3 -c "
import json, sys
with open(sys.argv[1]) as f:
    data = json.load(f)
if not isinstance(data, dict):
    print('FAIL:hooks.json must be an object, not ' + type(data).__name__)
    sys.exit(0)
if 'hooks' not in data:
    print('FAIL:hooks.json missing required \"hooks\" key')
    sys.exit(0)
hooks = data['hooks']
if isinstance(hooks, list):
    print('FAIL:hooks must be an object keyed by event name, not an array (flat format is not supported by Claude Code)')
    sys.exit(0)
if not isinstance(hooks, dict):
    print('FAIL:hooks must be an object keyed by event name, not ' + type(hooks).__name__)
    sys.exit(0)
for event_name, entries in hooks.items():
    if not isinstance(entries, list):
        print(f'FAIL:hooks.{event_name} must be an array')
        sys.exit(0)
    for i, entry in enumerate(entries):
        if not isinstance(entry, dict):
            print(f'FAIL:hooks.{event_name}[{i}] must be an object')
            sys.exit(0)
        if 'hooks' not in entry:
            print(f'FAIL:hooks.{event_name}[{i}] missing required \"hooks\" array (got keys: {list(entry.keys())})')
            sys.exit(0)
        if not isinstance(entry['hooks'], list):
            print(f'FAIL:hooks.{event_name}[{i}].hooks must be an array, not ' + type(entry['hooks']).__name__)
            sys.exit(0)
        for j, hook in enumerate(entry['hooks']):
            if not isinstance(hook, dict):
                print(f'FAIL:hooks.{event_name}[{i}].hooks[{j}] must be an object')
                sys.exit(0)
            if 'type' not in hook:
                print(f'FAIL:hooks.{event_name}[{i}].hooks[{j}] missing required \"type\" field')
                sys.exit(0)
            if 'command' not in hook:
                print(f'FAIL:hooks.{event_name}[{i}].hooks[{j}] missing required \"command\" field')
                sys.exit(0)
print('OK:nested format with ' + str(len(hooks)) + ' event(s)')
" "$hooks_file" 2>/dev/null || echo "FAIL:parse error")

    if [[ "$validation" == OK:* ]]; then
        pass "[$name] hooks.json valid (${validation#OK:})"
    else
        fail "[$name] ${validation#FAIL:}"
    fi
done
if [[ $hooks_count -eq 0 ]]; then
    pass "No hooks.json files to check"
fi

# ─────────────────────────────────────────────
header "7. Python tests (plugins with .py scripts must have tests/)"
# ─────────────────────────────────────────────
py_test_count=0
for plugin_dir in "${PLUGIN_DIRS[@]}"; do
    name=$(basename "$plugin_dir")
    py_files=()
    while IFS= read -r f; do
        py_files+=("$f")
    done < <(find "${plugin_dir%/}" -path "*/scripts/*.py" -not -path "*/tests/*" -not -path "*/__pycache__/*" -type f 2>/dev/null)
    [[ ${#py_files[@]} -eq 0 ]] && continue
    total_lines=$(wc -l "${py_files[@]}" 2>/dev/null | tail -1 | awk '{print $1}')
    [[ "${total_lines:-0}" -lt 100 ]] && continue

    py_test_count=$((py_test_count + 1))
    test_dir="${plugin_dir%/}/tests"
    if [[ ! -d "$test_dir" ]]; then
        fail "[$name] has ${#py_files[@]} Python script(s) but no tests/ directory"
        continue
    fi
    test_file_count=$(find "$test_dir" -name "test_*.py" -type f 2>/dev/null | wc -l | tr -d ' ')
    if [[ "$test_file_count" -eq 0 ]]; then
        fail "[$name] tests/ directory exists but has no test_*.py files"
        continue
    fi
    if command -v python3 &>/dev/null && python3 -m pytest --version &>/dev/null 2>&1; then
        if pytest_out=$(python3 -m pytest "$test_dir" -q --tb=short 2>&1); then
            result_line=$(echo "$pytest_out" | tail -1)
            pass "[$name] pytest: $result_line"
        else
            fail "[$name] pytest failed:"
            printf '    %s\n' "${pytest_out//$'\n'/$'\n'    }"
        fi
    else
        pass "[$name] has $test_file_count test file(s) (pytest not available — skipping execution)"
    fi
done
if [[ $py_test_count -eq 0 ]]; then
    pass "No plugins with Python scripts to check"
fi

# ─────────────────────────────────────────────
header "8. Marketplace tooling tests (scripts/tests/)"
# ─────────────────────────────────────────────
#
# Unit tests for the build/maintenance scripts themselves (generate-manifests,
# bump-version helpers, new-plugin scaffolder, etc.). Only runs when
# scripts/tests/ exists and pytest is available; targeted single-plugin runs
# skip this section.
tooling_tests_dir="$REPO_ROOT/scripts/tests"
if [[ "${1:-}" != "" ]]; then
    pass "Single-plugin run — skipping marketplace tooling tests"
elif [[ ! -d "$tooling_tests_dir" ]]; then
    pass "No scripts/tests/ directory — nothing to run"
elif ! command -v python3 &>/dev/null || ! python3 -m pytest --version &>/dev/null 2>&1; then
    test_file_count=$(find "$tooling_tests_dir" -name "test_*.py" -type f 2>/dev/null | wc -l | tr -d ' ')
    pass "scripts/tests/ has $test_file_count test file(s) (pytest not available — skipping execution)"
else
    if pytest_out=$(python3 -m pytest "$tooling_tests_dir" -q --tb=short 2>&1); then
        result_line=$(echo "$pytest_out" | tail -1)
        pass "scripts/tests pytest: $result_line"
    else
        fail "scripts/tests pytest failed:"
        printf '    %s\n' "${pytest_out//$'\n'/$'\n'    }"
    fi
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
    red "Tests FAILED — fix the issues above."
    exit 1
else
    green "Tests PASSED"
    exit 0
fi
