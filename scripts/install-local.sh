#!/usr/bin/env bash
# Builds this checkout as a release Patchbay.app and puts it in /Applications (or the folder given), replacing any
# copy there. It's signed for this Mac only, so it isn't one to hand to anyone: releases are `just ship`.
# Usage: scripts/install-local.sh [folder]
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
dir=${1:-/Applications}
target="$dir/Patchbay.app"
built=$(dirname "$(dirname "$(dirname "$("$root/scripts/bundle-macos.sh" release)")")")

# Copy beside the target and swap, so a failed copy leaves the old app in place.
next=$(mktemp -d "$dir/.patchbay.XXXXXX")
trap 'rm -rf "$next"' EXIT
ditto "$built" "$next/Patchbay.app"
rm -rf "$target"
mv "$next/Patchbay.app" "$target"

branch=$(git -C "$root" rev-parse --abbrev-ref HEAD 2>/dev/null || echo "no branch")
commit=$(git -C "$root" rev-parse --short HEAD 2>/dev/null || echo "no commit")
dirty=""
[ -z "$(git -C "$root" status --porcelain 2>/dev/null)" ] || dirty=", with uncommitted changes"
echo "Installed $branch at $commit$dirty to $target"
if pgrep -xq "Patchbay"; then
  echo "The old version is still running. Quit it and open Patchbay again."
fi
