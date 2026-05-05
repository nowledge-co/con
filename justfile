# Auto-detect arch: arm64 on Apple Silicon / aarch64 Linux, x86_64 elsewhere
arch := `uname -m | sed 's/aarch64/arm64/'`

# Release channel for macOS/Linux app bundles (stable | beta | dev)
channel := "stable"

# ── list ──────────────────────────────────────────────────────────────────────

# List all recipes (default)
default:
    @just --list

# ── universal dev commands ────────────────────────────────────────────────────

# Debug build (current platform)
build:
    cargo build -p con

# Release build (current platform)
build-release:
    cargo build --release -p con

# Run from source (current platform)
run:
    cargo run -p con

# Run all workspace tests (current platform)
test:
    cargo test --workspace

# Check without building
check:
    cargo check --workspace

# Run clippy on the workspace
lint:
    cargo clippy --workspace -- -D warnings

# Clean cargo build artifacts
clean:
    cargo clean

# Print the current workspace version
version:
    @grep -A3 '^\[workspace\.package\]' Cargo.toml | awk -F'"' '/^version/{print $2}'

# ── macOS ─────────────────────────────────────────────────────────────────────

# [macOS] Build a local .app bundle — no signing, no notarization
# Output: dist/macos/{channel}/{arch}/con.app
macos-bundle channel=channel arch=arch:
    CON_CHANNEL={{channel}} CON_ARCH={{arch}} ./scripts/macos/build-app.sh

# [macOS] Build .app and copy to /Applications (replaces existing)
macos-install channel=channel arch=arch: (macos-bundle channel arch)
    #!/usr/bin/env bash
    set -euo pipefail
    app_name="con"
    if [[ "{{channel}}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{channel}}" == "dev" ]];  then app_name="con Dev";  fi
    src="dist/macos/{{channel}}/{{arch}}/${app_name}.app"
    dst="/Applications/${app_name}.app"
    echo "Installing ${src} → ${dst}"
    rm -rf "${dst}"
    cp -R "${src}" "${dst}"
    echo "Done. Launch ${app_name} from /Applications or Spotlight."

# [macOS] Ad-hoc signed bundle (no Apple Developer account needed; Gatekeeper will warn once)
macos-bundle-adhoc channel=channel arch=arch: (macos-bundle channel arch)
    #!/usr/bin/env bash
    set -euo pipefail
    app_name="con"
    if [[ "{{channel}}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{channel}}" == "dev" ]];  then app_name="con Dev";  fi
    bundle="dist/macos/{{channel}}/{{arch}}/${app_name}.app"
    echo "Ad-hoc signing ${bundle}"
    codesign --force --deep --sign - "${bundle}"
    echo "Signed (ad-hoc): ${bundle}"

# [macOS] Install ad-hoc signed bundle to /Applications
macos-install-adhoc channel=channel arch=arch: (macos-bundle-adhoc channel arch)
    #!/usr/bin/env bash
    set -euo pipefail
    app_name="con"
    if [[ "{{channel}}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{channel}}" == "dev" ]];  then app_name="con Dev";  fi
    src="dist/macos/{{channel}}/{{arch}}/${app_name}.app"
    dst="/Applications/${app_name}.app"
    echo "Installing ${src} → ${dst}"
    rm -rf "${dst}"
    cp -R "${src}" "${dst}"
    echo "Done. Launch ${app_name} from /Applications or Spotlight."

# [macOS] Full release: build + sign + notarize + DMG
# Requires: APPLE_SIGNING_IDENTITY + APPLE_NOTARY_* or APPLE_ID env vars
macos-release channel=channel arch=arch:
    CON_CHANNEL={{channel}} CON_ARCH={{arch}} ./scripts/macos/release.sh

# [macOS] Download Sparkle.framework into .sparkle/ (enables auto-update in bundle)
macos-sparkle-download:
    ./scripts/sparkle/download.sh

# [macOS] Open the built app bundle in Finder
macos-open channel=channel arch=arch:
    #!/usr/bin/env bash
    app_name="con"
    if [[ "{{channel}}" == "beta" ]]; then app_name="con Beta"; fi
    if [[ "{{channel}}" == "dev" ]];  then app_name="con Dev";  fi
    open "dist/macos/{{channel}}/{{arch}}/${app_name}.app"

# ── Linux ─────────────────────────────────────────────────────────────────────

# [Linux] Build a release binary and package it
# Output: dist/con-{version}-linux-{arch}.tar.gz
linux-release arch=arch:
    CON_LINUX_ARCH={{arch}} ./scripts/linux/release.sh

# [Linux] Install the release binary to ~/.local/bin
linux-install arch=arch: (linux-release arch)
    #!/usr/bin/env bash
    set -euo pipefail
    # scripts/linux/release.sh stages to dist/con-{version}-linux-{arch}/
    # Resolve the versioned staging dir by globbing.
    stage_dir="$(ls -d dist/con-*-linux-{{arch}} 2>/dev/null | sort -V | tail -1)"
    if [[ -z "${stage_dir}" || ! -f "${stage_dir}/con" ]]; then
        echo "Binary not found under dist/con-*-linux-{{arch}}/ — run 'just linux-release' first"
        exit 1
    fi
    mkdir -p "$HOME/.local/bin"
    cp "${stage_dir}/con" "$HOME/.local/bin/con"
    chmod 755 "$HOME/.local/bin/con"
    echo "Installed ${stage_dir}/con → $HOME/.local/bin/con"

# ── Windows (run from Developer Command Prompt for VS 2022) ───────────────────

# [Windows] Debug build (con-app.exe — CON is a reserved DOS device name)
windows-build:
    cargo wbuild -p con

# [Windows] Release build
windows-build-release:
    cargo wbuild -p con --release

# [Windows] Run
windows-run:
    cargo wrun -p con

# [Windows] Test
windows-test:
    cargo wtest -p con-core -p con-cli -p con-agent -p con-terminal

# ── dist cleanup ──────────────────────────────────────────────────────────────

# Remove all dist/ output
clean-dist:
    rm -rf dist/
