//! Ensure the web UI output folder exists so `rust-embed` compiles even when
//! the SPA has not been built (CI's Rust job, a fresh checkout). An empty
//! folder makes the server answer "web UI not built" instead of failing to build.

use std::path::Path;

fn main() {
    let dist = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/dist");
    std::fs::create_dir_all(&dist).expect("create web/dist");
    println!("cargo:rerun-if-changed={}", dist.display());
}
