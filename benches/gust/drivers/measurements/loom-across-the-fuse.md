# Does loom optimize more once it sees across the fuse? Measured: no — it costs 170 B

Intuition says fusing first should help: after `meld fuse` the five provider
components are one core module, so loom can inline and fold across boundaries it
could never see when they were separate components. Measured on the real
five-provider gust:os composition, that is not what happens.

All three arms end with the identical
`synth compile --target cortex-m3 --all-exports --relocatable`:

| arm | synth `.text` |
|---|---|
| fuse, **no loom at all** | **4642 B** |
| `loom optimize --passes inline` **per component**, then fuse | **4642 B** |
| fuse, then `loom optimize --passes inline` (**the shipped order**) | **4812 B** |

On the wasm itself the honest figure is the **code section**, not the file:

| section | fused core | after loom |
|---|---|---|
| **code** | **4194 B** | **4612 B**  (+418 B, +10.0%) |
| `wsc.transformation.attestation` | — | 1498 + 881 B |
| `component-provenance` | — | 1931 B |
| `wsc.facts` | absent | **still absent** |

**Do not quote the file size.** 9874 → 10832 B (+958 B) is what `wc -c` reports,
and it conflates a 418 B code increase with ~4.3 KB of newly-added attestation and
provenance metadata (offset by dropped `producers` and `.debug_*` sections). The
code section is the axis that propagates to the native object, and it is the one
that grew.

Note also that loom emitted **no `wsc.facts` section at all** on this input —
consistent with the fact producer not being wired. So none of this size change is
the proof channel; it is inlining plus bookkeeping.

## Reading it

**Loom does see across.** Per-component it shrinks each provider ~20%
(`time-provider` 2518 → 1982 B, `exec-provider` 10246 → 9308 B). Run after the
fuse, the same `inline` pass now has the whole composite in scope — and inlines
enough to grow the module.

**Two findings, not one.**

1. **Fuse-then-loom is a net size regression here: +170 B of `.text` (+3.7%).**
   The shipped pipeline runs in exactly this order.
2. **Per-component optimization is washed out by fusion.** Optimizing each
   component first produces a final object byte-for-byte the same size as not
   optimizing at all — meld's index-space merge and adapter generation
   renormalize whatever loom did upstream.

**What this does NOT say.** Only size was measured. Inlining classically trades
size for speed, so the +170 B may buy cycles; no cycle measurement was taken, and
`gust_codegen_bench` would be the way to take one. It also says nothing about
other pass sets — only `--passes inline` was exercised, because that is what the
gust dissolve scripts use.

## Why it matters beyond gust

The whole-program argument for build-time composition is that the optimizer gets
a scope no separate-compilation toolchain has. That argument is sound, and this
measurement does not refute it — but it shows the argument is not automatically
cashed by running the existing pass at the new scope. A pass tuned for
single-component modules can be actively counterproductive once the scope is the
whole OS. Worth raising with loom as a pass-ordering / cost-model question.

## Reproduce

    OUT=/tmp/gustos bash benches/gust/drivers/build-fused-gustos.sh
    cd /tmp/gustos
    meld fuse os.wasm --memory shared -o core.wasm
    synth compile core.wasm --target cortex-m3 --all-exports --relocatable -o noloom.o
    loom optimize --passes inline core.wasm -o opt.wasm
    synth compile opt.wasm  --target cortex-m3 --all-exports --relocatable -o loom.o
    arm-zephyr-eabi-size noloom.o loom.o
