#!/usr/bin/env python3
# _bump_registry_version.py — bump a plugin's version in registry/plugins.json.
#
# Internal helper for scripts/bump-version.sh. Reads the registry, mutates the
# named plugin's version per the bump kind, writes the registry back, and
# prints "<old>|<new>" to stdout for the caller to consume.
#
# Pure semver logic lives in `bump_semver()` so it can be unit-tested without
# touching the filesystem (see scripts/tests/test_bump_registry_version.py).
#
# Args (env, not CLI, to avoid shell-injection vectors):
#   PLUGIN_NAME   plugin to bump
#   BUMP_TYPE     patch | minor | major
#   REGISTRY      path to registry/plugins.json
#
# Exit codes:
#   0  bumped successfully ("<old>|<new>" on stdout)
#   1  unexpected error (exception trace) or missing env var
#   2  plugin not found in registry
import json
import os
import sys


def bump_semver(version: str, kind: str) -> str:
    """Pure version bumper — returns the new semver string."""
    parts = version.split(".")
    if len(parts) != 3:
        raise ValueError(f"not semver: {version!r}")
    try:
        major, minor, patch = (int(x) for x in parts)
    except ValueError as e:
        raise ValueError(f"not semver: {version!r}") from e
    if kind == "major":
        return f"{major + 1}.0.0"
    if kind == "minor":
        return f"{major}.{minor + 1}.0"
    if kind == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise ValueError(f"unknown bump kind: {kind!r}")


def require_env(name: str) -> str:
    val = os.environ.get(name)
    if not val:
        sys.stderr.write(
            f"_bump_registry_version: missing required env var {name}. "
            "Call via scripts/bump-version.sh, not directly.\n"
        )
        sys.exit(1)
    return val


def main() -> int:
    registry_path = require_env("REGISTRY")
    plugin_name = require_env("PLUGIN_NAME")
    bump = require_env("BUMP_TYPE")

    with open(registry_path) as f:
        data = json.load(f)

    found = next((p for p in data["plugins"] if p["name"] == plugin_name), None)
    if found is None:
        sys.stderr.write(f"Plugin not found in registry: {plugin_name}\n")
        return 2

    old = found["version"]
    new = bump_semver(old, bump)
    found["version"] = new

    with open(registry_path, "w") as f:
        json.dump(data, f, indent=2, ensure_ascii=False)
        f.write("\n")

    print(f"{old}|{new}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
