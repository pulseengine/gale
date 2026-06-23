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
    // Reproduce (loom 1.1.16 + synth 0.12.0): loom optimize gust_kernel.wasm
    //   --passes inline (merges the gust_mix wrapper into its body, loom#228),
    //   strip exports to {memory, gust_mix}, then synth compile <stripped>.wasm
    //   --target cortex-m3 --all-exports --relocatable. COMPARE.md tracks the
    //   measured 2.81x -> 2.63x progression and the residual synth#428 gap.
    let kobj = Path::new(&manifest).join("wasm-kernel/gust_mix-cm3.o");
    if kobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_codegen_bench={}", kobj.display());
        println!("cargo:rerun-if-changed={}", kobj.display());
    }
    // The dissolved gale deciders (sem/msgq/mutex/event) for the regression
    // differential — all 8 verified primitives as the "shim as wasm". Reproduce:
    //   (cd browser && cargo build --release --target wasm32-unknown-unknown)
    //   loom optimize gust_browser.wasm --passes inline | synth compile --target
    //   cortex-m3 --all-exports --relocatable  (flag-off = dec_diff.o, the default;
    //   set SYNTH_CMP_SELECT_FUSE=1 for the flag-on variant). gale_decider_diff
    //   folds every decision into a checksum so off/on/native are byte-comparable.
    let dobj = Path::new(&manifest).join("wasm-kernel/dec_diff.o");
    if dobj.exists() {
        println!("cargo:rustc-link-arg-bin=gale_decider_diff={}", dobj.display());
        println!("cargo:rerun-if-changed={}", dobj.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
