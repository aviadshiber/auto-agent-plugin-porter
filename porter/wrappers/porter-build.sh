#!/usr/bin/env bash
#
# porter-build.sh — shared "ensure the porter binary is built" helper (Unix).
#
# VENDORED, IDENTICAL in every porter plugin. Sourced by porter-sync.sh and
# porter-bootstrap.sh so the build/cache logic lives in exactly one place.
#
# Sets $PORTER_BIN to the path of a ready-to-run binary. Builds it ONCE (first
# run, or when the crate source hash changes) under the plugin data dir; the
# fast path is pure file-hashing + no cargo. On failure it prints to stderr and
# returns non-zero — callers decide whether to exit 0 (session hooks always do).

porter_ensure_built() {
  # $1 = plugin root (dir containing porter/ and scripts/)
  local plugin_root="$1"
  local crate_dir="$plugin_root/porter"

  local data_dir="${CLAUDE_PLUGIN_DATA:-${PLUGIN_DATA:-${XDG_CACHE_HOME:-${HOME:-${USERPROFILE:-/tmp}}/.cache}/auto-agent-plugin-porter}}"
  local bin_dir="$data_dir/bin"
  local target_dir="$data_dir/target"
  PORTER_BIN="$bin_dir/agent-porter"
  local stamp="$bin_dir/.src-sha"

  mkdir -p "$bin_dir"

  _porter_hasher() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256
    elif command -v sha256sum >/dev/null 2>&1; then sha256sum
    else cat; fi
  }
  _porter_src_hash() {
    {
      cat "$crate_dir/Cargo.toml" "$crate_dir/Cargo.lock" 2>/dev/null || true
      find "$crate_dir/src" -type f -print0 2>/dev/null | sort -z | xargs -0 cat 2>/dev/null || true
    } | _porter_hasher | cut -d' ' -f1
  }

  local need_build=0
  if [ ! -x "$PORTER_BIN" ]; then
    need_build=1
  elif [ ! -f "$stamp" ] || [ "$(cat "$stamp" 2>/dev/null || true)" != "$(_porter_src_hash)" ]; then
    need_build=1
  fi

  if [ "$need_build" -eq 1 ]; then
    if ! command -v cargo >/dev/null 2>&1; then
      echo "agent-porter: Rust toolchain not found — install from https://rustup.rs to enable skill porting." >&2
      return 1
    fi
    echo "agent-porter: building the porter binary (first run or source changed; this can take ~30-90s once)…" >&2
    # Build OUTSIDE the plugin dir: the plugin install/cache dir may be
    # read-only, and its target/ would be thrown away on every plugin upgrade.
    # CARGO_TARGET_DIR points at the (writable, persistent) data dir, so
    # incremental artifacts survive across sessions and upgrades.
    if ! ( cd "$crate_dir" && CARGO_TARGET_DIR="$target_dir" cargo build --release --quiet ); then
      echo "agent-porter: build failed." >&2
      return 1
    fi
    cp "$target_dir/release/agent-porter" "$PORTER_BIN"
    _porter_src_hash > "$stamp"
  fi
  return 0
}
