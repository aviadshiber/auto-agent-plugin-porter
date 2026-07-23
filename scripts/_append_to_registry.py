#!/usr/bin/env python3
# _append_to_registry.py — add a new plugin entry to registry/plugins.json.
#
# Internal helper for scripts/new-plugin.sh. The shell driver has already
# scaffolded the plugin tree; this script only mutates the registry. If the
# plugin is already present the registry is left untouched (exit 0) so the
# scaffolder stays idempotent during reruns.
#
# Args (env):
#   SKILL_NAME    new plugin's name (must match ^[a-z][a-z0-9-]*$)
#   DESCRIPTION   one-line description (caller already truncates)
#   REGISTRY      path to registry/plugins.json
#   SKILL_OWNER   default owner username (caller passes $(whoami))
#
# Defaults populated for required schema fields so the registry stays valid
# after append; the caller still must edit keywords/category/owners before
# committing. We use the plugin's own name as a placeholder keyword and the
# importing user's username as the placeholder owner — both are non-empty
# strings that satisfy the schema's minItems:1 + minLength:1 constraints.
#
# Exit codes:
#   0  appended (or already present, no-op)
#   1  unexpected error or missing/invalid env var
import json
import os
import re
import sys

NAME_RE = re.compile(r"^[a-z][a-z0-9-]*$")


def require_env(name: str) -> str:
    val = os.environ.get(name)
    if not val:
        sys.stderr.write(
            f"_append_to_registry: missing required env var {name}. "
            "Call via scripts/new-plugin.sh, not directly.\n"
        )
        sys.exit(1)
    return val


def main() -> int:
    registry_path = require_env("REGISTRY")
    name = require_env("SKILL_NAME")
    desc = require_env("DESCRIPTION")
    owner = require_env("SKILL_OWNER")
    # Optional: caller may override the placeholder category. Defaults to
    # "documentation" to match the schema enum and the common plugin shape.
    category = os.environ.get("CATEGORY", "documentation")

    if not NAME_RE.match(name):
        sys.stderr.write(
            f"_append_to_registry: invalid plugin name {name!r}. "
            "Must match ^[a-z][a-z0-9-]*$.\n"
        )
        return 1

    with open(registry_path) as f:
        data = json.load(f)

    if any(p["name"] == name for p in data["plugins"]):
        sys.stderr.write(
            f"Plugin '{name}' already exists in registry. "
            "Edit registry/plugins.json directly to update.\n"
        )
        return 0

    data["plugins"].append({
        "name": name,
        "version": "0.1.0",
        "description": desc,
        "category": category,
        "keywords": [name],
        "owners": [owner],
    })

    with open(registry_path, "w") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"Added {name} to registry/plugins.json (review keywords, category, owners before committing)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
