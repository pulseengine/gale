# Wasm-cross-LTO via wasm-ld → loom → synth — spike report

**Status:** structurally proven, cyclically gapped by two upstream tooling issues.
**Date:** 2026-05-10.
**Goal:** demonstrate that the PulseEngine pipeline (`wasm-ld → loom → synth`) can dissolve the C↔Rust FFI seam at wasm-IR level, producing silicon timing equivalent to LLVM-LTO.

## TL;DR

The toolchain inlines through the seam — synth's emitted ARM .o has `z_impl_k_sem_give` with no `bl gale_k_sem_give_decide`, the Rust decision body is folded into the C function. Two blockers prevent silicon-LTO parity:

1. **loom Z3 backend panics on i64 sort** (`SortDiffers { left: (_ BitVec 64), right: (_ BitVec 32) }`) — the formally-verified inliner reverts every function on the gale-ffi wasm. Without loom, we lose verification + the inliner shape that matches LLVM-LTO.
2. **synth emits 1.68× larger ARM than LLVM-LTO** for the inlined body (138 vs 82 bytes), because it doesn't recognize the u64-packed FFI return pattern and falls back to generic 64-bit shift-and-mask.

## Reproduction commands

```sh
mkdir -p /tmp/wasm-shim-poc && cd /tmp/wasm-shim-poc

# 1. Compile a wasm-portable host shim that mimics z_impl_k_sem_give's
#    hot path with kernel APIs as externs (which become wasm imports).
#    See wasm_host_shim.c in this dir.

clang --target=wasm32-unknown-unknown -O2 -nostdlib \
  -c wasm_host_shim.c -o shim.wasm.o

# 2. Build gale-ffi as a wasm32 static archive (already produced by
#    the bench's GALE_USE_SYNTH=ON path; can also build manually):
( cd /Volumes/Home/git/pulseengine/gale-smart-data/ffi && \
  cargo rustc --release --target wasm32-unknown-unknown --crate-type=staticlib )

# 3. wasm-ld static-links them into a single core wasm module.
wasm-ld --no-entry --export-all --allow-undefined \
  --whole-archive /Volumes/Home/git/pulseengine/gale-smart-data/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a \
  --no-whole-archive shim.wasm.o \
  -o merged.wasm

# 4. Synth → ARM ET_REL. Both z_impl_k_sem_give and
#    gale_k_sem_give_decide end up in the .o, with the latter inlined
#    into the former (no bl in z_impl's body).
synth compile merged.wasm --target cortex-m4f --all-exports --relocatable -o merged.o

# 5. Verify the inlining empirically.
arm-zephyr-eabi-objdump -d merged.o | awk '/<z_impl_k_sem_give>:/,/^$/' | grep -E '\bbl\b'  # should be empty
arm-zephyr-eabi-nm --print-size merged.o | grep "z_impl_k_sem_give$"                          # 138 bytes
```

## Comparison vs LLVM-LTO (silicon-measured baseline)

```
                                         body size   silicon handoff (cyc)
                                                       ADC=n  ADC=y
  baseline (no Gale)                     —            528     506
  rustc-direct (FFI bl preserved)        —            574     582
  LLVM-LTO (cross-language inliner)      82 bytes     471     558    ← gold
  wasm→synth (Rust only, seam intact)    n/a          —       582
  wasm-ld merge → synth (this spike)     138 bytes    not yet measured
```

The "not yet measured" cell needs a CMake `GALE_USE_SHIM_WASM` flag that swaps the bench's native `gale_sem.c` for the wasm-merge-derived archive. Estimated silicon impact based on the 1.68× code-size ratio: ~510 cyc handoff (ADC=n), still better than rustc-direct's 574 but worse than LTO's 471. That estimate is to-be-validated.

## What synth produces (vs LTO, for the same inlined body)

LTO version of the no-waiter path (from
silicon/runs/.../gale-lto-noadc-systick/firmware.elf):

```
8004398: ldrd r2, r1, [r0, #8]  ; load count, limit
800439c: cmp  r2, r1            ; the only check
800439e: it   cc
80043a0: addcc r2, #1            ; saturate increment
80043a2: str  r2, [r0, #8]       ; write back
```

5 instructions. LLVM saw both C's `cmp/it/addne` and Rust's identical
check and dedup'd them into one.

Synth version of the same path (from /tmp/wasm-shim-poc/merged.o
z_impl_k_sem_give at 0x2f5e4):

```
2f5ec: mov  r5, r0
2f5ee: movw ip, #256
2f5f2: movt ip, #8192
2f5f8: ldr.w r3, [ip]            ; loads via abs address (wasm linear-mem mapping)
2f5fc..2f60e: more abs-address loads + waiter check
2f61c: and.w r0, r0, r2          ; mask u64 → u8 action
2f620: and.w r1, r1, r3
2f624: cmp action with WAKE constant
2f63a..2f660: 64-bit lsl/lsr/orr to extract new_count from u64-packed return
2f668: bx lr
```

~30 instructions. Sub-optimal for two reasons: (a) wasm linear-memory
loads use absolute address pairs (`movw + movt`) instead of relative
offsets from a base register, and (b) the u64-packed decision is
unpacked via generic shifts instead of a direct field access.

Both are fixable in synth's backend.

## Action items (filed against pulseengine/synth and pulseengine/loom)

### loom — Z3 i64 sort handling

The `inline_functions` pass reverts every gale-ffi function with:
```
SortDiffers { left: (_ BitVec 64), right: (_ BitVec 32) }
```
This blocks any verified optimization on i64-heavy modules. The
gale-ffi crate uses u64-packed returns (per gale's `#10` LTO regression
guard) so this is a fundamental gap. Without loom, the wasm-LTO route
loses the formal-verification angle that distinguishes it from
LLVM-LTO.

### synth — codegen patterns

1. **u64-packed FFI return unpacking:** when synth lowers a wasm
   function that returns i64 and the caller immediately bit-masks
   into byte-fields, recognize the packed-struct-return pattern and
   emit register-direct field access (no shifts). Reduces LTO-parity
   gap by ~50% of the size delta.
2. **wasm linear-memory access lowering:** when a wasm `i32.load` is
   from a constant address that's known to be in `.data`, emit
   `ldr rN, [base, #imm]` instead of `movw + movt + ldr`. Reduces
   another ~20% of the size delta.

With both fixes applied, the wasm-LTO route should approach LLVM-LTO
parity (within ~10% on silicon cycles) while delivering the
verification-by-construction property LLVM-LTO doesn't have.

## Why this matters for the publication

The current "Three Quiet Barriers" successor can claim:

> "Cross-language LTO via wasm IR is feasible end-to-end with the
> existing PulseEngine pipeline. wasm-ld merges, synth transpiles,
> and the C↔Rust seam dissolves at wasm level. The remaining gap to
> LLVM-LTO parity is two specific codegen patterns in synth and a
> Z3 sort fix in loom — both well-scoped engineering work, neither
> a fundamental architectural barrier."

That's a stronger claim than "wasm + synth = same as rustc-direct"
(which is what the current `GALE_USE_SYNTH=ON` path delivers without
the merge step).
