use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

const GHOSTTY_REPO: &str = "https://github.com/ghostty-org/ghostty.git";
/// Pinned Ghostty revision. Bump together when updating either the
/// macOS full-libghostty build or the Windows libghostty-vt build —
/// both consume the same source tree to keep VT semantics in sync.
///
/// 2026-04-17 bump: from `e740f6fc1...` to `ca7516bea6...`. The older
/// pin predated libghostty-vt's render-state implementation on
/// Windows — `ghostty_render_state_new` was exported as a symbol but
/// dereferenced a null internal function pointer at runtime. The new
/// pin is tip-of-main on 2026-04-17; see the postmortem in
/// docs/impl/windows-port.md.
///
/// If a bump breaks the macOS libghostty build (different zig flags
/// needed), revert by re-pinning to `e740f6fc117971da9df9fc957a706e6d96554aa5`
/// — that's known-good on macOS.
const GHOSTTY_REV: &str = "ca7516bea60190ee2e9a4f9182b61d318d107c6e";
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

    if env::var_os("CON_STUB_GHOSTTY_VT").is_some() {
        // Fallback path: compile the C stub in `src/windows/ghostty_vt_stub.c`
        // and link it instead of libghostty-vt. The resulting binary
        // launches fully — GPUI window, HWND host view, D3D11 swapchain,
        // ConPTY spawn — but the terminal pane paints an empty grid
        // because the stub returns no rows. Useful for iterating the
        // non-VT parts of the backend while Zig / Ghostty build issues
        // are resolved separately. See docs/impl/windows-port.md.
        println!("cargo:warning=CON_STUB_GHOSTTY_VT set — linking stub C implementations \
                  instead of libghostty-vt. Terminal output will be empty.");
        cc::Build::new()
            .file("src/windows/ghostty_vt_stub.c")
            .compile("ghostty_vt_stub");
        // The static lib name above means `-lghostty_vt_stub` is emitted
        // by cc-rs; but our vt.rs binds to `ghostty_*` symbols that the
        // stub provides, so the linker resolves them from ghostty_vt_stub.
        // No extra cargo:rustc-link-lib needed — cc-rs handles it.
        return;
    }

    if env::var_os("CON_SKIP_GHOSTTY_VT").is_some() {
        // Escape hatch: skip both upstream and stub. The vt.rs symbols
        // become unresolved at link time; intended for `cargo check`
        // flows that don't link, or when you plan to supply a static
        // library via other means.
        println!(
            "cargo:warning=CON_SKIP_GHOSTTY_VT set — skipping libghostty-vt build. \
             A subsequent `cargo build` will fail to link unless a static \
             library is provided manually. Consider CON_STUB_GHOSTTY_VT=1 \
             if you want a linkable placeholder."
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

    // Upstream Ghostty exposes libghostty-vt in two shapes depending on
    // revision:
    //   (a) Current main: option `-Demit-lib-vt=true` on the default
    //       `install` step disables the xcframework / macOS-app / docs
    //       and leaves libghostty-vt as the produced artifact.
    //   (b) Older revisions: a named step like `ghostty-vt-static`.
    // We probe `zig build -h` to pick the right invocation. Override
    // the probe via `CON_GHOSTTY_VT_STEP` (step name) or
    // `CON_GHOSTTY_VT_EMIT_OPTION=1` (force the option-based path).
    let invocation = pick_vt_invocation(&zig_bin, &ghostty_dir);

    // `-Doptimize=ReleaseFast` for terminal-class throughput.
    //
    // `-Dsimd`: Ghostty vendors `simdutf` (a C++ SIMD UTF-8 library)
    // when SIMD is on. Zig's `-Demit-lib-vt=true` produces
    // `ghostty-vt-static.lib` that *references* simdutf symbols but
    // doesn't bundle the simdutf C++ objects into the archive — link
    // fails with unresolved `simdutf::convert_utf8_to_utf32` etc.
    // Default off on Windows so the static lib is self-contained; the
    // user can flip via `CON_GHOSTTY_VT_SIMD=1` once we figure out how
    // to link simdutf separately (it's produced somewhere in zig-cache
    // but surfacing it cleanly across revisions is its own project).
    // macOS/Linux: the full libghostty build bundles everything, so
    // `-Dsimd=true` is safe and default there.
    let simd_on = env::var("CON_GHOSTTY_VT_SIMD")
        .map(|s| matches!(s.as_str(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false);
    let simd_flag = if simd_on {
        "-Dsimd=true"
    } else {
        "-Dsimd=false"
    };

    let mut cmd = Command::new(&zig_bin);
    cmd.current_dir(&ghostty_dir);
    cmd.arg("build");
    for arg in &invocation {
        cmd.arg(arg);
    }
    cmd.args(["-Doptimize=ReleaseFast", simd_flag]);

    let status = cmd
        .status()
        .expect("failed to run zig build for libghostty-vt");

    if !status.success() {
        panic!(
            "zig build {:?} failed; see output above.\n\
             If this Ghostty revision doesn't expose libghostty-vt,\n\
             bump GHOSTTY_REV in crates/con-ghostty/build.rs or set\n\
             CON_GHOSTTY_VT_STEP to the correct step name.",
            invocation
        );
    }

    // Zig produces both a shared library (`ghostty-vt.dll` +
    // `ghostty-vt.lib` import-stub on MSVC) and a static archive
    // (`ghostty-vt-static.lib`). We want the static archive — linking
    // the import stub would leave `con-app.exe` dependent on
    // `ghostty-vt.dll` at runtime, which we don't ship.
    //
    // `cfg!()` at build.rs compile time reflects the *host*, not the
    // target. Use the runtime env the cargo build passes us.
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let (static_lib_name, static_link_name, fallback_lib_name, fallback_link_name) =
        if target_env == "msvc" {
            (
                "ghostty-vt-static.lib",
                "ghostty-vt-static",
                "ghostty-vt.lib",
                "ghostty-vt",
            )
        } else {
            (
                "libghostty-vt-static.a",
                "ghostty-vt-static",
                "libghostty-vt.a",
                "ghostty-vt",
            )
        };

    // Prefer static; fall back to the import-lib variant with a loud
    // warning so the user notices the runtime DLL dependency.
    let (lib_path, link_name) = match try_find_lib(&ghostty_dir, static_lib_name) {
        Some(p) => (p, static_link_name),
        None => {
            println!(
                "cargo:warning=libghostty-vt-static not found; linking shared `{fallback_lib_name}` \
                 instead. The resulting executable will depend on ghostty-vt.dll at runtime — \
                 ship it alongside the .exe or bump the Ghostty pin to a revision that emits \
                 the static archive."
            );
            let path = find_lib(&ghostty_dir, fallback_lib_name);
            (path, fallback_link_name)
        }
    };

    println!(
        "cargo:rustc-link-search=native={}",
        lib_path.parent().unwrap().display()
    );
    println!("cargo:rustc-link-lib=static={link_name}");

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

/// Probe `zig build -h` and pick how to invoke a libghostty-vt build.
/// Returns a list of extra arguments to append after `zig build`.
///
/// Priority:
///   1. `CON_GHOSTTY_VT_STEP` env var -> single named step.
///   2. `-Demit-lib-vt=` option in help -> `["-Demit-lib-vt=true"]`.
///   3. Named step matching one of the known candidates.
fn pick_vt_invocation(zig_bin: &std::ffi::OsStr, ghostty_dir: &PathBuf) -> Vec<String> {
    if let Some(val) = env::var_os("CON_GHOSTTY_VT_STEP") {
        if let Some(s) = val.to_str() {
            return vec![s.to_string()];
        }
    }

    let output = Command::new(zig_bin)
        .args(["build", "-h"])
        .current_dir(ghostty_dir)
        .output();

    let help_text = match output {
        Ok(out) => {
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&out.stdout));
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            text
        }
        Err(err) => {
            panic!("failed to run `zig build -h` in Ghostty checkout: {err}");
        }
    };

    // Preferred: current Ghostty exposes `-Demit-lib-vt=[bool]` which
    // configures the default `install` step to produce only
    // libghostty-vt (disables xcframework / macOS-app / docs).
    if help_text.contains("-Demit-lib-vt=") {
        return vec!["-Demit-lib-vt=true".to_string()];
    }

    // Fallback: older revisions that split the lib behind a named step.
    const CANDIDATES: &[&str] = &[
        "ghostty-vt-static",
        "libghostty-vt-static",
        "vt-static",
        "libghostty-vt",
        "ghostty-vt",
    ];
    for cand in CANDIDATES {
        if help_text.lines().any(|line| {
            let t = line.trim_start();
            t.starts_with(&format!("{cand} ")) || t == *cand
        }) {
            return vec![cand.to_string()];
        }
    }

    panic!(
        "\n========================================================\n\
         con-ghostty: couldn't find a libghostty-vt build knob in this\n\
         Ghostty checkout. The pinned revision may predate libghostty-vt\n\
         entirely (PR ghostty-org/ghostty#8840), or the upstream build\n\
         surface changed again.\n\n\
         `zig build -h` output:\n{help}\n\n\
         To work around: bump GHOSTTY_REV in crates/con-ghostty/build.rs,\n\
         or set CON_GHOSTTY_VT_STEP to an exact step name from the list\n\
         above.\n\
         ========================================================\n",
        help = help_text,
    );
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

fn try_find_lib(ghostty_dir: &PathBuf, lib_name: &str) -> Option<PathBuf> {
    let zig_cache = ghostty_dir.join(".zig-cache");
    if zig_cache.exists() {
        if let Ok(output) = Command::new("find")
            .args([zig_cache.to_str().unwrap(), "-name", lib_name, "-type", "f"])
            .output()
        {
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
                return Some(path.clone());
            }
        }
    }
    for relative in ["zig-out/lib", "zig-out\\lib", "macos/build/Debug"] {
        let candidate = ghostty_dir.join(relative).join(lib_name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
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
