#!/bin/sh
# con — macOS terminal emulator installer
# Usage: curl -fsSL https://con-releases.nowledge.co/install.sh | sh
set -eu

# ── Config ──────────────────────────────────────────────────────────────────

REPO="nowledge-co/con"
APP_NAME="con.app"
INSTALL_DIR="/Applications"

# ── Helpers ─────────────────────────────────────────────────────────────────

dim="\033[2m"    bold="\033[1m"
cyan="\033[36m"  green="\033[32m"  red="\033[31m"  yellow="\033[33m"
reset="\033[0m"

say()  { printf "${cyan}${bold}con${reset} ${dim}·${reset} %s\n" "$*"; }
ok()   { printf "${green}${bold} ok${reset}  %s\n" "$*"; }
warn() { printf "${yellow}${bold}  !${reset}  %s\n" "$*"; }
die()  { printf "${red}${bold}err${reset}  %s\n" "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found"
}

# ── Preflight ───────────────────────────────────────────────────────────────

[ "$(uname -s)" = "Darwin" ] || die "con is macOS-only"
need curl
need hdiutil

arch="$(uname -m)"
case "$arch" in
  arm64)  dmg_arch="arm64"  ;;
  x86_64) dmg_arch="x86_64" ;;
  *)      die "Unsupported architecture: $arch" ;;
esac

# ── Resolve latest release ──────────────────────────────────────────────────

say "finding latest release..."

# Use GitHub API to get the latest release tag and DMG URL
release_json="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null)" \
  || die "could not reach GitHub — check your network"

tag="$(printf '%s' "$release_json" | grep '"tag_name"' | head -1 | sed 's/.*: *"//;s/".*//')"
[ -n "$tag" ] || die "could not determine latest release"

version="${tag#v}"

# Find the DMG asset URL for this architecture
dmg_url="$(printf '%s' "$release_json" \
  | grep '"browser_download_url"' \
  | grep "macos-${dmg_arch}\\.dmg" \
  | head -1 \
  | sed 's/.*: *"//;s/".*//')"

[ -n "$dmg_url" ] || die "no macOS ${dmg_arch} DMG found for ${tag}"

ok "con ${version} (${dmg_arch})"

# ── Check existing installation ─────────────────────────────────────────────

if [ -d "${INSTALL_DIR}/${APP_NAME}" ]; then
  existing="$(defaults read "${INSTALL_DIR}/${APP_NAME}/Contents/Info" CFBundleShortVersionString 2>/dev/null || echo "unknown")"
  warn "existing installation found (${existing}) — will be replaced"
fi

# ── Download ────────────────────────────────────────────────────────────────

say "downloading..."

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

dmg_path="${tmpdir}/con.dmg"
curl -fSL --progress-bar "$dmg_url" -o "$dmg_path" \
  || die "download failed"

ok "$(du -h "$dmg_path" | cut -f1 | tr -d ' ')"

# ── Verify signature ───────────────────────────────────────────────────────

say "verifying code signature..."

mount_point="${tmpdir}/con-volume"
mkdir -p "$mount_point"
hdiutil attach -quiet -nobrowse -mountpoint "$mount_point" "$dmg_path" \
  || die "could not mount DMG"

app_src="${mount_point}/${APP_NAME}"
[ -d "$app_src" ] || {
  hdiutil detach -quiet "$mount_point" 2>/dev/null
  die "DMG does not contain ${APP_NAME}"
}

codesign -v --deep --strict "$app_src" 2>/dev/null \
  && ok "valid Developer ID signature" \
  || warn "signature check failed — installing anyway"

# ── Install ─────────────────────────────────────────────────────────────────

say "installing to ${INSTALL_DIR}..."

if [ -d "${INSTALL_DIR}/${APP_NAME}" ]; then
  rm -rf "${INSTALL_DIR}/${APP_NAME}" 2>/dev/null \
    || { warn "need permission to write to ${INSTALL_DIR}"; sudo rm -rf "${INSTALL_DIR}/${APP_NAME}"; }
fi

cp -R "$app_src" "${INSTALL_DIR}/" 2>/dev/null \
  || sudo cp -R "$app_src" "${INSTALL_DIR}/"

hdiutil detach -quiet "$mount_point" 2>/dev/null

ok "installed to ${INSTALL_DIR}/${APP_NAME}"

# ── Done ────────────────────────────────────────────────────────────────────

printf "\n"
printf "  ${bold}con ${version}${reset} ${dim}is ready${reset}\n"
printf "  ${dim}open ${INSTALL_DIR}/${APP_NAME} or run:${reset}\n"
printf "\n"
printf "    open -a con\n"
printf "\n"
