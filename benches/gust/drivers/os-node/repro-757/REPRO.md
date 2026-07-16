# synth#757 — exact reproduction input (the module the reconstructions were missing)

`loom.wasm` (md5 `18da000d9142dfa0885f57578d3af150`, 3730 B) is the EXACT synth input that
miscompiles — the meld-fused + loom-inlined `gust:os {time, log}` node. The maintainer's 7
RawVec-grow+memmove reconstructions were all green (synth PR #772), so this is the module
they need.

## One command (synth 0.45.0, deterministic)

    synth compile loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi --shadow-stack-size 2048 -o os-tl.o
    # dissolves clean: 13 functions, 0 skipped, only read32/write32 undefined

## The wrong result

The node's `run` builds `"gust:os up\n"` (11 bytes) into a `Vec<u8>` and the provider writes
each byte to an mmio sink. Captured on the sink at runtime (qemu cortex-m3):

    got      = [2, 0, 0, 0, 1, 0, 0, 32, 117, 112, 10]
    expected = [103,117,115,116,58,111,115, 32, 117, 112, 10]   ("gust:os up\n")

Bytes 7–10 (`" up\n"`) correct; bytes 0–6 are the LOW-offset `.data` constants leaking in
(see `os-tl-0.45.data`). The copy's source address is wrong for the head chunk only.

## Isolation (holds across 0.43.0 / 0.43.1 / 0.44.0 / 0.45.0 — byte-identical)

- Pre-synth wasm is CORRECT under wasmtime (both `fused.wasm` and this `loom.wasm` emit
  `"gust:os up\n"`). Only synth's ARM object is wrong.
- `.data` holds the correct string (`os-tl-0.45.data`); the CODE that copies it is wrong
  (traced to the inlined RawVec-grow/memmove path, `func_17`, reached `run → func_20 →
  func_16 → func_17`).
- r11=0 trampoline ruled out (failure identical with/without).

`os-tl-0.45.disasm` is the full `objdump -dr` of the miscompiled object. Happy to pair on it.

## v0.5.0 I-ISO oracle set: this bug, physically contained (2026-07-16)

The archived input above is now also the tenant of the I-ISO fault-containment
flagship oracle (`src/bin/gust_iso_contain_probe.rs` + no-fault control
`gust_iso_contain_ctl.rs`, qemu lm3s6965evb — enforcement pre-verified by
`src/bin/mpu_spike.rs`). Committed here, both dissolved from the SAME
`loom.wasm` (md5 above) with the one-command recipe above:

- `os-tl-buggy.o` — synth **0.45.0** (miscompiles; md5 `f0ec01ecbd048fecb6441f686bd32d4a`)
- `os-tl-fixed.o` — synth **0.45.1** (control; md5 `db79315e928d3ff66c76d02114b7136a` is
  the checked-in `../os-tl-cm3.o` from a separate build — THIS pair's fixed object is a
  fresh dissolve so the diff below is exact)

`.text` and `.data` of the pair are BYTE-IDENTICAL; the entire 0.45.0 defect is
**one relocation**: the literal-pool word at `.text+0x694` (in `func_20`, inline
addend `+8` — the log string-copy's head-chunk source pointer) is bound to
`__synth_wasm_seg_0` instead of `__synth_wasm_seg_2`:

    $ objdump -r os-tl-buggy.o | diff - <(objdump -r os-tl-fixed.o)
    < 00000694 R_ARM_ABS32   __synth_wasm_seg_0     (buggy: seg_0+8 = the stale constants)
    > 00000694 R_ARM_ABS32   __synth_wasm_seg_2     (fixed: seg_2+8 = "gust:os up\n")

Re-verified live before the oracle was built: `gust_os_tl_probe` linked against
`os-tl-buggy.o` FAILs with exactly the head-byte signature above
(`[2, 0, 0, 0, 1, 0, 0, 32, 117, 112, 10]`). The containment probe then places
the renamed `.data` at `0x2000_BFF0` (see `../../../iso_contain.x`) so
`seg_0+8 = 0x2000_BFF8` sits in MPU-denied SRAM while everything the correct
program needs is granted, programs the MPU through the VERIFIED
`gale::mpu_switch` core, and observes the miscompiled read MemManage-fault at
exactly `MMFAR == 0x2000_BFF8` with 0 bytes reaching the log sink.
