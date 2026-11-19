#!/usr/bin/env bash
# Builds Patchbay.app, which carries the Dock icon and the bundle identifier, and prints the path of its binary.
# app-release builds its own for a release, from the same packaging/Info.plist and entitlements.
# Usage: scripts/bundle-macos.sh [debug|release|profiling], with CARGO_FEATURES=devtools for the inspector.
set -euo pipefail

profile=${1:-release}
root=$(cd "$(dirname "$0")/.." && pwd)
target=${CARGO_TARGET_DIR:-$root/target}

flag=()
[ "$profile" = debug ] || flag=(--profile "$profile")
if [ -n "${CARGO_FEATURES:-}" ]; then flag+=(--features "$CARGO_FEATURES"); fi
(cd "$root" && cargo build -p patchbay ${flag[@]+"${flag[@]}"}) >&2

version=$(cd "$root" && cargo pkgid -p patchbay | sed -E 's/.*(#|@)//')
build=$(sed -n 's/^build = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
app="$target/$profile/Patchbay.app"
plist="$app/Contents/Info.plist"

mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$target/$profile/patchbay" "$app/Contents/MacOS/Patchbay"
cp "$root/packaging/AppIcon.icns" "$app/Contents/Resources/"
cp "$root/packaging/Info.plist" "$plist"
set_string() { plutil -replace "$1" -string "$2" "$plist"; }
set_string CFBundleDevelopmentRegion en
set_string CFBundleInfoDictionaryVersion 6.0
set_string CFBundleName "Patchbay"
set_string CFBundleDisplayName "Patchbay"
set_string CFBundleExecutable "Patchbay"
set_string CFBundleIdentifier "tech.phin.patchbay"
set_string CFBundleIconFile AppIcon
set_string CFBundlePackageType APPL
set_string CFBundleShortVersionString "$version"
set_string CFBundleVersion "$build"
codesign --force --sign - --entitlements "$root/packaging/app.entitlements" "$app" >&2

echo "$app/Contents/MacOS/Patchbay"
