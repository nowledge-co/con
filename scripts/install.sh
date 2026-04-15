#!/bin/sh
# con — macOS terminal emulator installer
# Usage: curl -fsSL https://con-releases.nowledge.co/install.sh | sh
set -eu

# ── Config ──────────────────────────────────────────────────────────────────

REPO="nowledge-co/con"
INSTALL_DIR="/Applications"

# ── Colors ──────────────────────────────────────────────────────────────────

if [ -t 1 ]; then
  R="\033[0m"  B="\033[1m"  D="\033[2m"  I="\033[3m"
  RED="\033[31m"  GRN="\033[32m"  YLW="\033[33m"  CYN="\033[36m"  MGN="\033[35m"
else
  R=""  B=""  D=""  I=""
  RED=""  GRN=""  YLW=""  CYN=""  MGN=""
fi

# ── Helpers ─────────────────────────────────────────────────────────────────

info()  { printf "  ${CYN}>${R}  %s\n" "$*"; }
ok()    { printf "  ${GRN}>${R}  %s\n" "$*"; }
warn()  { printf "  ${YLW}>${R}  %s\n" "$*"; }
die()   { printf "  ${RED}>${R}  %s\n" "$*" >&2; exit 1; }

need() {
  command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not found"
}

# ── Banner ──────────────────────────────────────────────────────────────────

printf "\n"
printf "  ${B}${CYN}               ${R}\n"
printf "  ${B}${CYN}   ┌──────┐   ${R}\n"
printf "  ${B}${CYN}   │ ▓▓▓▓ │   ${R}  ${B}con${R} ${D}installer${R}\n"
printf "  ${B}${CYN}   │ ▓▓▓▓ │   ${R}  ${D}GPU-accelerated terminal + AI agent${R}\n"
printf "  ${B}${CYN}   │ ░░░░ │   ${R}\n"
printf "  ${B}${CYN}   └──────┘   ${R}\n"
printf "\n"

# ── Preflight ───────────────────────────────────────────────────────────────

[ "$(uname -s)" = "Darwin" ] || die "con requires macOS"
need curl
need hdiutil

arch="$(uname -m)"
case "$arch" in
  arm64)  dmg_arch="arm64"  ;;
  x86_64) dmg_arch="x86_64" ;;
  *)      die "unsupported architecture: $arch" ;;
esac

# ── Resolve latest release ──────────────────────────────────────────────────

info "fetching latest release..."

release_json="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null)" \
  || die "could not reach GitHub — check your connection"

tag="$(printf '%s' "$release_json" | grep '"tag_name"' | head -1 | sed 's/.*: *"//;s/".*//')"
[ -n "$tag" ] || die "could not determine latest release"
version="${tag#v}"

# Determine channel from version string
channel=""
case "$version" in
  *-beta.*)  channel="Beta" ;;
  *-dev.*)   channel="Dev" ;;
esac

dmg_url="$(printf '%s' "$release_json" \
  | grep '"browser_download_url"' \
  | grep "macos-${dmg_arch}\\.dmg" \
  | head -1 \
  | sed 's/.*: *"//;s/".*//')"

[ -n "$dmg_url" ] || die "no macOS ${dmg_arch} DMG found for ${tag}"

if [ -n "$channel" ]; then
  ok "${B}con ${channel}${R} ${D}${version}${R} ${D}(${dmg_arch})${R}"
else
  ok "${B}con${R} ${D}${version}${R} ${D}(${dmg_arch})${R}"
fi

# ── Download ────────────────────────────────────────────────────────────────

info "downloading..."

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT

dmg_path="${tmpdir}/con.dmg"
curl -fSL --progress-bar "$dmg_url" -o "$dmg_path" \
  || die "download failed"

size="$(du -h "$dmg_path" | cut -f1 | tr -d ' ')"
ok "downloaded ${D}(${size})${R}"

# ── Mount and discover app ──────────────────────────────────────────────────

info "verifying..."

mount_point="${tmpdir}/con-volume"
mkdir -p "$mount_point"
hdiutil attach -quiet -nobrowse -mountpoint "$mount_point" "$dmg_path" \
  || die "could not mount disk image"

# Find the .app bundle inside the DMG (handles "con.app", "con Beta.app", etc.)
app_src=""
for f in "$mount_point"/*.app; do
  [ -d "$f" ] && app_src="$f" && break
done
[ -n "$app_src" ] || {
  hdiutil detach -quiet "$mount_point" 2>/dev/null
  die "no .app found in disk image"
}

app_name="$(basename "$app_src")"

codesign -v --deep --strict "$app_src" 2>/dev/null \
  && ok "code signature ${D}valid${R}" \
  || warn "signature verification failed — installing anyway"

# ── Install ─────────────────────────────────────────────────────────────────

target="${INSTALL_DIR}/${app_name}"

if [ -d "$target" ]; then
  existing="$(defaults read "${target}/Contents/Info" CFBundleShortVersionString 2>/dev/null || echo "?")"
  info "replacing ${D}${app_name} (${existing})${R}"
  rm -rf "$target" 2>/dev/null \
    || sudo rm -rf "$target"
fi

cp -R "$app_src" "${INSTALL_DIR}/" 2>/dev/null \
  || sudo cp -R "$app_src" "${INSTALL_DIR}/"

hdiutil detach -quiet "$mount_point" 2>/dev/null

# ── Success ─────────────────────────────────────────────────────────────────

# Derive the open -a name (strip .app)
open_name="${app_name%.app}"

printf "\n"
if [ -n "$channel" ]; then
  label="con ${channel} ${version}"
else
  label="con ${version}"
fi

# Dynamic-width success box
label_len="$(printf '%s' "$label installed" | wc -c | tr -d ' ')"
pad_len=$((label_len + 4))
border=""
i=0
while [ "$i" -lt "$pad_len" ]; do
  border="${border}─"
  i=$((i + 1))
done

printf "  ${GRN}${B}┌${border}┐${R}\n"
printf "  ${GRN}${B}│${R}  ${GRN}${B}%s${R} installed  ${GRN}${B}│${R}\n" "$label"
printf "  ${GRN}${B}└${border}┘${R}\n"
printf "\n"
printf "  ${D}Launch from Spotlight or run:${R}\n"
printf "\n"
printf "    ${B}open -a \"${open_name}\"${R}\n"
printf "\n"
