fn main() {
    // Windows users hit `cargo build -p con --release` out of muscle
    // memory and get a confusing cargo warning: "binary target `con` is
    // a reserved Windows filename, this target will not work on Windows
    // platforms". That's because the default feature set targets
    // macOS/Linux (`bin-con`). Detect the bad combination and fail
    // early with a pointer at the `cargo wbuild` alias.
    //
    // Default macOS/Linux path: no feature gating — `bin-con` is the
    // default; the `con` bin target builds normally.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let bin_con = std::env::var_os("CARGO_FEATURE_BIN_CON").is_some();
    let bin_con_app = std::env::var_os("CARGO_FEATURE_BIN_CON_APP").is_some();

    if target_os == "windows" && bin_con && !bin_con_app {
        panic!(
            "\n\n========================================================\n\
             con: `cargo build` with the default feature set targets the\n\
             `con` binary, but `CON` is a reserved DOS device name on\n\
             Windows — `con.exe` cannot be created.\n\n\
             Use the `cargo wbuild` / `cargo wcheck` / `cargo wrun` /\n\
             `cargo wtest` aliases instead — they select the `con-app`\n\
             binary via `--no-default-features --features con/bin-con-app`.\n\
             Aliases are declared in the workspace `.cargo/config.toml`.\n\n\
             See `docs/impl/windows-port.md` and `CLAUDE.md` for details.\n\
             ========================================================\n"
        );
    }

    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/objc/sparkle_trampoline.m")
            .file("src/objc/global_hotkey_trampoline.m")
            .flag("-fobjc-arc")
            .flag("-fmodules")
            .compile("con_objc_trampolines");

        println!("cargo:rerun-if-changed=src/objc/sparkle_trampoline.m");
        println!("cargo:rerun-if-changed=src/objc/global_hotkey_trampoline.m");
        println!("cargo:rustc-link-lib=framework=Carbon");
    }
}
