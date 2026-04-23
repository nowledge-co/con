#!/bin/sh
# con — Unix terminal emulator installer (macOS + Linux).
# Usage: curl -fsSL https://con-releases.nowledge.co/install.sh | sh
#
# macOS path: download the signed DMG, mount it, copy the bundled
#   `con*.app` into /Applications, launch it. Same flow that's been
#   live since the macOS DMG release pipeline shipped.
# Linux path: download the channel tarball, extract `con` into
#   ~/.local/bin, drop a `.desktop` entry into ~/.local/share/applications
#   so it shows up in your launcher, and `chmod +x` the binary. The
#   binary is self-contained — no shared libs other than the GPUI Linux
#   runtime apt deps that come pre-installed on every modern desktop
#   distro.
#
# Both paths share the same one-liner UX: pretty banner, channel
# detection from the GitHub `releases/latest` tag, no sudo unless the
# install dir actually requires it. The Sparkle-shaped appcast feed
# at https://con-releases.nowledge.co/appcast/{channel}-{platform}-{arch}.xml
# is updated by the release CI for each platform; the in-app updater
# polls it and re-runs this script via `apply_update_in_place()` when
# the user clicks "Update" in Settings → Updates.

set -eu

REPO="nowledge-co/con-terminal"

# ── Colors ──────────────────────────────────────────────────────────────────

ESC=$(printf '\033')

if [ -t 1 ]; then
  R="${ESC}[0m"  B="${ESC}[1m"
  OK="${ESC}[38;2;0;210;160m"
  DIM="${ESC}[38;2;140;150;175m"
  ERR="${ESC}[38;2;230;57;70m"
else
  R=""  B=""  OK=""  DIM=""  ERR=""
fi

pass() { printf "   ${OK}✓${R}  %s\n" "$*"; }
fail() { printf "   ${ERR}✗${R}  %s\n" "$*" >&2; exit 1; }

# ── Banner ──────────────────────────────────────────────────────────────────
# Exact output from: npx oh-my-logo "con" --palette-colors "#4ea8ff,#a855f7,#ec4899" --filled --block-font tiny --color

print_banner() {
  printf "\n"
  if [ -t 1 ]; then
    printf '   \033[38;5;111m█▀\033[38;5;105m▀\033[38;5;141m █▀\033[38;5;177m█\033[38;5;176m █\033[38;5;170m▄\033[38;5;169m \033[38;5;205m█\033[0m\n'
    printf '   \033[38;5;111m█▄\033[38;5;105m▄\033[38;5;141m █▄\033[38;5;177m█\033[38;5;176m █\033[38;5;170m \033[38;5;169m▀\033[38;5;205m█\033[0m\n'
  else
    printf '   con\n'
  fi
  printf "\n"
}

print_banner

# ── Preflight ───────────────────────────────────────────────────────────────

uname_s="$(uname -s)"
case "$uname_s" in
  Darwin) os="macos" ;;
  Linux)  os="linux" ;;
  *)      fail "unsupported OS: $uname_s (con supports macOS and Linux via this script; Windows uses install.ps1)" ;;
esac

arch="$(uname -m)"
case "$arch" in
  arm64|aarch64) art_arch="arm64"  ;;
  x86_64|amd64)  art_arch="x86_64" ;;
  *)             fail "unsupported architecture: $arch" ;;
esac

# ── Resolve ─────────────────────────────────────────────────────────────────
#
# `CON_INSTALL_VERSION` lets the in-app `apply_update_in_place` path
# pin the installer to the exact version the appcast advertised.
# Without that pin, GitHub's `/releases/latest` silently skips
# prereleases — a beta-channel user clicking through to "Update
# now" would otherwise risk getting a stable downgrade. Treat
# `0.1.0-beta.32`, `v0.1.0-beta.32`, and a stray-whitespace mix of
# either as equivalent.

install_version="${CON_INSTALL_VERSION:-}"
install_version="$(printf '%s' "$install_version" | tr -d '[:space:]')"
install_version="${install_version#v}"

if [ -n "$install_version" ]; then
  release_api="https://api.github.com/repos/${REPO}/releases/tags/v${install_version}"
