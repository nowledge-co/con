## What happened

Con's macOS runtime was embedding libghostty successfully, but it was not consistently providing Ghostty's bundled runtime resources.

That had two different failure modes:

- `cargo run -p con` debug sessions launched without a discoverable Ghostty resources directory.
- Con app bundles copied only the Con binary and icon assets, not Ghostty's `share/ghostty` payload.

In that state, Ghostty does not present itself to child processes the same way standalone Ghostty does.

## Root cause

Ghostty's subprocess launcher depends on its resources directory to set up:

- `TERM=xterm-ghostty`
- `TERMINFO`
- `GHOSTTY_RESOURCES_DIR`
- shell integration support files

When the resources directory is missing, Ghostty falls back to `TERM=xterm-256color` and disables parts of shell integration.

Con was treating libghostty as if the static library alone were the full runtime, which was wrong.

## Fix applied

- `con-ghostty/build.rs` now exposes the built Ghostty resources path to Rust code as `CON_GHOSTTY_RESOURCES_DIR`.
- `con-ghostty` now seeds `GHOSTTY_RESOURCES_DIR` during startup when that built resources directory exists locally, which fixes `cargo run -p con` debug sessions.
- `scripts/macos/build-app.sh` now copies Ghostty's built `share/ghostty` tree into `Contents/Resources/ghostty` inside the Con app bundle.

## What we learned

- With embedded Ghostty, "linking libghostty" is not enough. The bundled Ghostty resources are part of the runtime contract.
- Product comparisons against standalone Ghostty are not valid unless child processes see the same terminal identity and shell integration environment.
- Missing runtime assets can look like rendering or TUI-behavior bugs even when the deeper issue is environment drift.
