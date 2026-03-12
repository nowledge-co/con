# Implementation: Build System

## Overview

con's build has two stages: compile libghostty-vt (Zig), then compile everything else (Rust/Cargo).

## Zig → Rust Integration

### con-terminal/build.rs

```rust
// Pseudocode for build.rs
fn main() {
    let ghostty_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../3pp/ghostty");

    // 1. Check zig version
    let zig_version = Command::new("zig").arg("version").output();
    assert!(zig_version >= "0.15.2", "Zig 0.15.2+ required");

    // 2. Build libghostty-vt
    let status = Command::new("zig")
        .args(["build", "lib-vt"])
        .current_dir(&ghostty_dir)
        .status();

    // 3. Tell cargo where to find the library
    println!("cargo:rustc-link-search={}/zig-out/lib", ghostty_dir);
    println!("cargo:rustc-link-lib=static=ghostty-vt");

    // 4. Generate Rust bindings
    let bindings = bindgen::Builder::default()
        .header(ghostty_dir.join("include/ghostty/vt.h"))
        .generate();
    bindings.write_to_file(out_dir.join("ghostty_vt.rs"));

    // 5. Rebuild if ghostty source changes
    println!("cargo:rerun-if-changed=../3pp/ghostty/src/");
}
```

### CI Caching

```yaml
# .github/workflows/build.yml
- uses: actions/cache@v4
  with:
    path: 3pp/ghostty/zig-out
    key: ghostty-vt-${{ runner.os }}-${{ hashFiles('3pp/ghostty/src/**') }}
```

## Cross-Compilation

Zig excels at cross-compilation. From macOS:

```bash
# Build for Linux
zig build lib-vt -Dtarget=x86_64-linux-gnu

# Build for Windows
zig build lib-vt -Dtarget=x86_64-windows-msvc
```

Cargo cross-compilation needs `cross` or manual sysroot setup. The Zig part is the easy half.

## Workspace Cargo.toml

```toml
[workspace]
members = [
    "crates/con",
    "crates/con-core",
    "crates/con-terminal",
    "crates/con-agent",
    "crates/con-cli",
]
resolver = "2"

[workspace.dependencies]
gpui = { path = "3pp/gpui-ce", package = "gpui-ce" }
rig-core = "0.10"            # pin to stable
portable-pty = "0.8"
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["full"] }
anyhow = "1"
```

## Dev Dependencies

```bash
# Install dev tools
cargo install cargo-watch    # auto-rebuild
cargo install cargo-nextest  # faster test runner
```

```bash
# Dev loop
cargo watch -x 'run -p con'
```
