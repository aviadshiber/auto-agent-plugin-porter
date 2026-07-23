#!/usr/bin/env python3
# validate-registry.py — JSON Schema validation for registry/plugins.json.
#
# Used by scripts/validate-json.sh and the GitHub Actions CI. Provides clear,
# contributor-friendly error messages on schema violations (unlike the
# cryptic stack traces from raw `jsonschema` invocation).
#
# Usage:
#   python3 scripts/validate-registry.py
#
# Exit codes:
#   0  registry valid
#   1  registry invalid (errors printed to stderr)
#   2  jsonschema not installed (advisory; CI/setup should install it)
import json
import os
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
# Paths default to the repo's registry/schema but may be overridden via the
# REGISTRY / SCHEMA env vars (used by tests to validate a throwaway copy
# instead of mutating the real registry in place).
REGISTRY = Path(os.environ.get("REGISTRY", REPO / "registry" / "plugins.json"))
SCHEMA = Path(os.environ.get("SCHEMA", REPO / "registry" / "schema.json"))


def main() -> int:
    try:
        import jsonschema
    except ImportError:
        print(
            "validate-registry: jsonschema not installed — "
            "install with `pip install jsonschema` (or `pip3 install --user jsonschema`).",
            file=sys.stderr,
        )
        return 2

    registry = json.loads(REGISTRY.read_text())
    schema = json.loads(SCHEMA.read_text())

    validator = jsonschema.Draft202012Validator(schema)
    errors = sorted(validator.iter_errors(registry), key=lambda e: list(e.absolute_path))
    if not errors:
        n = len(registry["plugins"])
        print(f"validate-registry: OK ({n} plugins, schema valid)")
        return 0

    for err in errors:
        path = "/".join(str(p) for p in err.absolute_path) or "<root>"
        # Try to surface plugin name when the error lives inside a plugin entry
        plugin_hint = ""
        try:
            if err.absolute_path and err.absolute_path[0] == "plugins":
                idx = int(err.absolute_path[1])
                plugin = registry["plugins"][idx]
                plugin_hint = f" (plugin: {plugin.get('name', f'#{idx}')})"
        except (IndexError, KeyError, ValueError):
            pass
        print(f"  ERROR at {path}{plugin_hint}: {err.message}", file=sys.stderr)

    print(f"\nvalidate-registry: {len(errors)} error(s) in registry/plugins.json", file=sys.stderr)
    return 1


if __name__ == "__main__":
    sys.exit(main())
