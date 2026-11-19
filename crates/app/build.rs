//! Rebuilds when a translation changes: `rust_i18n::i18n!` reads `locales/` while compiling, and Cargo can't see that.

fn main() {
    println!("cargo::rerun-if-changed=locales");
}
