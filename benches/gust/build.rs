// Link the meld-fused, synth-dissolved Component-Model composition into the
// `gust_fused` demonstrator bin only.
//
// wasm-kernel/fused.o is the gale-app-demo + gale-kiln Component-Model
// composition (app imports gale:kernel; gale-kiln provides it over the verified
// gale::* decisions), MELD-fused (--memory shared --address-rebase) into one
// merged-memory core, then loom-inlined and synth-compiled to a relocatable
// Cortex-M3 object exporting `run-demo`. Built by build-fused.sh; checked in so
// the bench builds without the full dissolve toolchain on PATH. Scoped to the
// gust_fused bin via -bin= so the native `gust` / `bench` binaries are
// unaffected.
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let obj = Path::new(&manifest).join("wasm-kernel/fused.o");
    if obj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_fused={}", obj.display());
        println!("cargo:rerun-if-changed={}", obj.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
