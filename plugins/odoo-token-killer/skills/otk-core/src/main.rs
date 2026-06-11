// OTK - Odoo Token Killer
// High-performance CLI proxy to minimize LLM token consumption for Odoo development.
// Inspired by RTK (Rust Token Killer) - https://github.com/rtk-ai/rtk

mod filters;
mod gain;
mod tee;
mod tracking;

use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "otk",
    version,
    about = "Odoo Token Killer - minimize LLM token consumption for Odoo development",
    long_about = "OTK filters and compresses command outputs before they reach your LLM context,\n\
                  saving 60-90% of tokens on common Odoo development operations.\n\n\
                  Inspired by RTK (rtk-ai/rtk). Thanks to the RTK team for pioneering this approach."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Increase verbosity (-v, -vv)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    verbose: u8,
}

#[derive(Subcommand)]
enum Commands {
    /// Show token savings analytics dashboard
    Gain {
        /// Show daily breakdown
        #[arg(long)]
        daily: bool,

        /// Export as JSON
        #[arg(long)]
        json: bool,

        /// Reset tracking database
        #[arg(long)]
        reset: bool,
    },

    /// Read a file with token-optimized filtering
    Read {
        /// File path to read
        path: String,

        /// Filter level: none, minimal, aggressive
        #[arg(short, long, default_value = "minimal")]
        level: String,
    },

    /// Run any command (passed verbatim) and apply the matching output filter.
    ///
    /// This is the entry point the PreToolUse hook uses: it prefixes the FULL
    /// original command with `otk`, e.g. `docker logs web` -> `otk docker logs web`.
    /// OTK runs the command exactly as given and picks a filter by inspecting it.
    #[command(external_subcommand)]
    Run(Vec<String>),
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Gain { daily, json, reset } => {
            gain::run(daily, json, reset)?;
        }
        Commands::Read { path, level } => {
            run_read(&path, &level, cli.verbose)?;
        }
        Commands::Run(args) => {
            run_command(&args, cli.verbose)?;
        }
    }

    Ok(())
}

/// Map a command (already split into tokens, binary first) to a stable filter key.
///
/// The key is intentionally a `&'static str` so it can be unit-tested without
/// comparing function pointers. `filter_for` turns the key into the filter fn.
fn classify(args: &[String]) -> &'static str {
    let a0 = args.first().map(|s| s.as_str()).unwrap_or("");
    let a1 = args.get(1).map(|s| s.as_str()).unwrap_or("");
    let a2 = args.get(2).map(|s| s.as_str()).unwrap_or("");

    match a0 {
        "git" => match a1 {
            "status" => "git_status",
            "diff" => "git_diff",
            "log" => "git_log",
            "add" | "commit" | "push" | "pull" | "fetch" | "checkout" | "stash" | "merge"
            | "rebase" | "tag" | "reset" => "ok",
            _ => "passthrough",
        },
        // `docker logs ...` and `docker compose logs ...` -> log filter; other docker -> docker filter
        "docker" | "docker-compose" | "podman" => {
            if a1 == "logs" || (a1 == "compose" && a2 == "logs") {
                "log"
            } else {
                "docker"
            }
        }
        "invoke" => {
            if a1 == "test" {
                "test"
            } else {
                "passthrough"
            }
        }
        "pytest" | "py.test" => "test",
        "grep" | "rg" | "egrep" | "fgrep" | "ag" => "grep",
        "ls" => "ls",
        "tree" => "ls",
        "find" | "fd" => "ls",
        "pip" | "pip3" | "uv" => "pip",
        "psql" | "mysql" => "sql",
        _ => "passthrough",
    }
}

/// Resolve a filter key to its filter function.
fn filter_for(key: &str) -> fn(&str) -> String {
    match key {
        "test" => filters::test_filter,
        "log" => filters::log_filter,
        "git_status" => filters::git_status_filter,
        "git_diff" => filters::git_diff_filter,
        "git_log" => filters::git_log_filter,
        "ok" => filters::ok_filter,
        "grep" => filters::grep_filter,
        "ls" => filters::ls_filter,
        "docker" => filters::docker_filter,
        "pip" => filters::pip_filter,
        "sql" => filters::sql_filter,
        _ => filters::passthrough,
    }
}

