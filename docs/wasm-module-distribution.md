# Shipping gale's WebAssembly modules — release artifacts + Zephyr integration

**Status:** design + first implementation (sem). **Drives:** release v0.1.0 (sem) per
`docs/release-plan.md`. **Proven on:** NUCLEO-G474RE silicon (engine_control bench),
qemu_riscv32 (icount lane).

## Why ship the wasm modules

The wasm-cross-LTO pipeline (`clang → wasm-ld → loom (seam-dissolve) → synth → ET_REL`)
produces, per kernel primitive, a **target-independent verification artifact** (the
dissolved `.wasm` — what witness MC/DC and the wasmtime oracle run against) and
**target-specific relocatable objects** (Thumb-2 / RV32). Today only our benches consume
them. Shipping both with each release lets a downstream Zephyr+gale user:

1. **Audit** the exact module their firmware embeds (the .wasm is the unit of
   verification evidence — witness truth tables and the functional oracle bind to it).
2. **Link** the prebuilt object without owning the loom/synth toolchain.
3. **Reproduce** the object from the .wasm when they do own the toolchain
   (`loom optimize --passes inline` + `synth compile` — pinned versions in the manifest).

## Release artifacts (per primitive, starting with `sem`)

```
gale-wasm-sem-<ver>.wasm              # loom-dissolved module (verification artifact)
gale-wasm-sem-<ver>.wat               # readable form (review/diff)
gale-wasm-sem-<ver>-cortex-m4f.o      # synth ET_REL, import syms renamed gale_w_*
gale-wasm-sem-<ver>-rv32imac.o        # when the RV32 lane ships (synth#312 fixed in v0.11.37)
gale-wasm-manifest-<ver>.json         # sha256s + pinned toolchain (clang/wasm-ld/loom/synth) + flags
gale-wasm-manifest-<ver>.json.sig     # sigil attestation over the manifest
```

The manifest is the trust anchor: every object lists the exact tool versions and the
sha256 of the .wasm it was compiled from. `sigil verify` must accept the chain before a
safety build consumes the artifacts (skipping it is a traceability-gate finding at
release time).

## Zephyr integration — Kconfig + CMake (this commit)

Downstream `prj.conf`:

```
CONFIG_GALE_KERNEL_SEM=y
CONFIG_GALE_WASM_LTO_SEM=y
```

and either of:

```
west build ... -- -DGALE_WASM_LTO_OBJ_DIR=/path/to/release/assets     # prebuilt objects
west build ... -- -DGALE_WASM_LTO_SEM_OBJ=/path/to/gale-wasm-sem-cortex-m4f.o
```

What the hook does (see `zephyr/CMakeLists.txt`, ported from the silicon-validated
bench integration):

- excludes the native `z_impl_k_sem_give` (compile-guard `GALE_WASM_LTO_OVERRIDE_SEM_GIVE`),
- links the prebuilt object + the `r11=0` trampoline (`zephyr/wasm/gale_wasm_sem_tramp.S`)
  that bridges AAPCS callers to synth's linear-memory-base addressing,
- adds the out-of-line kernel-API wrappers (`zephyr/wasm/gale_wasm_wrappers.c`) for the
  `static inline` APIs the dissolved object imports (`gale_w_*`).

Scope note: the **give** hot path is the shipped surface today (the silicon-measured
907-cyc handoff path, re-baseline pending synth#311-fix validation); take/init stay
native. The same pattern extends per-primitive (`GALE_WASM_LTO_MUTEX` next, gated on
synth#237/v0.11.37 validation).

## Release flow

`release-wasm.yml` builds the artifacts on every `v*` tag from pinned tool versions,
writes the manifest, signs it (sigil; key wiring TODO until the sigil release flow
lands), and attaches everything to the GitHub release. Local equivalent:
`scripts/build-wasm-dist.sh <outdir>`.

rivet: the v0.1.0 release scope (`release-v0.1.0` tag) gains a distribution requirement
linked to the testbed verification (`run_testbed.sh` exit 0 = the functional+codegen
oracle for the shipped module); the witness MC/DC truth-table artifact binds to the
shipped `.wasm` once the witness lane runs in CI.

## Falsifiable claims (release-notes statement)

1. The shipped `.o` byte-matches `synth compile` of the shipped `.wasm` at the manifest's
   pinned versions (verifiable by any consumer; CI re-derives before attach).
2. The shipped `.wasm` passes the in-repo testbed oracle (wasmtime vectors + unicorn
   funccheck lanes).
3. A Zephyr build with `CONFIG_GALE_WASM_LTO_SEM=y` + the shipped object is functionally
   equivalent to the native-FFI build under the kernel semaphore test suite.
