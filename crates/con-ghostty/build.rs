use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const GHOSTTY_REPO: &str = "https://github.com/ghostty-org/ghostty.git";
const GHOSTTY_REV: &str = "e740f6fc117971da9df9fc957a706e6d96554aa5";
const GHOSTTY_ENV: &str = "CON_GHOSTTY_SOURCE_DIR";

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed={GHOSTTY_ENV}");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    // libghostty's full embedded C API is macOS-only upstream (April 2026).
    // See docs/impl/windows-port.md for the Windows/Linux porting plan.
    // On non-macOS targets we still expose the crate (so the workspace
    // resolves and `cargo check` passes) but do not invoke `zig build`
    // and do not emit any link instructions.
    if target_os != "macos" {
        println!(
            "cargo:warning=con-ghostty: skipping libghostty build on target_os={target_os} \
             (macOS-only — see docs/impl/windows-port.md)"
        );
        return;
    }

    let ghostty_dir = resolve_ghostty_source();

    // Build libghostty via zig build
    let status = Command::new("zig")
        .args([
            "build",
            "-Dapp-runtime=none",
            "-Dxcframework-target=native",
            "-Demit-macos-app=false",
        ])
        .current_dir(&ghostty_dir)
        .status()
        .expect("failed to run zig build — is zig installed?");

    if !status.success() {
        panic!("zig build failed for libghostty");
    }

    // Find the built library. The zig build produces it in the cache.
    // For macOS, the native target produces a single-arch .a in the zig-cache.
    let lib_path = find_libghostty(&ghostty_dir);

    println!(
        "cargo:rustc-link-search=native={}",
        lib_path.parent().unwrap().display()
    );
    println!("cargo:rustc-link-lib=static=ghostty-fat");

    for framework in &[
        "AppKit",
        "Metal",
        "CoreGraphics",
        "CoreText",
        "CoreVideo",
        "CoreFoundation",
        "Foundation",
        "IOSurface",
        "QuartzCore",
        "Carbon",
    ] {
        println!("cargo:rustc-link-lib=framework={}", framework);
    }
    // C++ runtime (ghostty uses some C++ deps like simdutf)
    println!("cargo:rustc-link-lib=c++");

    // If the caller provided a local Ghostty checkout, track its source changes.
    if env::var_os(GHOSTTY_ENV).is_some() {
        println!("cargo:rerun-if-changed={}", ghostty_dir.join("src").display());
        println!(
            "cargo:rerun-if-changed={}",
            ghostty_dir.join("include/ghostty.h").display()
        );
    }

    // Generate include path for FFI
    let include_dir = ghostty_dir.join("include");
    println!("cargo:include={}", include_dir.display());
}

fn resolve_ghostty_source() -> PathBuf {
    if let Some(source_dir) = env::var_os(GHOSTTY_ENV) {
        let source_dir = PathBuf::from(source_dir)
            .canonicalize()
            .expect("CON_GHOSTTY_SOURCE_DIR points to a missing directory");
        return source_dir;
    }

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing"));
    let vendor_root = out_dir.join("ghostty-src");

    if vendor_root.exists() && current_git_rev(&vendor_root).as_deref() == Some(GHOSTTY_REV) {
        return vendor_root;
    }

    if vendor_root.exists() {
        fs::remove_dir_all(&vendor_root).expect("failed to clear stale vendored ghostty source");
    }

    run(
        Command::new("git").args([
            "clone",
            "--no-checkout",
            GHOSTTY_REPO,
            vendor_root.to_str().expect("non-utf8 ghostty vendor path"),
        ]),
        "failed to clone Ghostty source",
    );
    run(
        Command::new("git")
            .args(["checkout", GHOSTTY_REV])
            .current_dir(&vendor_root),
        "failed to checkout pinned Ghostty revision",
    );

    vendor_root
}

fn find_libghostty(ghostty_dir: &PathBuf) -> PathBuf {
    // Check zig-cache for the freshly built library
    let zig_cache = ghostty_dir.join(".zig-cache");
    if zig_cache.exists() {
        // Walk the cache looking for the most recent libghostty-fat.a
        let output = Command::new("find")
            .args([
                zig_cache.to_str().unwrap(),
                "-name",
                "libghostty-fat.a",
                "-type",
                "f",
            ])
            .output()
            .expect("find command failed");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut candidates: Vec<PathBuf> = stdout
            .lines()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();

        // Sort by modification time (newest first)
        candidates.sort_by(|a, b| {
            let a_time = std::fs::metadata(a).and_then(|m| m.modified()).ok();
            let b_time = std::fs::metadata(b).and_then(|m| m.modified()).ok();
            b_time.cmp(&a_time)
        });

        if let Some(path) = candidates.first() {
            return path.clone();
        }
    }

    // Fallback: check macos/build/Debug
    let fallback = ghostty_dir.join("macos/build/Debug/libghostty-fat.a");
    if fallback.exists() {
        return fallback;
    }

    panic!("Could not find libghostty-fat.a after building pinned Ghostty source");
}

fn current_git_rev(repo_dir: &PathBuf) -> Option<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let rev = String::from_utf8(output.stdout).ok()?;
    Some(rev.trim().to_string())
}

fn run(command: &mut Command, context: &str) {
    let status = command.status().unwrap_or_else(|err| panic!("{context}: {err}"));
    if !status.success() {
        panic!("{context}");
    }
}