else
  release_api="https://api.github.com/repos/${REPO}/releases/latest"
fi

release_json="$(curl -fsSL "$release_api" 2>/dev/null)" \
  || fail "could not reach GitHub"

tag="$(printf '%s' "$release_json" | grep '"tag_name"' | head -1 | sed 's/.*: *"//;s/".*//')"
[ -n "$tag" ] || fail "could not determine release${install_version:+ for v${install_version}}"
version="${tag#v}"

channel=""
case "$version" in
  *-beta.*)  channel="Beta" ;;
  *-dev.*)   channel="Dev" ;;
esac

# Asset name pattern depends on the OS — the macOS pipeline emits
# `con-<version>-macos-<arch>.dmg`, the Linux pipeline emits
# `con-<version>-linux-<arch>.tar.gz`. Pull the matching enclosure URL
# straight out of the releases JSON so we don't have to guess the tag
# format here.
if [ "$os" = "macos" ]; then
  asset_pattern="macos-${art_arch}\\.dmg"
else
  asset_pattern="linux-${art_arch}\\.tar\\.gz"
fi

asset_url="$(printf '%s' "$release_json" \
  | grep '"browser_download_url"' \
  | grep "$asset_pattern" \
  | head -1 \
  | sed 's/.*: *"//;s/".*//')"

[ -n "$asset_url" ] || fail "no ${os} ${art_arch} artifact found in latest release ($tag)"

if [ -n "$channel" ]; then
  pass "${B}con ${channel}${R}  ${DIM}${version} · ${os} · ${art_arch}${R}"
else
  pass "${B}con${R}  ${DIM}${version} · ${os} · ${art_arch}${R}"
fi

# ── Download ────────────────────────────────────────────────────────────────

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

if [ "$os" = "macos" ]; then
  archive_path="${tmpdir}/con.dmg"
else
  archive_path="${tmpdir}/con.tar.gz"
fi

printf "   ${DIM}·${R}  downloading"
curl -fSL "$asset_url" -o "$archive_path" 2>/dev/null \
  || fail "download failed"
size="$(du -h "$archive_path" | cut -f1 | tr -d ' ')"
printf "\r\033[K"
pass "downloaded  ${DIM}${size}${R}"

# ── Install ─────────────────────────────────────────────────────────────────

