#!/usr/bin/env python3
# _is_claude_only.py — exit 0 iff a plugin is marked claude_only in the registry.
#
# Utility for scripting around the claude_only gate (the generator itself reads
# the flag directly). The exit-code-as-boolean shape lets a bash caller do:
#
#   if PLUGIN_NAME=foo REGISTRY=... python3 scripts/_is_claude_only.py; then
#       # claude_only → skip Codex-specific work
#   fi
#
# Args (env):
#   PLUGIN_NAME   plugin to look up
#   REGISTRY      path to registry/plugins.json
#
# Exit codes:
#   0  plugin exists and claude_only is true
#   1  plugin exists and claude_only is false/missing — OR plugin not found
#      (treated as "not claude_only")
#   2  missing required env var (helpful message instead of cryptic KeyError)
import json
import os
import sys


def require_env(name: str) -> str:
    val = os.environ.get(name)
    if not val:
        sys.stderr.write(
            f"_is_claude_only: missing required env var {name}.\n"
        )
        sys.exit(2)
    return val


def main() -> int:
    registry_path = require_env("REGISTRY")
    name = require_env("PLUGIN_NAME")

    with open(registry_path) as f:
        data = json.load(f)

    for p in data["plugins"]:
        if p["name"] == name:
            return 0 if p.get("claude_only") else 1
    return 1


if __name__ == "__main__":
    sys.exit(main())
