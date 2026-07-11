# gust-dma-own — DMA as `own<buffer>` handoff (gale#124)

DMA modeled as a Component-Model **ownership round-trip**: `own<dma-buffer>` moves
from wasm to the DMA engine on transfer and returns on completion. The ownership
state machine — the part that decides who may touch the buffer — is **verified
wasm**, dissolved to native; the trusted surface is the irreducible atoms
(descriptor poke, barrier op, IRQ poll).

## Verified core (Kani, `cargo kani`)

**6 harnesses, 0 failures** (73 obligations discharged):

| proof | property |
|---|---|
| `p1_access_iff_wasm_owned` | wasm may touch the buffer **iff** wasm-owned — hands-off during the DMA window |
| `p2_barrier_pairing` | every ownership handoff carries the correct coherency op **on every path** (clean+DSB on Wasm→Dma, invalidate+DMB on Dma→Wasm) — impossible to transfer without the barrier |
| `p3_abort_never_ownerless` | abort from **any** state returns the buffer to a defined owner (never a limbo/gap) |
| `p4_round_trip` | start→complete returns to wasm-owned; double-start / unpaired-complete are faults, not silent corruption |
| `p5_ring_per_chunk_exclusive` | streaming/circular: arm/reap flip exactly one chunk's owner; each chunk owned by exactly one side; access tracks ownership per chunk |
| `p6_ring_reap_and_abort` | reap returns one chunk with the read barrier; abort_all returns the whole ring to wasm |

The safety property (`p1`/`p2`) is the whole point: wasm is **provably** hands-off
for the transfer window, and the coherency barrier is paired **by construction**
(the transition function is the only way to move ownership and always returns the
barrier).

## Dissolved object (loom-free direct, synth 0.17.0 → cortex-m3)

`synth compile gust_dma_own.wasm --target cortex-m3 --all-exports --relocatable`:

| primitive | `.text` |
|---|---|
| `dma_start` | 96 B |
| `dma_poll_complete` | 92 B |
| `dma_abort` | 30 B |
| **total** | **218 B** |

- **SRAM:** 0 B `.bss` / `.data` — state lives in the caller (the kiln task); the
  primitives are pure transitions over a `u32`-encoded owner. No linmem data → no
  `--native-pointer-abi` / r11 trampoline (same as uart-thin).
- **TCB = 3 import relocations:** `dma_program` (descriptor poke), `dma_barrier`
  (cache/DSB-DMB op), `dma_irq_poll` (completion IRQ). That is the entire trusted
  native surface for DMA — nothing above the HAL line.

## TCB statement (what remains trusted)

The CM type system + the Kani-proven FSM protect the buffer **from our code**
(wasm cannot touch it mid-transfer). The shim + silicon protect the rest: the
`dma_program` shim must program the descriptor within the region (the engine is
not constrained by the handle), and the barrier op must be correct for the target.
Keep the descriptor-programming shim minimal and audited. On M3 the barrier is a
no-op (no cache); the abstraction is present so it is correct on M7/A-class.

## Async integration

`dma_poll_complete` wraps `dma_irq_poll` in the split-phase pattern: not-fired →
returns state unchanged (kiln yields); fired → completes the transfer and re-owns.
This keeps DMA inside the fuel-bounded cooperative poll model — the completion IRQ
wakes the kiln waitable that resolves the `future<own<dma-buffer>>`. No new trusted
surface beyond the existing IRQ shim; no preemption reintroduced.

## Reproduce

```sh
cd benches/gust/drivers/dma-own
cargo kani                                   # 6/6 proofs
cargo build --release --target wasm32-unknown-unknown
synth compile target/wasm32-unknown-unknown/release/gust_dma_own.wasm \
  --target cortex-m3 --all-exports --relocatable -o dma-own-cm3.o
```

## Follow-ons (tracked, not in this increment)

- **Worked example on silicon** (SPI/ADC DMA round-trip, byte-identical non-DMA
  portions): needs a Renode DMA-controller model or a board with a probe.
- **synth shared-segment region marking:** synth must treat the DMA-owned window
  as externally-mutable (no caching loads across the handoff) — the codegen signal
  the ownership handle provides. Filed as synth#543 (related: synth#390 / loom#226).
