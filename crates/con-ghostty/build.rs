use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const GHOSTTY_REPO: &str = "https://github.com/ghostty-org/ghostty.git";
/// Pinned Ghostty revision. Bump together when updating either the
/// macOS full-libghostty build or the Windows libghostty-vt build —
/// both consume the same source tree to keep VT semantics in sync.
const GHOSTTY_REV: &str = "e740f6fc117971da9df9fc957a706e6d96554aa5";
const GHOSTTY_ENV: &str = "CON_GHOSTTY_SOURCE_DIR";

fn main() {
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARGET_OS");
    println!("cargo:rerun-if-env-changed={GHOSTTY_ENV}");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();

    match target_os.as_str() {
        "macos" => build_macos(),
        "windows" => build_windows(),
        other => {
            // Linux + everything else: terminal pane is a placeholder
            // until that platform's backend lands. The crate compiles
            // to a no-op stub via `src/stub.rs`.
            println!(
                "cargo:warning=con-ghostty: skipping native build on target_os={other} \
                 (using stub backend — see docs/impl/windows-port.md)"
            );
        }
    }
}

// ── macOS ─────────────────────────────────────────────────────────────

fn build_macos() {
    let ghostty_dir = resolve_ghostty_source();

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

    let lib_path = find_lib(&ghostty_dir, "libghostty-fat.a");
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
    println!("cargo:rustc-link-lib=c++");

    if env::var_os(GHOSTTY_ENV).is_some() {
        println!("cargo:rerun-if-changed={}", ghostty_dir.join("src").display());
        println!(
            "cargo:rerun-if-changed={}",
            ghostty_dir.join("include/ghostty.h").display()
        );
    }

    let include_dir = ghostty_dir.join("include");
    println!("cargo:include={}", include_dir.display());
}

// ── Windows ───────────────────────────────────────────────────────────

fn build_windows() {
    // libghostty-vt is the cross-platform VT parser carved out of
    // Ghostty (PR ghostty-org/ghostty#8840). It builds via `zig build`
    // and produces a static library `libghostty-vt.a` we link into
    // `con-ghostty`. ConPTY + the D3D11 renderer + the WS_CHILD host
    // live in `src/windows/` and don't need this library directly,
    // but `src/windows/vt.rs` does — its FFI declarations resolve to
    // the symbols `libghostty-vt.a` exports.

    if env::var_os("CON_SKIP_GHOSTTY_VT").is_some() {
        // Escape hatch: skip the upstream build entirely. The
        // `src/windows/vt.rs` symbols become unresolved at link time;
        // intended for IDE / `cargo check` flows where the compile-only
        // pass doesn't link.
        println!(
            "cargo:warning=CON_SKIP_GHOSTTY_VT set — skipping libghostty-vt build. \
             A subsequent `cargo build` will fail to link unless a static \
             library is provided manually."
        );
        return;
    }

    // Detect Zig. Surface the actual error (not-found, permission-denied,
    // etc.) and the PATH we searched so the user can diagnose — previous
    // silent warnings led to confusing link-time failures.
    let zig_bin =
        env::var_os("CON_ZIG_BIN").unwrap_or_else(|| std::ffi::OsString::from("zig"));
    let zig_probe = Command::new(&zig_bin).arg("version").output();
    match zig_probe {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
            println!("cargo:warning=using zig {} (from `{}`)", version, zig_bin.to_string_lossy());
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            panic!(
                "\n\n========================================================\n\
                 con-ghostty: `{bin} version` exited with status {status}.\n\
                 stderr:\n{stderr}\n\
                 ========================================================\n",
                bin = zig_bin.to_string_lossy(),
                status = output.status,
                stderr = stderr,
            );
        }
        Err(err) => {
            let path = env::var("PATH").unwrap_or_default();
            panic!(
                "\n\n========================================================\n\
                 con-ghostty: could not spawn `{bin} version`: {err}\n\n\
                 Zig 0.13+ is required to build libghostty-vt on Windows.\n\
                 Install it from https://ziglang.org/download/ and ensure\n\
                 the `zig` executable is on PATH, or set CON_ZIG_BIN to\n\
                 the absolute path of the zig executable.\n\n\
                 Current PATH: {path}\n\n\
                 To skip this step entirely (the terminal backend will\n\
                 fail to link), set CON_SKIP_GHOSTTY_VT=1.\n\
                 ========================================================\n",
                bin = zig_bin.to_string_lossy(),
                err = err,
                path = path,
            );
        }
    }

    let ghostty_dir = resolve_ghostty_source();

    // `ghostty-vt-static` is the upstream artifact name; it produces
    // `libghostty-vt.a` (or `ghostty-vt.lib` on MSVC). `-Doptimize=ReleaseFast`
    // gives us terminal-class throughput; `-Dsimd=true` enables the
    // SIMD UTF-8 paths.
    let status = Command::new(&zig_bin)
        .args([
            "build",
            "ghostty-vt-static",
            "-Doptimize=ReleaseFast",
            "-Dsimd=true",
        ])
        .current_dir(&ghostty_dir)
        .status()
        .expect("failed to run zig build for libghostty-vt");

    if !status.success() {
        panic!("zig build ghostty-vt-static failed; see output above");
    }

    let lib_name = if cfg!(target_env = "msvc") {
        "ghostty-vt.lib"
    } else {
        "libghostty-vt.a"
    };
    let lib_path = find_lib(&ghostty_dir, lib_name);
    println!(
        "cargo:rustc-link-search=native={}",
        lib_path.parent().unwrap().display()
    );
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    // libghostty-vt has zero Win32 dependencies of its own (only libc).
    // The MSVC default runtime + the windows-rs crate's transitive
    // imports cover everything else.

    if env::var_os(GHOSTTY_ENV).is_some() {
        println!("cargo:rerun-if-changed={}", ghostty_dir.join("src").display());
        println!(
            "cargo:rerun-if-changed={}",
            ghostty_dir.join("include/ghostty/vt.h").display()
        );
    }

    let include_dir = ghostty_dir.join("include");
    println!("cargo:include={}", include_dir.display());
}