/// A short, filesystem-safe label for tee files (e.g. "git_status", "docker_logs", "ls").
fn tee_name(args: &[String]) -> String {
    let take = match args.first().map(|s| s.as_str()) {
        Some("git") | Some("docker") | Some("docker-compose") | Some("invoke") | Some("pip") => {
            2.min(args.len())
        }
        _ => 1.min(args.len()),
    };
    let raw = args[..take].join("_");
    raw.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Quote one argv element for `sh -c`. Args that are plain
/// alphanumeric/path-ish pass through; anything else gets single-quoted
/// (with embedded `'` escaped as `'\''`).
fn shell_quote(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:=@%+,".contains(c));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', "'\\''"))
    }
}

/// Re-serialize parsed argv into a `sh -c` string. The caller's shell already
/// consumed one layer of quoting, so each element must be re-quoted or
/// patterns like `O4\|O6` lose their backslash and `-E "O4|O6"` turns into a
/// real shell pipe. A single element is treated as a complete raw command
/// string (the `otk proxy "cmd | cmd"` form) and passed through untouched.
fn shell_join(args: &[String]) -> String {
    if args.len() == 1 {
        return args[0].clone();
    }
    args.iter()
        .map(|a| shell_quote(a))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Dispatch a verbatim command: handle the `proxy`/`err` modifiers, otherwise
/// run the command and apply its classified filter.
fn run_command(args: &[String], verbose: u8) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("No command provided");
    }

    match args[0].as_str() {
        // `otk proxy <cmd>` - run without filtering, track for metrics only
        "proxy" => return run_proxy(&args[1..], verbose),
        // `otk err <cmd>` - run any command, keep error/warning lines only
        "err" => {
            if args.len() < 2 {
                anyhow::bail!("No command provided to 'err'");
            }
            let command = shell_join(&args[1..]);
            return exec_filtered(&command, "err", filters::error_filter, true, verbose);
        }
        _ => {}
    }

    let key = classify(args);
    let command = shell_join(args);
    exec_filtered(
        &command,
        &tee_name(args),
        filter_for(key),
        benign_nonzero(key),
        verbose,
    )
}

/// True for commands where a non-zero exit code is benign/expected and the output
/// filter should still run instead of being replaced by a raw error dump:
/// - "test": a failing test suite exits non-zero but we want the filtered failures
/// - "grep": exit 1 means "no matches", not an error
/// - "ls" (ls/tree/find): exit 1 often means "some paths were inaccessible"
fn benign_nonzero(key: &str) -> bool {
    matches!(key, "test" | "grep" | "ls")
}

