#!/bin/sh
# con — macOS terminal emulator installer
# Usage: curl -fsSL https://con-releases.nowledge.co/install.sh | sh
set -eu

REPO="nowledge-co/con"
INSTALL_DIR="/Applications"

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

printf "\n"
if [ -t 1 ]; then
  printf '   \033[38;5;111m█▀\033[38;5;105m▀\033[38;5;141m █▀\033[38;5;177m█\033[38;5;176m █\033[38;5;170m▄\033[38;5;169m \033[38;5;205m█\033[0m\n'
  printf '   \033[38;5;111m█▄\033[38;5;105m▄\033[38;5;141m █▄\033[38;5;177m█\033[38;5;176m █\033[38;5;170m \033[38;5;169m▀\033[38;5;205m█\033[0m\n'
else
  printf '   con\n'
fi
printf "\n"

# ── Preflight ───────────────────────────────────────────────────────────────

[ "$(uname -s)" = "Darwin" ] || fail "con requires macOS"

arch="$(uname -m)"
case "$arch" in
  arm64)  dmg_arch="arm64"  ;;
  x86_64) dmg_arch="x86_64" ;;
  *)      fail "unsupported architecture: $arch" ;;
esac

# ── Resolve ─────────────────────────────────────────────────────────────────

release_json="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null)" \
  || fail "could not reach GitHub"

tag="$(printf '%s' "$release_json" | grep '"tag_name"' | head -1 | sed 's/.*: *"//;s/".*//')"
[ -n "$tag" ] || fail "could not determine latest release"
version="${tag#v}"

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

[ -n "$dmg_url" ] || fail "no DMG found for ${dmg_arch}"

if [ -n "$channel" ]; then
  pass "${B}con ${channel}${R}  ${DIM}${version} · ${dmg_arch}${R}"
else
  pass "${B}con${R}  ${DIM}${version} · ${dmg_arch}${R}"
fi

# ── Download ────────────────────────────────────────────────────────────────

tmpdir="$(mktemp -d)"
trap 'rm -rf "$tmpdir"' EXIT
dmg_path="${tmpdir}/con.dmg"

printf "   ${DIM}·${R}  downloading"
curl -fSL "$dmg_url" -o "$dmg_path" 2>/dev/null \
  || fail "download failed"
size="$(du -h "$dmg_path" | cut -f1 | tr -d ' ')"
printf "\r\033[K"
pass "downloaded  ${DIM}${size}${R}"

# ── Install ─────────────────────────────────────────────────────────────────

printf "   ${DIM}·${R}  installing"

mount_point="${tmpdir}/con-volume"
mkdir -p "$mount_point"
hdiutil attach -quiet -nobrowse -mountpoint "$mount_point" "$dmg_path" \
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
target="${INSTALL_DIR}/${app_name}"

if [ -d "$target" ]; then
  rm -rf "$target" 2>/dev/null \
    || sudo rm -rf "$target"
fi

cp -R "$app_src" "${INSTALL_DIR}/" 2>/dev/null \
  || sudo cp -R "$app_src" "${INSTALL_DIR}/"

hdiutil detach -quiet "$mount_point" 2>/dev/null

printf "\r\033[K"
pass "installed  ${DIM}${INSTALL_DIR}/${app_name}${R}"

# ── Launch ──────────────────────────────────────────────────────────────────

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
