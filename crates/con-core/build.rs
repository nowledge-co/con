fn main() {
    // `option_env!("CON_RELEASE_CHANNEL")` in release_channel.rs bakes
    // the channel into the binary, but Cargo does not track env-var
    // reads inside macros — so a cached build with the wrong channel
    // would persist until something else invalidated it. Declaring
    // the dependency here forces a rebuild on change.
    println!("cargo:rerun-if-env-changed=CON_RELEASE_CHANNEL");
}