/// Execute `command` via `sh -c`, apply `filter_fn`, tee full output, track metrics,
/// and preserve the child's exit code.
///
/// `filter_on_failure` controls what happens on a non-zero exit:
/// - `false` (most commands): show the raw error tail, do NOT filter. This stops a
///   filter from masking a real error as success (e.g. `git_status_filter` printing
///   "Clean working tree." when git actually failed).
/// - `true` (test runners, grep, find/ls): still apply the filter, because a non-zero
///   exit is benign/expected there (failing tests are the point; grep exit 1 = no
///   matches; find exit 1 = some paths inaccessible but results are valid).
///
/// Exit codes 126/127 (cannot execute / command not found) are ALWAYS treated as a
/// hard failure regardless of `filter_on_failure`, so a launcher that never ran
/// (e.g. `invoke: not found`) is never reported as success.
fn exec_filtered(
    command: &str,
    label: &str,
    filter_fn: fn(&str) -> String,
    filter_on_failure: bool,
    verbose: u8,
) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("[otk] Running: {}", command);
    }

    let output = std::process::Command::new("sh")
        .args(["-c", command])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute '{}': {}", command, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let raw = if stderr.is_empty() {
        stdout.to_string()
    } else if stdout.is_empty() {
        stderr.to_string()
    } else {
        format!("{}\n{}", stdout, stderr)
    };

    let exit_code = output.status.code().unwrap_or(1);

    let launch_failure = exit_code == 126 || exit_code == 127;
    let hard_failure = exit_code != 0 && (launch_failure || !filter_on_failure);

    let raw_tail = || {
        let lines: Vec<&str> = raw.lines().collect();
        let n = lines.len().min(20);
        lines[lines.len() - n..].join("\n")
    };

    let final_output = if hard_failure {
        // Show the raw error tail verbatim so a filter can't mask the failure.
        if raw.trim().is_empty() {
            format!("Command failed (exit code {}).", exit_code)
        } else {
            format!("Command failed (exit code {}):\n{}", exit_code, raw_tail())
        }
    } else {
        let filtered = filter_fn(&raw);
        if filtered.trim().is_empty() {
            if exit_code == 0 {
                "ok".to_string()
            } else {
                raw_tail()
            }
        } else {
            filtered
        }
    };

    if let Some(hint) = tee::tee_and_hint(&raw, label, exit_code) {
        println!("{}\n{}", final_output, hint);
    } else {
        println!("{}", final_output);
    }

    timer.track(command, &format!("otk {}", label), &raw, &final_output);

    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

/// Read a file with language-aware filtering.
fn run_read(path: &str, level: &str, verbose: u8) -> Result<()> {
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("[otk] Reading: {} (level: {})", path, level);
    }

    let raw = std::fs::read_to_string(path)
        .map_err(|e| anyhow::anyhow!("Failed to read '{}': {}", path, e))?;

    let filtered = if path.ends_with(".xml") || path.ends_with(".html") {
        filters::xml_filter(&raw)
    } else if path.ends_with(".py") {
        match level {
            "aggressive" => filters::python_filter_aggressive(&raw),
            "none" => raw.clone(),
            _ => filters::python_filter(&raw),
        }
    } else {
        raw.clone()
    };

    // Always tee file reads (agent may need full source)
    if let Some(hint) = tee::tee_always(&raw, &format!("read_{}", path.rsplit('/').next().unwrap_or("file"))) {
        println!("{}\n{}", filtered, hint);
    } else {
        println!("{}", filtered);
    }

    timer.track(
        &format!("cat {}", path),
        &format!("otk read {}", path),
        &raw,
        &filtered,
    );

    Ok(())
}

