fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new()
            .file("src/objc/sparkle_trampoline.m")
            .flag("-fobjc-arc")
            .flag("-fmodules")
            .compile("sparkle_trampoline");

        println!("cargo:rerun-if-changed=src/objc/sparkle_trampoline.m");
    }
}
