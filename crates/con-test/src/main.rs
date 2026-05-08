mod parser;
mod runner;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "con-test",
    about = "Logic test runner for con-cli — drives a live con session via .test files"
)]
struct Cli {
    /// Paths to .test files or directories containing .test files
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Path to the con binary to launch (defaults to sibling in cargo target/)
    #[arg(long, value_name = "PATH")]
    con: Option<PathBuf>,

    /// Path to the con-cli binary (defaults to sibling in cargo target/)
    #[arg(long, value_name = "PATH")]
    con_cli: Option<PathBuf>,

    /// Socket path for the launched con process (default: /tmp/con-test-<pid>.sock)
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    /// Seconds to wait for con to start (default: 30)
    #[arg(long, default_value_t = 30)]
    startup_timeout: u64,

    /// Rewrite expected output in .test files from actual output (baseline mode)
    #[arg(long)]
    rewrite: bool,

    /// Stop after the first failing test file
    #[arg(long)]
    fail_fast: bool,

    /// Show full pass/skip results in addition to failures
    #[arg(long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let con_bin = match resolve_bin(cli.con.as_deref(), "con") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let con_cli = match resolve_bin(cli.con_cli.as_deref(), "con-cli") {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    let socket = cli.socket.unwrap_or_else(|| {
        std::env::temp_dir().join(format!("con-test-{}.sock", std::process::id()))
    });

    let test_files = match collect_test_files(&cli.paths) {
        Ok(files) => files,
        Err(e) => {
            eprintln!("error collecting test files: {e}");
            return ExitCode::FAILURE;
        }
    };

    if test_files.is_empty() {
        eprintln!("no .test files found in the given paths");
        return ExitCode::FAILURE;
    }

    // Launch a single con process for the entire test run.
    println!("launching con ({})...", con_bin.display());
    let _con_process = match runner::ConProcess::launch(
        &con_bin,
        &socket,
        Duration::from_secs(cli.startup_timeout),
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: failed to launch con: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("con ready on {}\n", socket.display());

    let config = runner::RunConfig {
        con_cli: con_cli.clone(),
        socket: socket.clone(),
        rewrite: cli.rewrite,
        verbose: cli.verbose,
    };

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for path in &test_files {
        // Reset con to a clean baseline before each file.
        if let Err(e) = runner::reset_state(&con_cli, &socket) {
            eprintln!("error: reset_state failed before {}: {e}", path.display());
            failed += 1;
            if cli.fail_fast {
                break;
            }
            continue;
        }

        match runner::run_file(path, &config) {
            Ok(result) => {
                print_file_result(path, &result);
                passed += result.passed;
                failed += result.failed;
                skipped += result.skipped;
                if cli.fail_fast && result.failed > 0 {
                    break;
                }
            }
            Err(e) => {
                eprintln!("error running {}: {e}", path.display());
                failed += 1;
                if cli.fail_fast {
                    break;
                }
            }
        }
    }

    println!();
    println!(
        "results: {} passed, {} failed, {} skipped  ({} files)",
        passed,
        failed,
        skipped,
        test_files.len()
    );

    if failed > 0 {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn print_file_result(path: &Path, result: &runner::FileResult) {
    let status = if result.failed > 0 { "FAIL" } else { "ok  " };
    println!(
        "{status}  {}  ({} passed, {} failed, {} skipped)",
        path.display(),
        result.passed,
        result.failed,
        result.skipped
    );
    for failure in &result.failures {
        println!("  --- FAIL: {}", failure.step_label);
        println!("      cmd:      {}", failure.cmd);
        println!("      expected: {}", failure.expected.trim());
        println!("      actual:   {}", failure.actual.trim());
        if !failure.diff.is_empty() {
            for line in failure.diff.lines() {
                println!("      {line}");
            }
        }
    }
}

/// Resolve a binary path.
/// Priority: explicit flag → CON_<NAME> env var → cargo target/ sibling → PATH
fn resolve_bin(flag: Option<&Path>, name: &str) -> Result<PathBuf> {
    if let Some(p) = flag {
        return Ok(p.to_path_buf());
    }

    let env_key = format!("CON_{}", name.to_uppercase().replace('-', "_"));
    if let Ok(env) = std::env::var(&env_key) {
        return Ok(PathBuf::from(env));
    }

    // Sibling in the same target directory as con-test itself.
    let sibling = {
        let mut p = std::env::current_exe().unwrap_or_default();
        p.pop();
        p.push(name);
        p
    };
    if sibling.exists() {
        return Ok(sibling);
    }

    // Fall back to PATH.
    let output = Command::new("which").arg(name).output();
    match output {
        Ok(o) if o.status.success() => {
            let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            Ok(PathBuf::from(path))
        }
        _ => anyhow::bail!(
            "{name} not found. Build it with `cargo build -p {name}` or pass --{name} <path>"
        ),
    }
}

/// Recursively collect all .test files from the given paths.
fn collect_test_files(paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            collect_from_dir(path, &mut files)?;
        } else if path.extension().map(|e| e == "test").unwrap_or(false) {
            files.push(path.clone());
        } else {
            anyhow::bail!(
                "{} is not a .test file or directory",
                path.display()
            );
        }
    }
    files.sort();
    Ok(files)
}

fn collect_from_dir(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            collect_from_dir(&path, out)?;
        } else if path.extension().map(|e| e == "test").unwrap_or(false) {
            out.push(path);
        }
    }
    Ok(())
}

use std::process::Command;