if [ "$os" = "macos" ]; then
  install_dir="/Applications"
  printf "   ${DIM}·${R}  installing"

  mount_point="${tmpdir}/con-volume"
  mkdir -p "$mount_point"
  hdiutil attach -quiet -nobrowse -mountpoint "$mount_point" "$archive_path" \
    || fail "could not mount disk image"

  app_src=""
  for f in "$mount_point"/*.app; do
    [ -d "$f" ] && app_src="$f" && break
  done
  [ -n "$app_src" ] || {
    hdiutil detach -quiet "$mount_point" 2>/dev/null
    fail "no .app found in disk image"
  }

  app_name="$(basename "$app_src")"
  target="${install_dir}/${app_name}"

  if [ -d "$target" ]; then
    rm -rf "$target" 2>/dev/null \
      || sudo rm -rf "$target"
  fi

  cp -R "$app_src" "${install_dir}/" 2>/dev/null \
    || sudo cp -R "$app_src" "${install_dir}/"

  hdiutil detach -quiet "$mount_point" 2>/dev/null

  printf "\r\033[K"
  pass "installed  ${DIM}${install_dir}/${app_name}${R}"

  # Launch
  open_name="${app_name%.app}"
  printf "\n"
  if [ -t 1 ]; then
    printf '   \033[38;5;111m━━\033[38;5;105m━━\033[38;5;141m━━\033[38;5;177m━━\033[38;5;176m━━\033[38;5;170m━━\033[38;5;169m━━\033[38;5;205m━━\033[0m\n'
  else
    printf '   ────────────────\n'
  fi
  printf "\n"
  open -a "$open_name" 2>/dev/null && pass "launched — enjoy!" || true
  printf "\n"
  exit 0
fi

# ── Linux install ───────────────────────────────────────────────────────────
#
# Per-user install under ~/.local. Matches XDG conventions and avoids
# requiring sudo. The binary is self-contained; the only runtime
# dependencies are the GPUI Linux apt packages (libxcb-*, libxkbcommon,
# libwayland, libvulkan, libfreetype, libfontconfig) that ship on
# every modern desktop distro by default. We log the recommended apt
# install line at the end for users on minimal images.

extract_dir="${tmpdir}/extract"
mkdir -p "$extract_dir"

printf "   ${DIM}·${R}  extracting"
tar -xzf "$archive_path" -C "$extract_dir" 2>/dev/null \
  || fail "could not extract tarball"
printf "\r\033[K"
pass "extracted"

# The release tarball contains:
#   con-<version>-linux-<arch>/
#     con            (the binary)
#     LICENSE
#     README.md
#     con.desktop
#     con.png
staged_root=""
for d in "$extract_dir"/*; do
  [ -d "$d" ] && [ -f "$d/con" ] && staged_root="$d" && break
done
[ -n "$staged_root" ] || fail "tarball layout unexpected — no con/ binary found"

bin_dir="${HOME}/.local/bin"
share_dir="${HOME}/.local/share"
apps_dir="${share_dir}/applications"
icons_dir="${share_dir}/icons/hicolor/256x256/apps"

mkdir -p "$bin_dir" "$apps_dir" "$icons_dir"

printf "   ${DIM}·${R}  installing"

target_bin="${bin_dir}/con"
# If con is currently running, the kernel keeps the open exe inode
# alive even if we replace the file. Move-then-overwrite is the
# rsync-style pattern that survives self-update — `cp` would fail
# with `Text file busy` on some kernels.
if [ -f "$target_bin" ]; then
  rm -f "$target_bin" 2>/dev/null || true
fi
cp "$staged_root/con" "$target_bin"
chmod +x "$target_bin"

# Desktop entry — handles "con shows up in the launcher" and
# "double-clicking a `con://` URL". The tarball ships a templated
# `con.desktop` that points at /usr/local/bin; rewrite the Exec line
# to the resolved per-user binary path so it works regardless of
# whether ~/.local/bin is on the user's PATH.
if [ -f "$staged_root/con.desktop" ]; then
  sed "s|^Exec=.*|Exec=${target_bin} %U|" "$staged_root/con.desktop" \
    > "${apps_dir}/con.desktop"
  chmod 644 "${apps_dir}/con.desktop"
fi

if [ -f "$staged_root/con.png" ]; then
  cp "$staged_root/con.png" "${icons_dir}/con.png"
fi

# Refresh the desktop database so the new .desktop file is picked up
# by GNOME / KDE / xfce launchers without a logout. Best-effort —
# headless / minimal environments may not have these tools.
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -f -t -q "${share_dir}/icons/hicolor" >/dev/null 2>&1 || true
fi

printf "\r\033[K"
pass "installed  ${DIM}${target_bin}${R}"

# ── PATH check ──────────────────────────────────────────────────────────────

case ":${PATH:-}:" in
  *":${bin_dir}:"*) ;;
  *)
    printf "\n"
    pass "${DIM}note:${R}  ${B}~/.local/bin${R} ${DIM}is not on your PATH yet${R}"
    printf "          ${DIM}add this to your shell rc:${R}\n"
    printf "          ${DIM}export PATH=\"\$HOME/.local/bin:\$PATH\"${R}\n"
    ;;
esac

# ── Launch ──────────────────────────────────────────────────────────────────

printf "\n"
if [ -t 1 ]; then
  printf '   \033[38;5;111m━━\033[38;5;105m━━\033[38;5;141m━━\033[38;5;177m━━\033[38;5;176m━━\033[38;5;170m━━\033[38;5;169m━━\033[38;5;205m━━\033[0m\n'
else
  printf '   ────────────────\n'
fi
printf "\n"

# Don't auto-launch on Linux — the user might be on a headless box,
# in a CI runner, or piping the install through `ssh host -- sh -c
# "curl ... | sh"` from a desktop shell that has no DISPLAY of its
# own. Just tell them how to start it.
pass "run  ${B}con${R}  ${DIM}from any terminal — enjoy!${R}"
printf "\n"
