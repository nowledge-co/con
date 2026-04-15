fn main() {
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
