//! agent-porter CLI — a thin shell over the `agent_porter` library.
//!
//! Subcommands:
//!   sync --source <a> --target <b> [--source-dir D] [--target-dir D]
//!        [--dry-run] [--no-prune] [--quiet]
//!   install-codex-hook [--codex-home D] [--porter-bin P] [--dry-run]
//!   doctor
//!
//! `sync` is the hot path run at every session start: it exits 0 even when
//! individual skills fail to port (a session must not be blocked by one bad
//! skill), reporting failures on stderr.

use agent_porter::agent::Agent;
use agent_porter::hooks::{self, InstallOptions};
use agent_porter::sync::{self, SyncOptions};
use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("agent-porter: error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> agent_porter::Result<ExitCode> {
    match args.first().map(String::as_str) {
        Some("sync") => cmd_sync(&args[1..]),
        Some("install-codex-hook") => cmd_install_codex_hook(&args[1..]),
        Some("doctor") => cmd_doctor(),
        Some("--version") | Some("-V") => {
            println!("agent-porter {}", agent_porter::PORTER_VERSION);
            Ok(ExitCode::SUCCESS)
        }
        Some("--help") | Some("-h") | Some("help") | None => {
            print_help();
            Ok(ExitCode::SUCCESS)
        }
        Some(other) => {
            eprintln!("agent-porter: unknown command {other:?}\n");
            print_help();
            Ok(ExitCode::FAILURE)
        }
    }
}

fn cmd_sync(args: &[String]) -> agent_porter::Result<ExitCode> {
    let source = Agent::parse(&require_opt(args, "source")?)?;
    let target = Agent::parse(&require_opt(args, "target")?)?;
    if source == target {
        return Err("--source and --target must be different agents".into());
    }

    let source_dir = match get_opt(args, "source-dir") {
        Some(d) => PathBuf::from(d),
        None => source.config_dir()?,
    };
    let target_dir = match get_opt(args, "target-dir") {
        Some(d) => PathBuf::from(d),
        None => target.config_dir()?,
    };

    let opts = SyncOptions {
        source,
        target,
        source_dir,
        target_dir,
        dry_run: has_flag(args, "dry-run"),
        prune: !has_flag(args, "no-prune"),
    };

    let report = sync::sync(&opts)?;

    if !has_flag(args, "quiet") {
        eprintln!("{}", report.summary(source, target));
    }
    for e in &report.errors {
        eprintln!("agent-porter: warning: {e}");
    }
    // Session-start resilience: never fail the session on per-skill errors.
    Ok(ExitCode::SUCCESS)
}

fn cmd_install_codex_hook(args: &[String]) -> agent_porter::Result<ExitCode> {
    let codex_home = match get_opt(args, "codex-home") {
        Some(d) => PathBuf::from(d),
        None => Agent::Codex.config_dir()?,
    };
    let porter_bin = match get_opt(args, "porter-bin") {
        Some(p) => p,
        None => std::env::current_exe()?.to_string_lossy().into_owned(),
    };

    let outcome = hooks::install_codex_session_hook(&InstallOptions {
        codex_home: codex_home.clone(),
        porter_bin,
        dry_run: has_flag(args, "dry-run"),
    })?;

    println!(
        "install-codex-hook: {:?} → {}",
        outcome,
        codex_home.join("hooks.json").display()
    );
    println!(
        "Note: Codex requires you to trust this hook once (it will prompt on the \
         next session, or run `codex` and approve). The porter never bypasses \
         hook trust."
    );
    Ok(ExitCode::SUCCESS)
}

fn cmd_doctor() -> agent_porter::Result<ExitCode> {
    println!("agent-porter {}", agent_porter::PORTER_VERSION);
    for a in [Agent::Claude, Agent::Codex] {
        match a.config_dir() {
            Ok(d) => println!(
                "  {:<7} config: {}  (skills dir present: {})",
                a.as_str(),
                d.display(),
                d.join("skills").is_dir()
            ),
            Err(e) => println!("  {:<7} config: <unresolved: {e}>", a.as_str()),
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn print_help() {
    println!(
        "agent-porter {} — port skills between Claude Code and OpenAI Codex CLI\n\
         \n\
         USAGE:\n\
         \x20 agent-porter sync --source <claude|codex> --target <claude|codex> [options]\n\
         \x20 agent-porter install-codex-hook [--codex-home DIR] [--porter-bin PATH] [--dry-run]\n\
         \x20 agent-porter doctor\n\
         \n\
         SYNC OPTIONS:\n\
         \x20 --source-dir DIR   override the source agent config dir (default: resolved from env)\n\
         \x20 --target-dir DIR   override the target agent config dir\n\
         \x20 --dry-run          compute the plan, write nothing\n\
         \x20 --no-prune         keep mirrors whose source skill was deleted\n\
         \x20 --quiet            suppress the summary line",
        agent_porter::PORTER_VERSION
    );
}

// ── tiny arg helpers (no external CLI dep → faster first build) ──

fn get_opt(args: &[String], name: &str) -> Option<String> {
    let long = format!("--{name}");
    let eq = format!("--{name}=");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == &long {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix(&eq) {
            return Some(v.to_string());
        }
    }
    None
}

fn require_opt(args: &[String], name: &str) -> agent_porter::Result<String> {
    get_opt(args, name).ok_or_else(|| format!("--{name} is required").into())
}

fn has_flag(args: &[String], name: &str) -> bool {
    let long = format!("--{name}");
    args.iter().any(|a| a == &long)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn get_opt_space_and_equals_forms() {
        let a = args(&["--source", "codex", "--target=claude"]);
        assert_eq!(get_opt(&a, "source").as_deref(), Some("codex"));
        assert_eq!(get_opt(&a, "target").as_deref(), Some("claude"));
        assert_eq!(get_opt(&a, "missing"), None);
    }

    #[test]
    fn has_flag_matches_exact_long() {
        let a = args(&["--dry-run", "--source", "codex"]);
        assert!(has_flag(&a, "dry-run"));
        assert!(!has_flag(&a, "no-prune"));
    }

    #[test]
    fn require_opt_errors_when_absent() {
        assert!(require_opt(&args(&["--target", "claude"]), "source").is_err());
        assert!(require_opt(&args(&["--source", "codex"]), "source").is_ok());
    }
}
