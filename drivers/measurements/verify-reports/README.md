# `synth-verify-v1` fixtures — the pinned denominator for T2

Three committed `--emit-verify-report` sidecars, one per thin-seam driver. They
exist so `REQ-OS-OBJVERIFY-001`'s union denominator is **pinned against drift in
`--emit-verify-report` itself**, which a reproduction script cannot do. Requested
by the synth maintainer on synth#1057.

## Exact provenance — every stage matters

    cargo build --release --target wasm32-unknown-unknown   (CARGO_PROFILE_RELEASE_DEBUG=2)
      -> wasm-tools component new
      -> wac plug <seam stub>            (mpu-thin, switch-thin only; hm-thin has no imports)
      -> meld  0.41.3  fuse --memory shared --preserve-names
      -> loom  1.2.0   optimize --passes inline --attestation false
      -> synth 0.57.0  compile --target cortex-m3 --all-exports --relocatable
                               --native-pointer-abi --shadow-stack-size 2048
      -> synth 0.57.0  verify <loom.wasm> <obj.o> --emit-verify-report

`synth` must be a `--features verify` build; released binaries error at runtime.

## Two things that are NOT obvious, and that cost a re-measurement to learn

**The `loom optimize --passes inline` stage is load-bearing for this number.**
Running `synth verify` on the meld-fused `core.wasm` — skipping loom — gives
**139** applied rule instances across the three drivers. Running it on the
loom-inlined `loom.wasm` gives **496**. Inlining duplicates callee bodies, so the
rule inventory is a property of the *optimized* module, not the source one. A
denominator quoted without naming the loom stage is not reproducible.

**The rule inventory is a property of the WASM, not the object.** Recompiling
`switch-thin` with and without `--native-pointer-abi` / `--shadow-stack-size`
yields byte-different objects and an **identical** 159-instance inventory. So the
compile flags above are recorded for completeness, not because they move the
number.

## Known non-reproducibility — stated rather than smoothed over

`hm-thin` reproduces its previously reported count **exactly** (190 applied / 13
verified / 177 declined). The two drivers that need a **seam stub** do not:

| driver | this fixture | earlier report | delta |
|---|---|---|---|
| `hm-thin` (no stub) | 190 | 190 | 0 |
| `mpu-thin` (`regs-stub`) | 147 | 138 | +9 |
| `switch-thin` (`ctx-stub`) | 159 | 149 | +10 |
| **total** | **496** | 477 | +19 |

`verified` reproduces exactly on all three (13 / 7 / 2 = **22**), so the SMT half
is stable; the drift is entirely in `declined`.

The stubs are the differing input: they are rebuilt from source by whatever
`rustc` is current, and **nothing pins them**. That is precisely the argument for
committing these fixtures. The earlier figure was not wrong when taken — it is
not reproducible, which is a different and more useful thing to know.
