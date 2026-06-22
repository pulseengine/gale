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
    // The dissolved `gust_mix` (wasm→loom→synth→cortex-m3), as a clean relocatable
    // object (no linmem .bss — gust_mix is pure scalar). Linked into the codegen
    // micro-bench only, so it can time the native (LLVM) vs dissolved (synth)
    // lowering of the SAME source. Scoped via -bin= so other bins are unaffected.
    // Reproduce: strip gust_kernel.wasm exports to {memory, gust_mix}, then
    //   synth compile <stripped>.wasm --target cortex-m3 --all-exports --relocatable
    let kobj = Path::new(&manifest).join("wasm-kernel/gust_mix-cm3.o");
    if kobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_codegen_bench={}", kobj.display());
        println!("cargo:rerun-if-changed={}", kobj.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
