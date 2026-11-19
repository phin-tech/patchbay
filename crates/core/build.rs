//! Hands the code the build number in the workspace's Cargo.toml, which app-release bumps with the version, as `BUILD`.

use std::{env, fs, path::Path};

fn main() {
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../Cargo.toml");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = fs::read_to_string(&manifest).expect("the workspace's Cargo.toml");
    let table: toml::Table = text.parse().expect("the workspace's Cargo.toml is TOML");
    let build = table["workspace"]["metadata"]["release"]["build"]
        .as_str()
        .expect("a build number, as a string, under [workspace.metadata.release]");
    println!("cargo::rustc-env=BUILD={build}");
}
