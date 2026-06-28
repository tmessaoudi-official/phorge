//! Build script (M2.5 Phase 3a): bake the cross-stub integrity manifest into the binary.
//!
//! `src/bundle/manifest.rs` does `include_str!(concat!(env!("OUT_DIR"), "/stub_manifest.txt"))`. Here
//! we write that file:
//!   * `PHORJ_BAKE_STUB_MANIFEST=<path>` set (the CI primary build) → copy the file's contents in;
//!   * unset (every other build — dev, and the cross-stub builds themselves) → write an EMPTY file.
//!
//! An empty manifest for the stub builds is what breaks the stub↔manifest circularity: a stub built
//! with the env unset has manifest-independent bytes, so its SHA-256 is stable and can be recorded in
//! the manifest the primary then bakes (design §6 / P3-3). `build.rs` is host build tooling — it runs
//! at build time and never links into the artifact, so the std-only line is intact.

use std::path::Path;

fn main() {
    // Rebuild whenever the bake env changes (set ↔ unset, or a new manifest path).
    println!("cargo:rerun-if-env-changed=PHORJ_BAKE_STUB_MANIFEST");

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR is always set for a build script");
    let dest = Path::new(&out_dir).join("stub_manifest.txt");

    let contents = match std::env::var_os("PHORJ_BAKE_STUB_MANIFEST") {
        Some(path) => {
            // Re-run if the source manifest itself changes.
            println!("cargo:rerun-if-changed={}", path.to_string_lossy());
            std::fs::read_to_string(&path).unwrap_or_else(|e| {
                panic!(
                    "PHORJ_BAKE_STUB_MANIFEST={} could not be read: {e}",
                    path.to_string_lossy()
                )
            })
        }
        None => String::new(),
    };

    std::fs::write(&dest, contents).expect("write baked stub manifest");
}
