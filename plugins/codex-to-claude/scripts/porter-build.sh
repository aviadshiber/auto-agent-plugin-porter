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

  _porter_have_hasher() {
    command -v shasum >/dev/null 2>&1 || command -v sha256sum >/dev/null 2>&1
  }
  _porter_hasher() {
    if command -v shasum >/dev/null 2>&1; then shasum -a 256
    else sha256sum; fi
  }
  _porter_src_hash() {
    {
      cat "$crate_dir/Cargo.toml" "$crate_dir/Cargo.lock" 2>/dev/null || true
      find "$crate_dir/src" -type f -print0 2>/dev/null | sort -z | xargs -0 cat 2>/dev/null || true
    } | _porter_hasher | cut -d' ' -f1
  }

  # Staleness decision, cheapest check first:
  #   1. no binary / no stamp        → build
  #   2. mtime quick check: is any crate file NEWER than the stamp? If not, the
  #      binary is current — fast path, NO content hash, NO cargo (this is the
  #      overwhelmingly common per-session case).
  #   3. something is newer → confirm with the content hash (handles a
  #      touch-without-change); rebuild only if content actually differs. With
  #      no SHA tool available we cannot verify content, so rebuild to be safe
  #      (never emit a bogus hash).
  local need_build=0
  if [ ! -x "$PORTER_BIN" ] || [ ! -f "$stamp" ]; then
    need_build=1
  elif find "$crate_dir/Cargo.toml" "$crate_dir/Cargo.lock" "$crate_dir/src" \
        -type f -newer "$stamp" -print 2>/dev/null | grep -q .; then
    if _porter_have_hasher; then
      if [ "$(cat "$stamp" 2>/dev/null || true)" != "$(_porter_src_hash)" ]; then
        need_build=1
      else
        touch "$stamp"   # content identical; reset the mtime baseline
      fi
    else
      need_build=1
    fi
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
    # Record the content hash when we can; otherwise an empty stamp still works
    # (the mtime quick check drives the no-hasher path — content is never read).
    if _porter_have_hasher; then _porter_src_hash > "$stamp"; else : > "$stamp"; fi
  fi
  return 0
}
