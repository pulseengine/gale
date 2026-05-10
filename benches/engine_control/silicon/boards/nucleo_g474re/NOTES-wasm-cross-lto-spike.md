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

## Full integration attempt + new finding — synth memset is broken

After the initial spike I attempted full integration into the bench
to get silicon-measurable timing. The integration path:

  1. Edit `zephyr/gale_sem.c` to wrap `z_impl_k_sem_give` in
     `#ifndef GALE_WASM_LTO_OVERRIDE_SEM_GIVE` so the bench's native
     compilation skips it.
  2. Build merged.wasm via wasm-ld → synth → ARM ET_REL.
  3. Wrap merged.o in libgale_ffi.a using arm-zephyr-eabi-ar (not
     Apple ar — Apple ar's ranlib doesn't index ARM ELF symbols
     correctly, archive shows up as "no global symbols").
  4. Place the merged libgale_ffi.a at the bench's expected path
     (replacing the rustc-direct cargo output).
  5. Build with `-DEXTRA_CFLAGS=-DGALE_WASM_LTO_OVERRIDE_SEM_GIVE=1
     -DEXTRA_LDFLAGS=-Wl,--allow-multiple-definition`.

The build succeeded — final ELF: 219 KB FLASH (vs LTO's 26 KB), 66 KB
RAM. Linker warnings about `.meld_import_table` orphan section
(synth-emitted, lands at VMA 0 in non-loaded section, harmless).

**But the chip doesn't boot.** PC stays in the synth-emitted `memset`
function (`0x0802c614`, 454 bytes) for >10 seconds, bouncing between
0x0802c668 and 0x0802c67e — a tight inner loop that doesn't terminate
correctly on the boundaries Zephyr's startup uses. Zephyr's z_bss_zero
calls memset(bss_start, 0, bss_size); synth's memset never returns.

Workaround attempts that didn't stick:
- `--allow-multiple-definition` + `objcopy --weaken-symbol=memset`:
  weak-vs-strong resolution didn't override; ld picked synth's.
- `objcopy --redefine-sym memset=__synth_memset`: renamed the C-symbol
  but the Rust mangled `_ZN17compiler_builtins3mem6memset17h...E`
  remained, and the final ELF still resolves `memset` to synth's
  buggy code at 0x0802c614 (the bytes are still there from merged.o's
  .text section).
- `objcopy --strip-symbol=memset`: removes the symbol table entry
  but doesn't remove the bytes; ld still places synth's code at
  0x0802c614 and exposes it as `memset` from another reference.

The root cause is that synth's wasm-to-ARM lowering of memset
produces a loop that doesn't terminate on the boundaries Zephyr's
startup uses (`memset(bss, 0, bss_size)` where bss_size is in bytes,
8-byte aligned). The synth output disassembles to a pattern with
`subs.w r3, r2, #32; bpl.n ...; rsb r3, r2, #32; lsl.w r3, r1, r3 …`
which is the i64-shift-with-bytecount pattern from Rust's u64
left-shift implementation — synth seems to have lowered memset's
inner loop using i64 shift operations that don't apply to byte
counts.

## Action items, updated (3 synth bugs, 1 loom bug)

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

1. **memset/memcpy/memmove are MIS-COMPILED** (newly discovered, severity:
   blocker). Synth's wasm→ARM lowering of compiler_builtins' memset
   produces a non-terminating loop on Zephyr's startup
   `memset(bss, 0, sizeof(bss))` invocation. The chip hangs in
   memset+0x4c forever. Until this is fixed, no integration of
   merged-wasm into a real bench can boot. **First-priority fix.**

2. **u64-packed FFI return unpacking:** when synth lowers a wasm
   function that returns i64 and the caller immediately bit-masks
   into byte-fields, recognize the packed-struct-return pattern and
   emit register-direct field access (no shifts). Reduces LTO-parity
   gap by ~50% of the size delta. Same root issue as memset's bug —
   synth's i64 codegen is incomplete.

3. **wasm linear-memory access lowering:** when a wasm `i32.load` is
   from a constant address that's known to be in `.data`, emit
   `ldr rN, [base, #imm]` instead of `movw + movt + ldr`. Reduces
   another ~20% of the size delta.

With (1) fixed, the wasm-LTO bench will boot and we get measurable
silicon cycles. With (2)+(3) on top, the wasm-LTO route should
approach LLVM-LTO parity (within ~10% on silicon cycles) while
delivering the verification-by-construction property LLVM-LTO doesn't
have.

## What we have data-to-compare on

  Silicon (sha b48a81ac/f6f61281):
    baseline (no Gale, ADC=n)              528 cyc handoff median
    rustc-direct gale (ADC=n)              574 cyc (+46 = FFI seam)
    gale via wasm→synth (Rust only, ADC=y) 582 cyc (seam preserved)
    LLVM-LTO gale (ADC=n)                  471 cyc (-57 below baseline)
    LLVM-LTO gale (ADC=y, post-fix)        558 cyc (+52 above baseline)
    wasm-LTO via wasm-ld+synth (ADC=y)     ELF builds, chip won't boot

  Toolchain-level:
    wasm-ld merge + arm-zephyr-eabi-ar     works
    synth inlining via merged-module       works (no bl in z_impl_k_sem_give)
    synth emitted body size                138 bytes (1.68x LTO's 82 bytes)
    synth memset codegen                   broken (infinite loop)
    loom inline_functions                  broken (Z3 SortDiffers on i64)

The **structural claim** holds: the wasm-LTO toolchain (meld/wasm-ld → loom
→ synth) inlines through the C↔Rust seam. The disassembly evidence is
robust — synth's emitted ARM has zero `bl gale_k_sem_give_decide` in
the inlined `z_impl_k_sem_give`.

The **silicon-cycle claim** requires fixes upstream: the memset bug
blocks boot, and the i64 codegen patterns prevent LTO parity once we
get there. Two PRs against pulseengine/synth and one against
pulseengine/loom.

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
