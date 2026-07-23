#!/usr/bin/env python3
# _is_codex_only.py — exit 0 iff a plugin is marked codex_only in the registry.
#
# The codex_only mirror of _is_claude_only.py. The generator reads the flag
# directly; this exit-code-as-boolean shape lets a bash caller do:
#
#   if PLUGIN_NAME=foo REGISTRY=... python3 scripts/_is_codex_only.py; then
#       # codex_only → skip Claude-specific work
#   fi
#
# Args (env):
#   PLUGIN_NAME   plugin to look up
#   REGISTRY      path to registry/plugins.json
#
# Exit codes:
#   0  plugin exists and codex_only is true
#   1  plugin exists and codex_only is false/missing — OR plugin not found
#      (treated as "not codex_only")
#   2  missing required env var (helpful message instead of cryptic KeyError)
import json
import os
import sys


def require_env(name: str) -> str:
    val = os.environ.get(name)
    if not val:
        sys.stderr.write(
            f"_is_codex_only: missing required env var {name}.\n"
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
            return 0 if p.get("codex_only") else 1
    return 1


if __name__ == "__main__":
    sys.exit(main())
