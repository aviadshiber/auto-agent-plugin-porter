//! agent-porter — port skills between Claude Code and OpenAI Codex CLI.
//!
//! One canonical crate, vendored identically into both marketplace plugins.
//! The sync is **one-way per invocation** (source → target), **hash-gated**
//! (only changed skills are rewritten), and **loop-safe** (skills that the
//! porter itself generated carry an identity marker and are never re-ported or
//! clobbered). This module is the library; `main.rs` is a thin CLI over it.
//!
//! Design invariants (see AGENTS.md / the plugin SKILL.md for the why):
//!   * The target mirror is *generated, never hand-edited* — like a compiler
//!     output. Its single source of truth is the source skill.
//!   * Every generated skill carries `metadata.ported_by = PORTER_ID`. The
//!     enumerator skips any source carrying it (prevents A→B→A loops), and the
//!     writer refuses to overwrite a target that lacks it (protects the user's
//!     own hand-authored skills).
//!   * Paths are derived from the environment (`CLAUDE_CONFIG_DIR`/`CODEX_HOME`
//!     with `HOME`/`USERPROFILE` fallback), never hard-coded — correct across
//!     macOS, Linux, and Windows.

pub mod agent;
pub mod frontmatter;
pub mod hashing;
pub mod hooks;
pub mod sync;

/// Identity stamped into every generated skill's `metadata.ported_by`. The
/// enumerator and the writer both key off this exact string, so it must be
/// stable across versions.
pub const PORTER_ID: &str = "auto-agent-plugin-porter";

/// This crate's version, stamped into generated skills for diagnostics.
pub const PORTER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Crate-wide result type. A boxed error keeps the dependency surface tiny;
/// the CLI turns these into a non-zero exit + stderr message.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
