mod parser;
mod runner;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

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

    /// Path to the con-cli binary (defaults to finding it on PATH or in cargo target/)
    #[arg(long, value_name = "PATH")]
    con_cli: Option<PathBuf>,

    /// Override the con socket path (passed as --socket to con-cli)
    #[arg(long, value_name = "PATH")]
    socket: Option<PathBuf>,

    /// Rewrite expected output in .test files from actual output (baseline mode)
    #[arg(long)]
    rewrite: bool,

    /// Stop after the first failing test file
    #[arg(long)]
    fail_fast: bool,

    /// Show full diff even for passing tests
    #[arg(long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let con_cli = match resolve_con_cli(cli.con_cli.as_deref()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

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

    let config = runner::RunConfig {
        con_cli,
        socket: cli.socket,
        rewrite: cli.rewrite,
        verbose: cli.verbose,
    };

    let mut passed = 0usize;
    let mut failed = 0usize;
    let mut skipped = 0usize;

    for path in &test_files {
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

/// Resolve the con-cli binary path.
/// Priority: --con-cli flag → CON_CLI env var → cargo target/debug/con-cli → PATH
fn resolve_con_cli(flag: Option<&Path>) -> Result<PathBuf> {
    if let Some(p) = flag {
        return Ok(p.to_path_buf());
    }
    if let Ok(env) = std::env::var("CON_CLI") {
        return Ok(PathBuf::from(env));
    }
    // Try workspace target directory (useful when running from the repo)
    let workspace_target = {
        let mut p = std::env::current_exe().unwrap_or_default();
        // current_exe is something like target/debug/con-test; sibling is con-cli
        p.pop();
        p.push("con-cli");
        p
    };
    if workspace_target.exists() {
        return Ok(workspace_target);
    }
    // Fall back to PATH
    which_con_cli()
}

fn which_con_cli() -> Result<PathBuf> {
    let output = std::process::Command::new("which")
        .arg("con-cli")
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
            Ok(PathBuf::from(path))
        }
        _ => anyhow::bail!(
            "con-cli not found. Build it with `cargo build -p con-cli` or pass --con-cli <path>"
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
