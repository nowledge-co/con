#!/usr/bin/env bash
#
# Initialize the gh-pages branch for hosting Sparkle appcasts.
#
# Run once to create the branch structure:
#   ./scripts/sparkle/init-gh-pages.sh
#
# After running, configure GitHub Pages:
#   Repo Settings → Pages → Source: "Deploy from a branch" → Branch: gh-pages → / (root)
#
# If using a custom domain (con-releases.nowledge.co):
#   Add a CNAME record pointing to <org>.github.io
#   The CNAME file below handles the GitHub Pages side.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$REPO_ROOT"

if git rev-parse --verify gh-pages >/dev/null 2>&1; then
  echo "gh-pages branch already exists — nothing to do."
  exit 0
fi

# Refuse to run with uncommitted changes — the orphan checkout would lose them.
if ! git diff-index --quiet HEAD -- 2>/dev/null; then
  echo "ERROR: You have uncommitted changes.  Commit or stash them first." >&2
  exit 1
fi

original_branch="$(git rev-parse --abbrev-ref HEAD)"

# Create an orphan branch (no history from main)
git checkout --orphan gh-pages

# Remove all tracked files from the index (not from disk — orphan branch is empty)
git rm -rf . >/dev/null 2>&1 || true

# Custom domain for GitHub Pages
echo "con-releases.nowledge.co" > CNAME

# Create appcast directory
mkdir -p appcast

# Jekyll bypass (GitHub Pages)
touch .nojekyll

git add CNAME appcast/ .nojekyll
git commit -m "Initialize GitHub Pages for appcast hosting"

# Switch back to the original branch immediately
git checkout "$original_branch"

echo ""
echo "gh-pages branch created and you are back on '$original_branch'."
echo ""
echo "Push it:"
echo "  git push -u origin gh-pages"
echo ""
echo "Then configure:"
echo "  1. GitHub repo Settings → Pages → Source: gh-pages branch"
echo "  2. DNS CNAME: con-releases.nowledge.co → nowledge-co.github.io"