// ── Shared helpers ─────────────────────────────────────────────────────

fn resolve_ghostty_source() -> PathBuf {
    if let Some(source_dir) = env::var_os(GHOSTTY_ENV) {
        return PathBuf::from(source_dir)
            .canonicalize()
            .expect("CON_GHOSTTY_SOURCE_DIR points to a missing directory");
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

fn find_lib(ghostty_dir: &PathBuf, lib_name: &str) -> PathBuf {
    let zig_cache = ghostty_dir.join(".zig-cache");
    if zig_cache.exists() {
        let output = Command::new("find")
            .args([zig_cache.to_str().unwrap(), "-name", lib_name, "-type", "f"])
            .output();

        if let Ok(output) = output {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut candidates: Vec<PathBuf> = stdout
                .lines()
                .map(PathBuf::from)
                .filter(|p| p.exists())
                .collect();

            candidates.sort_by(|a, b| {
                let a_time = std::fs::metadata(a).and_then(|m| m.modified()).ok();
                let b_time = std::fs::metadata(b).and_then(|m| m.modified()).ok();
                b_time.cmp(&a_time)
            });

            if let Some(path) = candidates.first() {
                return path.clone();
            }
        }
    }

    // Cross-platform fallback walk for systems without `find` (e.g.
    // Windows without WSL). Manual scan of common output dirs.
    for relative in ["zig-out/lib", "macos/build/Debug"] {
        let candidate = ghostty_dir.join(relative).join(lib_name);
        if candidate.exists() {
            return candidate;
        }
    }

    panic!(
        "Could not find {lib_name} after building ghostty source at {}",
        ghostty_dir.display()
    );
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
    String::from_utf8(output.stdout).ok().map(|s| s.trim().to_string())
}

fn run(command: &mut Command, context: &str) {
    let status = command
        .status()
        .unwrap_or_else(|err| panic!("{context}: {err}"));
    if !status.success() {
        panic!("{context}");
    }
}