/// Proxy: run command without filtering, track for metrics only.
fn run_proxy(args: &[String], verbose: u8) -> Result<()> {
    if args.is_empty() {
        anyhow::bail!("No command provided to 'proxy'");
    }
    let command = shell_join(args);
    let timer = tracking::TimedExecution::start();

    if verbose > 0 {
        eprintln!("[otk] Proxy: {}", command);
    }

    let output = std::process::Command::new("sh")
        .args(["-c", &command])
        .output()
        .map_err(|e| anyhow::anyhow!("Failed to execute '{}': {}", command, e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    print!("{}", stdout);
    if !stderr.is_empty() {
        eprint!("{}", stderr);
    }

    // Track with 0% savings (passthrough)
    timer.track_passthrough(&command, &format!("otk proxy {}", command));

    let exit_code = output.status.code().unwrap_or(1);
    if exit_code != 0 {
        std::process::exit(exit_code);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn classify_docker_logs_is_log_filter() {
        assert_eq!(classify(&a(&["docker", "logs", "web"])), "log");
        assert_eq!(classify(&a(&["docker", "compose", "logs", "web"])), "log");
    }

    #[test]
    fn classify_docker_other_is_docker_filter() {
        assert_eq!(classify(&a(&["docker", "ps"])), "docker");
        assert_eq!(classify(&a(&["docker", "images"])), "docker");
    }

    #[test]
    fn classify_git_subcommands() {
        assert_eq!(classify(&a(&["git", "status"])), "git_status");
        assert_eq!(classify(&a(&["git", "diff"])), "git_diff");
        assert_eq!(classify(&a(&["git", "log"])), "git_log");
        assert_eq!(classify(&a(&["git", "commit", "-m", "x"])), "ok");
        assert_eq!(classify(&a(&["git", "show"])), "passthrough");
    }

    #[test]
    fn classify_test_runners() {
        assert_eq!(classify(&a(&["invoke", "test", "sale"])), "test");
        assert_eq!(classify(&a(&["pytest", "tests/"])), "test");
        assert_eq!(classify(&a(&["invoke", "build"])), "passthrough");
    }

    #[test]
    fn classify_search_listing_pkg_sql() {
        assert_eq!(classify(&a(&["grep", "-n", "foo", "x.py"])), "grep");
        assert_eq!(classify(&a(&["rg", "foo"])), "grep");
        assert_eq!(classify(&a(&["ls", "-la"])), "ls");
        assert_eq!(classify(&a(&["find", "/etc", "-name", "hosts"])), "ls");
        assert_eq!(classify(&a(&["tree", "."])), "ls");
        assert_eq!(classify(&a(&["pip", "list"])), "pip");
        assert_eq!(classify(&a(&["psql", "-c", "select 1"])), "sql");
    }

    #[test]
    fn classify_unknown_is_passthrough() {
        assert_eq!(classify(&a(&["echo", "hi"])), "passthrough");
        assert_eq!(classify(&a(&["npm", "test"])), "passthrough");
    }

    #[test]
    fn tee_name_is_filesystem_safe() {
        assert_eq!(tee_name(&a(&["git", "status"])), "git_status");
        assert_eq!(tee_name(&a(&["docker", "logs", "web"])), "docker_logs");
        assert_eq!(tee_name(&a(&["ls", "-la"])), "ls");
    }

    // Argv arrives already parsed by the caller's shell; re-serializing it for
    // `sh -c` must re-quote, or patterns like `O4\|O6` lose their backslash and
    // `-E "O4|O6"` becomes a real shell pipe (`sh: O6: not found`).

    #[test]
    fn shell_join_preserves_bre_alternation_backslash() {
        assert_eq!(
            shell_join(&a(&["grep", "-n", r"O4\|O6", "/p/f.md"])),
            r"grep -n 'O4\|O6' /p/f.md"
        );
    }

    #[test]
    fn shell_join_quotes_shell_metacharacters() {
        assert_eq!(
            shell_join(&a(&["grep", "-nE", "O4|O6", "f.md"])),
            "grep -nE 'O4|O6' f.md"
        );
        assert_eq!(
            shell_join(&a(&["find", ".", "-name", "*.py"])),
            "find . -name '*.py'"
        );
        assert_eq!(
            shell_join(&a(&["psql", "-c", "select * from res_users"])),
            "psql -c 'select * from res_users'"
        );
    }

    #[test]
    fn shell_join_escapes_embedded_single_quotes() {
        assert_eq!(
            shell_join(&a(&["grep", "it's", "f.md"])),
            r"grep 'it'\''s' f.md"
        );
    }

    #[test]
    fn shell_join_leaves_simple_args_unquoted() {
        assert_eq!(
            shell_join(&a(&["git", "log", "--oneline", "-5"])),
            "git log --oneline -5"
        );
    }

    #[test]
    fn shell_join_single_arg_is_raw_command_string() {
        // `otk proxy "docker logs web | tail -5"` — one arg = whole shell command
        assert_eq!(
            shell_join(&a(&["docker logs web | tail -5"])),
            "docker logs web | tail -5"
        );
    }

    #[test]
    fn benign_nonzero_only_for_test_grep_ls() {
        assert!(benign_nonzero("test"));
        assert!(benign_nonzero("grep"));
        assert!(benign_nonzero("ls"));
        assert!(!benign_nonzero("git_status"));
        assert!(!benign_nonzero("git_diff"));
        assert!(!benign_nonzero("ok"));
        assert!(!benign_nonzero("docker"));
        assert!(!benign_nonzero("log"));
        assert!(!benign_nonzero("passthrough"));
    }
}
