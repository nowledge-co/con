use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let ghostty_dir = manifest_dir.join("../../3pp/ghostty");
    let ghostty_dir = ghostty_dir.canonicalize().expect("3pp/ghostty not found");

    // Build libghostty via zig build
    let status = Command::new("zig")
        .args(["build", "-Dapp-runtime=none", "-Dxcframework-target=native"])
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
    println!("cargo:rustc-link-lib=static=ghostty");

    // macOS frameworks required by libghostty
    if cfg!(target_os = "macos") {
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
    }

    // Rerun if ghostty source changes
    println!("cargo:rerun-if-changed=../../3pp/ghostty/src");
    println!("cargo:rerun-if-changed=../../3pp/ghostty/include/ghostty.h");

    // Generate include path for FFI
    let include_dir = ghostty_dir.join("include");
    println!(
        "cargo:include={}",
        include_dir.display()
    );
}

fn find_libghostty(ghostty_dir: &PathBuf) -> PathBuf {
    // Check zig-cache for the freshly built library
    let zig_cache = ghostty_dir.join(".zig-cache");
    if zig_cache.exists() {
        // Walk the cache looking for the most recent libghostty.a
        let output = Command::new("find")
            .args([
                zig_cache.to_str().unwrap(),
                "-name",
                "libghostty.a",
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
    let fallback = ghostty_dir.join("macos/build/Debug/libghostty.a");
    if fallback.exists() {
        return fallback;
    }

    panic!("Could not find libghostty.a — run: cd 3pp/ghostty && zig build -Dapp-runtime=none");
}
