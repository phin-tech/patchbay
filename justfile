set shell := ["bash", "-euo", "pipefail", "-c"]

default:
    @just --list

# Run the app from a bundle, which is what gives it its Dock icon
[positional-arguments]
run *args:
    "$(scripts/bundle-macos.sh debug)" "$@"

# Run the app in a forced appearance: `just run-as dark`
run-as appearance:
    PATCHBAY_APPEARANCE={{appearance}} "$(scripts/bundle-macos.sh debug)"

# Run the app with the element inspector (⌥⌘I): layout, styles and the source of whatever you pick
inspect:
    CARGO_FEATURES=devtools "$(scripts/bundle-macos.sh debug)"

# Run all tests, with cargo-nextest if it's installed (each test in a process of its own, and faster)
test:
    if command -v cargo-nextest >/dev/null; then cargo nextest run --workspace; else cargo test --workspace; fi

# Accept or reject changed snapshots (`insta::assert_snapshot!`), with cargo-insta
review:
    cargo insta review

# Licences, advisories and sources of every dependency (cargo-deny), and spelling (typos), each if it's installed
lint:
    if command -v cargo-deny >/dev/null; then cargo deny check; else echo "cargo-deny isn't installed: brew install cargo-deny"; fi
    if command -v typos >/dev/null; then typos; else echo "typos isn't installed: brew install typos-cli"; fi

# Save a rendered frame to target/shots/<name>.png, without showing a window: `PATCHBAY_KEYS="space space" just shot`.
# The settings are left alone; `PATCHBAY_SETTINGS=<file>` tries some, `PATCHBAY_WIDTH=600` a narrower window
shot name="app" appearance="light":
    @mkdir -p "${CARGO_TARGET_DIR:-target}/shots"
    PATCHBAY_APPEARANCE={{appearance}} PATCHBAY_SHOT="$(cd "${CARGO_TARGET_DIR:-target}/shots" && pwd)/{{name}}.png" \
        cargo run -p patchbay --features screenshot

# Build Patchbay.app and put it in /Applications, signed only for this Mac
install-local:
    scripts/install-local.sh

# Remove the copy in /Applications
uninstall-local:
    rm -rf "/Applications/Patchbay.app"

# Record a Time Profiler trace of a release build with symbols, to target/profile.trace, and open it in Instruments
profile:
    rm -rf "${CARGO_TARGET_DIR:-target}/profile.trace"
    xcrun xctrace record --template 'Time Profiler' --output "${CARGO_TARGET_DIR:-target}/profile.trace" \
        --launch -- "$(scripts/bundle-macos.sh profiling)"
    open "${CARGO_TARGET_DIR:-target}/profile.trace"

# Rebuild packaging/AppIcon.icns from packaging/logo.png
icon:
    uv run --quiet --with pillow packaging/make_icon.py

fmt:
    cargo fmt --all

# Formatting, clippy (with the screenshot feature too, which nothing else compiles) and every test
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo clippy -p patchbay --all-targets --features screenshot -- -D warnings
    cargo test --workspace

# Run the CI in .ci.star on this machine, against what's committed (needs starci)
ci:
    starci run . --bare

# Draw the pictures for the app's page on apps.phin.tech into docs/media (scripts/media.sh)
media:
    scripts/media.sh

# Remove build output. The installed copy stays
clean:
    cargo clean

# Bump, commit and tag v<version> and push, for GitHub to release it (.github/workflows/release.yml; ship: on this Mac)
[positional-arguments]
cut part *args:
    test "$(git branch --show-current)" = main || { echo "cut from main"; exit 1; }
    git fetch --quiet origin main && test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" || { echo "main is not origin/main; push or pull first"; exit 1; }
    {{release_tool}} bump "$@"
    version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1) && \
      git add -A && git commit --quiet -m "Patchbay $version" && git tag "v$version" && \
      git push --quiet --atomic origin main "v$version" && \
      echo "Pushed v$version; the build: https://github.com/phin-tech/patchbay/actions/workflows/release.yml"

# The release recipes: app-release, in release.just, which gpui-template keeps up to date
import 'release.just'
