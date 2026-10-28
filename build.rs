//! Build script: make `link.ld` visible to the linker.
//!
//! Cargo runs this on the *host* (x86-64) before compiling the crate. We copy
//! the linker script into the build output directory and tell cargo to pass
//! `-L <that dir>` to the linker. The final `-Tlink.ld` (see
//! `.cargo/config.toml`) is then resolved through that search path, which
//! means binaries in *other* crates that depend on pi5-hal also find it.
//! This is the same trick `cortex-m-rt` uses for its `link.x`.

use std::{env, fs, path::PathBuf};

fn main() {
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));

    // Build scripts run with the package root as current directory.
    fs::copy("link.ld", out_dir.join("link.ld")).expect("copy link.ld into OUT_DIR");

    println!("cargo:rustc-link-search={}", out_dir.display());

    // Only re-run when these files change, not on every build.
    println!("cargo:rerun-if-changed=link.ld");
    println!("cargo:rerun-if-changed=build.rs");
}
