//! Agent identity + cross-platform config-directory resolution.

use std::path::PathBuf;

/// The two agents the porter bridges.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Agent {
    Claude,
    Codex,
}

impl Agent {
    /// Parse a `--source`/`--target` value. Accepts both the short name and the
    /// `metadata.compatibility` tag so callers can pass either vocabulary.
    pub fn parse(s: &str) -> std::result::Result<Agent, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Ok(Agent::Claude),
            "codex" | "codex-cli" => Ok(Agent::Codex),
            other => Err(format!(
                "unknown agent {other:?} (expected 'claude' or 'codex')"
            )),
        }
    }

    /// Short, stable identifier — used in skill-name prefixes and markers.
    pub fn as_str(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
        }
    }

    /// Prefix applied to a skill ported *from* this agent, e.g. `codex-`. The
    /// prefix guarantees a ported skill can never collide with a native one of
    /// the same name in the target, and makes provenance obvious to the user.
    pub fn prefix(self) -> String {
        format!("{}-", self.as_str())
    }

    /// The environment variable that overrides this agent's config dir, and the
    /// default `$HOME`-relative subdirectory when it is unset.
    fn env_and_subdir(self) -> (&'static str, &'static str) {
        match self {
            Agent::Claude => ("CLAUDE_CONFIG_DIR", ".claude"),
            Agent::Codex => ("CODEX_HOME", ".codex"),
        }
    }

    /// Resolve the agent's config/home directory. Honors the agent-specific
    /// override env var first, then falls back to `$HOME/.<agent>` (or
    /// `%USERPROFILE%\.<agent>` on Windows).
    pub fn config_dir(self) -> std::result::Result<PathBuf, String> {
        let (env_key, subdir) = self.env_and_subdir();
        if let Some(v) = std::env::var_os(env_key) {
            if !v.is_empty() {
                return Ok(PathBuf::from(v));
            }
        }
        home_dir()
            .map(|h| h.join(subdir))
            .ok_or_else(|| "cannot determine home directory (no HOME or USERPROFILE)".to_string())
    }

    /// The directory user-level skills live in: `<config_dir>/skills`.
    pub fn skills_dir(self) -> std::result::Result<PathBuf, String> {
        Ok(self.config_dir()?.join("skills"))
    }
}

/// The user's home directory, cross-platform: `HOME` (Unix) then `USERPROFILE`
/// (Windows). Returns `None` only in the pathological case where neither is set.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("USERPROFILE")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_vocabularies() {
        assert_eq!(Agent::parse("claude").unwrap(), Agent::Claude);
        assert_eq!(Agent::parse("claude-code").unwrap(), Agent::Claude);
        assert_eq!(Agent::parse("CODEX").unwrap(), Agent::Codex);
        assert_eq!(Agent::parse("codex-cli").unwrap(), Agent::Codex);
        assert!(Agent::parse("gemini").is_err());
    }

    #[test]
    fn prefixes_are_distinct() {
        assert_eq!(Agent::Claude.prefix(), "claude-");
        assert_eq!(Agent::Codex.prefix(), "codex-");
    }
}
