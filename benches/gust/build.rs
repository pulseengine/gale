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
        // gust_stack drives the same dissolved composition (run-demo) as a kiln task.
        println!("cargo:rustc-link-arg-bin=gust_stack={}", obj.display());
        println!("cargo:rerun-if-changed={}", obj.display());
    }
    // The dissolved `gust_mix` (wasm→loom→synth→cortex-m3), as a clean relocatable
    // object (no linmem .bss — gust_mix is pure scalar). Linked into the codegen
    // micro-bench only, so it can time the native (LLVM) vs dissolved (synth)
    // lowering of the SAME source. Scoped via -bin= so other bins are unaffected.
    // Reproduce (loom 1.1.16 + synth 0.15.0): loom optimize gust_kernel.wasm
    //   --passes inline (merges the gust_mix wrapper into its body, loom#228),
    //   strip exports to {memory, gust_mix}, then synth compile <stripped>.wasm
    //   --target cortex-m3 --all-exports --relocatable. COMPARE.md tracks the
    //   measured 2.81x -> 2.63x -> 1.81x progression: synth v0.13-0.15 shipped the
    //   four #428 levers default-on (cmp->select fusion, stack-reload elim, local
    //   promotion, immediate-shift fold), -31% cycles / -32% .text, bit-identical.
    let kobj = Path::new(&manifest).join("wasm-kernel/gust_mix-cm3.o");
    if kobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_codegen_bench={}", kobj.display());
        // gust_floor_bench links the SAME dissolved gust_mix to show today's
        // dissolved-vs-native gap alongside the proof-carrying floor (synth#494a).
        println!("cargo:rustc-link-arg-bin=gust_floor_bench={}", kobj.display());
        println!("cargo:rerun-if-changed={}", kobj.display());
    }
    // silicon_bench runs on two MCUs and needs an arch-matched dissolved object:
    // a synth --target cortex-m3 .o links into a thumbv7m (F100) image; a
    // thumbv7em (G474RE/M4) image needs the synth --target cortex-m4 .o (the M3
    // object's ARMv7-M attributes make rust-lld silently emit an empty ELF when
    // linked into a v7E-M binary). Pick by the cargo TARGET. The cortex-m4 .o is
    // also the correct artifact for the M4 vs LLVM-thumbv7em comparison (synth#428).
    let target = std::env::var("TARGET").unwrap_or_default();
    let silicon_o = if target.contains("thumbv7em") {
        "wasm-kernel/gust_mix-cm4.o"
    } else {
        "wasm-kernel/gust_mix-cm3.o"
    };
    let sobj = Path::new(&manifest).join(silicon_o);
    if sobj.exists() {
        println!("cargo:rustc-link-arg-bin=silicon_bench={}", sobj.display());
        println!("cargo:rerun-if-changed={}", sobj.display());
    }
    println!("cargo:rustc-rerun-if-env-changed=TARGET");
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
    // The dissolved engine_control control_step — driven on the kiln stack by the
    // gust_control demonstrator (north-star rung 1: a realistic sensors→actuators
    // loop). Reproduce: clang wasm32 control.c+tables.c+shim.c → wasm-ld
    //   --export=control_step_packed → loom inline → synth --target cortex-m3
    //   --all-exports --relocatable. HISTORY: built with SYNTH_NO_LOCAL_PROMOTE=1
    //   on synth 0.14.0–0.15.0 because default-on local promotion register-
    //   exhausted on this denser function (synth#474). synth 0.15.1 (#475) FIXED
    //   that (recovery ladder) — verified: compiles clean WITHOUT the flag,
    //   byte-identical output (568 B). The flag is no longer needed on 0.15.1+.
    // arch-matched, like silicon_bench: cortex-m4 .o for thumbv7em (G474RE), else
    // cortex-m3 (qemu/F100). Both are --native-pointer-abi (table data) and driven
    // via the r11=0 trampoline in gust_control.rs.
    let cs_o = if target.contains("thumbv7em") {
        "wasm-kernel/control_step-cm4.o"
    } else {
        "wasm-kernel/control_step-cm3.o"
    };
    let cobj = Path::new(&manifest).join(cs_o);
    if cobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_control={}", cobj.display());
        println!("cargo:rerun-if-changed={}", cobj.display());
    }
    // The dissolved thin-seam UART driver (drivers/uart-thin → loom → synth
    // --native-pointer-abi): the ENTIRE STM32 USART protocol in verified wasm
    // (Kani-proven RX decision), importing only gust:hal mmio + irq. Driven by the
    // gust_uart demonstrator, whose ~10-line bridge supplies mmio_read32/write32 +
    // irq_poll. Reproduce: see drivers/uart-thin/RESULTS.md.
    let uobj = Path::new(&manifest).join("drivers/uart-thin/uart-thin-cm3.o");
    if uobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_uart={}", uobj.display());
        println!("cargo:rerun-if-changed={}", uobj.display());
    }
    // The dissolved thin-seam GPIO driver (drivers/gpio-thin → loom → synth): the
    // entire STM32F1 GPIO protocol in verified wasm (Kani 4/4, 0 new TCB atoms —
    // mmio only). The gust_gpio demonstrator links BOTH gpio + uart drivers: it
    // exercises gpio_configure/set/clear and emits the register-effect results over
    // USART1 for the Renode content-gate (renode-test/gust_gpio.robot).
    let gobj = Path::new(&manifest).join("drivers/gpio-thin/gpio-thin-cm3.o");
    if gobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_gpio={}", gobj.display());
        println!("cargo:rerun-if-changed={}", gobj.display());
    }
    println!("cargo:rerun-if-changed=build.rs");
}
