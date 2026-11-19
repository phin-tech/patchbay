# Patchbay's CI, for starci (https://git.phin.tech/sam/starci).
#
#   just ci        run it here, against what is committed
#
# Every step is bare, on a Mac: GPUI needs macOS and Metal, so there's no image to run it in. A bare step gets a HOME of
# its own and none of the machine's environment, so Cargo and rustup are pointed at caches kept between runs, and
# rustup installs what rust-toolchain.toml names. The machine only has to provide rustup on its PATH.

CACHES = {
    "rustup": "RUSTUP_HOME",
    "cargo": "CARGO_HOME",
    "patchbay-target": "CARGO_TARGET_DIR",
}
MAC = {"os": "darwin"}

def cargo(name, command, needs = []):
    return step(name, run = command, caches = CACHES, on = MAC, needs = needs)

def ci(ctx):
    fmt = cargo("fmt", "cargo fmt --all --check")
    clippy = cargo("clippy", "cargo clippy --workspace --all-targets --locked -- -D warnings", needs = [fmt])
    shots = cargo("clippy with screenshots", "cargo clippy -p patchbay --all-targets --locked --features screenshot -- -D warnings", needs = [fmt])
    test = cargo("test", "cargo test --workspace --locked", needs = [fmt])
    # The Mac has to have cargo-deny and typos on its PATH for this to check anything; without, it says so and passes.
    lint = cargo("lint", " && ".join([
        "if command -v cargo-deny >/dev/null; then cargo deny check; else echo no cargo-deny here; fi",
        "if command -v typos >/dev/null; then typos; else echo no typos here; fi",
    ]), needs = [fmt])
    return [fmt, clippy, shots, test, lint]
