# Release tooling: app-release (https://git.phin.tech/sam/app-release), pinned in scripts/release and run with uv;
# release.toml says what it needs to know. Each recipe takes --dry-run, which checks and reads but only prints what it
# would change. `just release-tool --help` lists it all. gpui-template keeps this file up to date (`copier update`), so
# recipes of the project's own go in the justfile.
release_tool := "uv run --quiet --project scripts/release app-release"

# Run app-release directly
[positional-arguments]
release-tool *args:
    {{release_tool}} "$@"

# Check release.toml and the version (no network, changes nothing)
release-check:
    {{release_tool}} check

# Check everything a release needs outside the repo
[positional-arguments]
doctor *args:
    {{release_tool}} doctor "$@"

# Draft release-notes/<version>.md with a cheap model, from the commits since the last release
[positional-arguments]
notes *args:
    {{release_tool}} notes "$@"

# Bump the version (major, minor, patch, or beta) and the build number; build: only the build number
[positional-arguments]
bump part *args:
    {{release_tool}} bump "$@"

# Build, sign with Developer ID, notarize and staple Patchbay.app, into build/release/<version>/export
[positional-arguments]
notarize *args:
    {{release_tool}} notarize "$@"

# Publish the newest notarized Patchbay.app
[positional-arguments]
release *args:
    {{release_tool}} release "$@"

# The whole release: bump, commit, push, notarize and publish. Ship from main
[positional-arguments]
ship part *args:
    {{release_tool}} ship "$@"
