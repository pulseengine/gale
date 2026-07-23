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
use std::process::Command;

/// Symbol-presence guard against the synth#746-class silent-skip defect
/// (disclosed in drivers/exec-provider/RESULTS.md's "separate synth finding"
/// section): a dissolve can silently DROP a function from the relocatable
/// object (no warning, no error — the function is simply absent), which
/// otherwise only surfaces as an undefined-symbol link error IF something
/// still references it, or worse, as silently-absent logic if nothing does.
/// Run `nm` on the object and require each name in `required` to appear as a
/// DEFINED text symbol (`T`/`t`); anything missing or undefined (`U`) fails
/// the BUILD rather than the field. Hermetic: if no `nm`-compatible tool is
/// on PATH, this warns and skips rather than failing (keeps the bench
/// buildable without the ARM toolchain installed) — but once `nm` DOES run,
/// a missing required symbol is a hard build failure, not a warning.
fn check_defined_text_symbols(obj: &Path, required: &[&str]) {
    const NM_CANDIDATES: &[&str] = &[
        "/opt/homebrew/bin/arm-none-eabi-nm",
        "arm-none-eabi-nm",
        "llvm-nm",
    ];
    let nm = NM_CANDIDATES
        .iter()
        .find(|c| Command::new(c).arg("--version").output().is_ok());
    let Some(nm) = nm else {
        println!(
            "cargo:warning=no nm-compatible tool (arm-none-eabi-nm/llvm-nm) found on PATH; \
             skipping symbol-presence guard for {} (synth#746-class silent-skip would NOT be caught)",
            obj.display()
        );
        return;
    };
    let output = Command::new(nm)
        .arg(obj)
        .output()
        .unwrap_or_else(|e| panic!("failed to run {nm} on {}: {e}", obj.display()));
    if !output.status.success() {
        println!(
            "cargo:warning={nm} exited non-zero on {}; skipping symbol-presence guard",
            obj.display()
        );
        return;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let is_defined_text = |sym: &str| {
        text.lines().any(|l| {
            let parts: Vec<&str> = l.split_whitespace().collect();
            match parts.as_slice() {
                // "<addr> <type> <name>" (defined) or "<type> <name>" (undefined, no addr)
                [_, ty, name] | [ty, name] if *name == sym => matches!(*ty, "T" | "t"),
                _ => false,
            }
        })
    };
    let missing: Vec<&str> = required
        .iter()
        .copied()
        .filter(|sym| !is_defined_text(sym))
        .collect();
    if !missing.is_empty() {
        panic!(
            "symbol-presence guard FAILED for {}: expected symbols {missing:?} to be DEFINED \
             text symbols (T/t) but nm did not report them as such (missing entirely, or only \
             undefined 'U') — this is exactly the synth#746-class silent-skip failure mode: a \
             dissolved function silently absent from the object with no compile warning. Full \
             nm output:\n{text}",
            obj.display()
        );
    }
}

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
    // Reproduce (loom 1.1.18 + synth 0.38.0, SYNTH_SHIFT_MASK_ELIDE=1): loom optimize
    //   gust_kernel.wasm --passes inline (merges the gust_mix wrapper into its body,
    //   loom#228), strip exports to {memory, gust_mix}, then
    //   SYNTH_SHIFT_MASK_ELIDE=1 synth compile <stripped>.wasm --target cortex-m3
    //   --all-exports --relocatable (cortex-m4 for the cm4 .o).
    //   COMPARE.md tracks 2.81x -> 2.63x -> 1.81x -> 1.69x -> 1.50x: synth v0.13-0.15
    //   shipped the four #428 levers default-on; 0.37.1 refreshed a STALE ~0.15-era
    //   pin (90 B / 0.725 t) to 82 B / 0.675 t; 0.38.0's SYNTH_SHIFT_MASK_ELIDE (#686,
    //   the beat-LLVM lever gale filed) elides #682's mod-32 mask where the shift
    //   amount is provably <32 -> 68 B / 0.600 t / 1.50x, -11% cycles vs 0.37.1,
    //   soundness-gated (gust_floor_bench mix_proven == native == gust_mix). NOTE: the
    //   relocatable .o grew 432 -> 496 B from 0.38.0's new ELF metadata (#656 STB_LOCAL
    //   + #637 .ARM.attributes) — dropped/merged at link, so flash tracks the 68 B
    //   .text. The flag is REQUIRED to rebuild these .o's; 0.38.0 DEFAULT is 1.69x.
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
    let tobj = Path::new(&manifest).join("drivers/timer-thin/timer-thin-cm3.o");
    if tobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_timer={}", tobj.display());
        println!("cargo:rerun-if-changed={}", tobj.display());
    }
    // The dissolved thin-seam SPI driver (drivers/spi-thin → loom → synth): the
    // STM32F1 SPI protocol (CR1 mode/baud config + full-duplex byte shift) and a
    // Kani-proven transfer FSM (SQE→CQE, exclusive-bus + no-lost-byte) in verified
    // wasm (Kani 6/6, 0 new TCB atoms — mmio only). The gust_spi demonstrator
    // asserts the CR1 write + byte shift + FSM over USART1 for the content-gate
    // (renode-test/gust_spi.robot).
    let spobj = Path::new(&manifest).join("drivers/spi-thin/spi-thin-cm3.o");
    if spobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_spi={}", spobj.display());
        // gust_spi_probe: the LOCAL qemu-semihosting probe of the SAME dissolved .o
        // (RAM-window register effects + FSM), run before the Renode gate.
        println!("cargo:rustc-link-arg-bin=gust_spi_probe={}", spobj.display());
        println!("cargo:rerun-if-changed={}", spobj.display());
    }
    // The dissolved thin-seam IWDG (independent watchdog) driver (drivers/wdg-thin →
    // loom → synth): the whole STM32F1 IWDG key-sequence lifecycle — 0x5555 unlock /
    // PR+RLR config / 0xCCCC start / 0xAAAA refresh, and the Kani-proven
    // cannot-un-start property (no software disable transition) — in verified wasm
    // (Kani 7/7, 0 new TCB atoms — mmio only). gust_wdg_probe is the LOCAL
    // qemu-semihosting demonstrator of this SAME dissolved .o (RAM-window register
    // effects + cannot-un-start), run before the `gust-wdg-renode` content-gate
    // (renode-test/gust_wdg.robot).
    let wobj = Path::new(&manifest).join("drivers/wdg-thin/wdg-thin-cm3.o");
    if wobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_wdg={}", wobj.display());
        println!("cargo:rustc-link-arg-bin=gust_wdg_probe={}", wobj.display());
        // gust_wdg_silicon: the same dissolved .o driving the REAL IWDG on a
        // NUCLEO-G474RE (thumbv7m ⊂ thumbv7em; IWDG is register-identical F1==G4).
        println!("cargo:rustc-link-arg-bin=gust_wdg_silicon={}", wobj.display());
        println!("cargo:rerun-if-changed={}", wobj.display());
    }
    // The 4-driver breadth node (REQ-DRV-BREADTH-001): gpio+timer+spi+uart, each a
    // verified-wasm gust:hal component, wac/meld-fused → ONE dissolved .o exporting
    // all 20 protocol fns (C-renamed), 0 SRAM, no func_N collision. Built by
    // drivers/build-breadth.sh. gust_breadth (Renode gate) + gust_breadth_probe
    // (local qemu liveness probe) link it; bridge = read32/write32/poll (3 atoms).
    let bobj = Path::new(&manifest).join("drivers/breadth/breadth-cm3.o");
    if bobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_breadth={}", bobj.display());
        println!("cargo:rustc-link-arg-bin=gust_breadth_probe={}", bobj.display());
        println!("cargo:rerun-if-changed={}", bobj.display());
    }
    // gust:os v0.4.0 syscall seam, step-1 node (drivers/os-node/os-time-cm3.o): an
    // app importing only gust:os/time, wac-plugged with a time provider, dissolved
    // to one 0-SRAM object exporting `run`, importing only read32. gust_os_probe is
    // the local qemu liveness check.
    let osobj = Path::new(&manifest).join("drivers/os-node/os-time-cm3.o");
    if osobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_os_probe={}", osobj.display());
        println!("cargo:rerun-if-changed={}", osobj.display());
    }
    // gust:os v0.4.0 syscall seam, step-2 node (drivers/os-node/os-tl-cm3.o): an app
    // importing gust:os {time, log} — log.line is the first BUFFER-CARRYING capability
    // (list<u8>) crossing the seam — wac-plugged with a time provider + a log
    // provider, dissolved to one bounded-SRAM object exporting `run`, importing only
    // read32/write32. gust_os_tl_probe is the local qemu liveness check.
    let tlobj = Path::new(&manifest).join("drivers/os-node/os-tl-cm3.o");
    if tlobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_os_tl_probe={}", tlobj.display());
        println!("cargo:rerun-if-changed={}", tlobj.display());
    }
    // gust:os v0.4.0 syscall seam, step-3 node (drivers/os-node/os-ts-cm3.o): an app
    // importing gust:os {time, spawn} — spawn is the first EXECUTOR-BACKED capability
    // (start/poll marshal onto the verified executor) — wac-plugged with a time
    // provider + a spawn provider, dissolved to one bounded-SRAM object exporting
    // `run`, importing only read32 + the trusted `poll-task` dispatch seam
    // (gust:os/taskdisp). gust_os_ts_probe is the local qemu liveness check.
    let tsobj = Path::new(&manifest).join("drivers/os-node/os-ts-cm3.o");
    if tsobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_os_ts_probe={}", tsobj.display());
        println!("cargo:rerun-if-changed={}", tsobj.display());
    }
    // gust:os v1 async executor (Task 6, REQ-OS-EXEC-001): the Verus+Kani-proven
    // scheduler core (src/executor.rs), dissolved SINGLE-component (no wac plug,
    // no meld fuse -> not synth#739-blocked) via drivers/exec-provider ->
    // drivers/os-node/exec-cm3.o. gust_exec_probe is the qemu liveness oracle.
    let eobj = Path::new(&manifest).join("drivers/os-node/exec-cm3.o");
    if eobj.exists() {
        // Guard against the synth#746-class silent-skip gap disclosed in
        // exec-provider/RESULTS.md: require exec_admit/exec_poll_round/
        // exec_state to actually be defined text symbols in the object.
        // `poll_task` is deliberately NOT checked here — it is the trusted
        // FFI seam, expected undefined until this probe's own `poll_task`
        // resolves it at final native link.
        check_defined_text_symbols(&eobj, &["exec_admit", "exec_poll_round", "exec_state"]);
        println!("cargo:rustc-link-arg-bin=gust_exec_probe={}", eobj.display());
        println!("cargo:rerun-if-changed={}", eobj.display());
    }
    // I-ISO fault-containment oracles (v0.5.0): gust_iso_contain_probe links the
    // BUGGY synth 0.45.0 dissolve of the ARCHIVED synth#757 miscompile input
    // (repro-757/loom.wasm -> repro-757/os-tl-buggy.o — its ONLY difference from
    // the fixed 0.45.1 dissolve, repro-757/os-tl-fixed.o, is the one R_ARM_ABS32
    // at .text+0x694 bound to __synth_wasm_seg_0 instead of __synth_wasm_seg_2);
    // gust_iso_contain_ctl links the fixed object under the IDENTICAL memory
    // arrangement as the no-fault control. Both need the object's .data renamed
    // to .iso_stale_data (objcopy, into OUT_DIR) so iso_contain.x can pin it at
    // 0x2000_BFF0 straddling the MPU guard boundary — see iso_contain.x.
    let out_dir = std::env::var("OUT_DIR").unwrap();
    const OBJCOPY_CANDIDATES: &[&str] = &["/opt/homebrew/bin/arm-none-eabi-objcopy", "arm-none-eabi-objcopy", "llvm-objcopy"];
    let objcopy = OBJCOPY_CANDIDATES
        .iter()
        .find(|c| Command::new(c).arg("--version").output().is_ok());
    for (src, bin, placed) in [
        ("drivers/os-node/repro-757/os-tl-buggy.o", "gust_iso_contain_probe", "os-tl-buggy-placed.o"),
        ("drivers/os-node/repro-757/os-tl-fixed.o", "gust_iso_contain_ctl", "os-tl-fixed-placed.o"),
    ] {
        let sobj = Path::new(&manifest).join(src);
        if !sobj.exists() {
            continue;
        }
        let Some(objcopy) = objcopy else {
            println!(
                "cargo:warning=no objcopy-compatible tool found; {bin} cannot place \
                 .iso_stale_data and will fail to link if built"
            );
            continue;
        };
        let out = Path::new(&out_dir).join(placed);
        let status = Command::new(objcopy)
            .args(["--rename-section", ".data=.iso_stale_data"])
            .arg(&sobj)
            .arg(&out)
            .status()
            .unwrap_or_else(|e| panic!("failed to run {objcopy}: {e}"));
        assert!(status.success(), "{objcopy} failed renaming .data in {}", sobj.display());
        // synth#746-class guard: the dissolved entry point must actually be there.
        check_defined_text_symbols(&out, &["run"]);
        println!("cargo:rustc-link-arg-bin={bin}={}", out.display());
        println!(
            "cargo:rustc-link-arg-bin={bin}=-T{}",
            Path::new(&manifest).join("iso_contain.x").display()
        );
        println!("cargo:rerun-if-changed={}", sobj.display());
    }
    println!("cargo:rerun-if-changed=iso_contain.x");
    println!("cargo:rerun-if-changed=build.rs");

    // The dissolved thin-seam ADC driver (drivers/adc-thin -> loom -> synth --target
    // cortex-m3 --all-exports --relocatable): the whole STM32F1 ADC single-conversion
    // path — SMPR sample-time + SQR regular-sequence config and the
    // enable->start->EOC->read cycle — in verified wasm (Kani 7/7, 0 new TCB atoms —
    // mmio only). The Kani-proven distinctive property is read-after-EOC exactly-once /
    // single-shot. gust_adc_probe is the LOCAL qemu-semihosting demonstrator of this
    // SAME dissolved .o (RAM-window register effects + read-after-EOC), run before the
    // `gust-adc-renode` content-gate (renode-test/gust_adc.robot); gust_adc drives it
    // on a real STM32 model.
    let aobj = Path::new(&manifest).join("drivers/adc-thin/adc-thin-cm3.o");
    if aobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_adc={}", aobj.display());
        println!("cargo:rustc-link-arg-bin=gust_adc_probe={}", aobj.display());
        // gust_adc_silicon points the SAME dissolved .o at the real F100 ADC1 to read
        // Vrefint (ch17) on hardware — the silicon anchor for the adc-thin driver.
        println!("cargo:rustc-link-arg-bin=gust_adc_silicon={}", aobj.display());
        println!("cargo:rerun-if-changed={}", aobj.display());
    }

    // The dissolved thin-seam DAC driver (drivers/dac-thin -> loom -> synth --target
    // cortex-m3 --all-exports --relocatable): the whole STM32F1 software-triggered DAC
    // path — CR channel/trigger config + the enable->load->trigger->output cycle — in
    // verified wasm (Kani 7/7, 0 new TCB atoms: mmio read32/write32 only), scalar
    // packed-u32 FSM (phase[31:30]/channel[29]/value[11:0]), table-free. gust_dac_probe
    // is the LOCAL qemu-semihosting demonstrator of this SAME dissolved .o (RAM-window
    // register effects + the Kani-proven glitch-free trigger-gated output), run before
    // the `gust-dac-renode` content-gate (renode-test/gust_dac.robot).
    let dacobj = Path::new(&manifest).join("drivers/dac-thin/dac-thin-cm3.o");
    if dacobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_dac={}", dacobj.display());
        println!("cargo:rustc-link-arg-bin=gust_dac_probe={}", dacobj.display());
        println!("cargo:rerun-if-changed={}", dacobj.display());
    }

    // The dissolved thin-seam PWM driver (drivers/pwm-thin → loom → synth --target
    // cortex-m3 --all-exports --relocatable): the whole STM32 advanced-timer PWM output
    // path — CCMR1 PWM-mode-1 + preload config, PSC/ARR period, CCR duty, CCER/BDTR.MOE
    // output enable — in verified wasm (Kani 7/7, 0 new TCB atoms; PWM is a pure-output
    // path so the ONLY import is env.mmio_write32). Its distinctive Kani-proven safety
    // properties are the duty clamp (CCR ≤ ARR always) and a total+latching failsafe
    // (MOE cleared from any state, un-clearable by a stray start). gust_pwm_probe is the
    // LOCAL qemu-semihosting demonstrator of this SAME dissolved .o (RAM-window register
    // effects + clamp + failsafe-latch), run before the `gust-pwm-renode` content-gate
    // (renode-test/gust_pwm.robot).
    let pobj = Path::new(&manifest).join("drivers/pwm-thin/pwm-thin-cm3.o");
    if pobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_pwm={}", pobj.display());
        println!("cargo:rustc-link-arg-bin=gust_pwm_probe={}", pobj.display());
        println!("cargo:rerun-if-changed={}", pobj.display());
    }

    // The dissolved thin-seam I2C driver (drivers/i2c-thin → loom → synth): the whole
    // STM32F1 I2C master path — CR2 FREQ + CCR + TRISE timing config (table-free bit
    // arithmetic) and the START→address→data→STOP transaction FSM with the Kani-proven
    // ACK-all-but-last rule — in verified wasm (Kani 7/7, 0 new TCB atoms — mmio only).
    // gust_i2c_probe is the LOCAL qemu-semihosting demonstrator of this SAME dissolved
    // .o (RAM-window register effects + ACK-all-but-last), run before the
    // `gust-i2c-renode` content-gate (renode-test/gust_i2c.robot); gust_i2c is the
    // Renode gate's ELF, driving the identical transaction over a real USART1.
    let iobj = Path::new(&manifest).join("drivers/i2c-thin/i2c-thin-cm3.o");
    if iobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_i2c={}", iobj.display());
        println!("cargo:rustc-link-arg-bin=gust_i2c_probe={}", iobj.display());
        println!("cargo:rerun-if-changed={}", iobj.display());
    }

    // The dissolved thin-seam CAN (bxCAN) driver (drivers/can-thin -> loom -> synth):
    // the whole STM32F1 bxCAN master path — BTR bit-timing config (write-protected to
    // Init only), the INRQ/INAK init handshake, and TX-mailbox / RX-FIFO gating, with
    // the Kani-proven config-only-in-init property — in verified wasm (Kani 7/7, 0 new
    // TCB atoms — mmio only). gust_can_probe is the LOCAL qemu-semihosting demonstrator
    // of this SAME dissolved .o (RAM-window register effects + config-only-in-init),
    // run before the `gust-can-renode` content-gate (renode-test/gust_can.robot).
    let cobj_can = Path::new(&manifest).join("drivers/can-thin/can-thin-cm3.o");
    if cobj_can.exists() {
        println!("cargo:rustc-link-arg-bin=gust_can={}", cobj_can.display());
        println!("cargo:rustc-link-arg-bin=gust_can_probe={}", cobj_can.display());
        println!("cargo:rerun-if-changed={}", cobj_can.display());
    }

    // The dissolved DMA-as-own<buffer> ownership driver (drivers/dma-own → synth
    // --target cortex-m3 --all-exports --relocatable): the DMA transfer OWNERSHIP
    // round-trip FSM in verified wasm (gale#124, Kani 6/6 — p1..p6 ownership +
    // barrier-by-construction), importing only the gust:hal dma seam (dma_program /
    // dma_barrier / dma_irq_poll — the 3 irreducible trusted atoms). gust_dma_probe
    // is the LOCAL qemu-semihosting demonstrator of this SAME dissolved .o
    // (RAM-window register effects through the seam + the ownership round-trip),
    // run before the `gust-dma-renode` content-gate (renode-test/gust_dma.robot).
    let dmaobj = Path::new(&manifest).join("drivers/dma-own/dma-own-cm3.o");
    if dmaobj.exists() {
        println!("cargo:rustc-link-arg-bin=gust_dma={}", dmaobj.display());
        println!("cargo:rustc-link-arg-bin=gust_dma_probe={}", dmaobj.display());
        println!("cargo:rerun-if-changed={}", dmaobj.display());
    }
}
