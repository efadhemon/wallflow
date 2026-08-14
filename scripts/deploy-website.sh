#!/usr/bin/env bash
# Deploy the assembled website/ folder (including installers in downloads/)
# to Vercel production.
#
# Required env: VERCEL_TOKEN, VERCEL_ORG_ID, VERCEL_PROJECT_ID
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

: "${VERCEL_TOKEN:?VERCEL_TOKEN is required}"
: "${VERCEL_ORG_ID:?VERCEL_ORG_ID is required}"
: "${VERCEL_PROJECT_ID:?VERCEL_PROJECT_ID is required}"

node scripts/sync-version.mjs --website

version="$(node -p "require('./package.json').version")"
downloads="$root/website/downloads"
required=(
  "Wallflow_${version}_amd64.deb"
  "Wallflow_${version}_amd64.AppImage"
  "Wallflow_${version}_aarch64.dmg"
  "Wallflow_${version}_x64.dmg"
  "Wallflow_${version}_x64-setup.exe"
)

if [[ ! -d "$downloads" ]]; then
  echo "Missing $downloads" >&2
  exit 1
fi

for name in "${required[@]}"; do
  if [[ ! -f "$downloads/$name" ]]; then
    echo "Missing installer $name in website/downloads" >&2
    ls -la "$downloads" >&2 || true
    exit 1
  fi
done

# Copy out of the git worktree so the repo-root .gitignore cannot exclude
# website/downloads when the Vercel CLI decides which files to upload.
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cp -a "$root/website/." "$stage/"

npx --yes vercel@56 deploy "$stage" --prod --yes --token "$VERCEL_TOKEN"
