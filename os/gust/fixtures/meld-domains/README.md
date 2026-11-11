# meld#427 fixture — a capability boundary that carries a resource handle

meld asked for "a handle-carrying fixture" before generalising its single
`SharedMemoryPlan` into per-domain packing, because handle tables are built per
**component** today and "domain" and "component" stop being interchangeable once
several components share a domain. This is that fixture, from gale's actual
direction rather than synthesised: it is shaped after `gust:os/spawn` +
`gust:os/timer`, which gale intends to move from bare `u32` handles to resources
because a `u32` handle is forgeable and a per-instance handle table is not
(gale#408, and the STPA-Sec finding that "tenant A sleeps tenant B's task" is
currently unpreventable).

`./build.sh` — builds both components and fuses them both ways.

## What it contains
- **provider** exports `gale:capfix/caps` with `resource task` (constructor,
  `tick` on an owned handle, `peek` as a borrow) plus a scalar `now`, so one
  boundary carries `own<>`, `borrow<>` and a scalar together.
- **consumer** imports it and calls all three, i.e. the tenant side that holds a
  handle across the boundary.

## Measured (meld 0.55.1, gale's pinned layer)

| | boundaries | memories in output | size |
|---|---|---|---|
| `--memory multi` | 4 × `memory-copy`, cross-memory | **2** | 4668 B |
| `--memory shared --address-rebase` | 4 × `direct`, `inlined-direct`, same-memory, **"4 wired with nothing interposed"** | **1** | 4972 B |

The handle ops are not special-cased by the erasure: the resource
**constructor and both methods** become `direct` / `inlined-direct` under shared
memory, exactly like the scalar `now`. So fusing a tenant with its supervisor
removes the copy *and* the interposition on handle operations — which is the
property gale wants preserved at a privilege boundary.

**A second finding, unplanned:** meld REFUSES the shared path outright for
components built without relocation metadata —
*"component 'provider.comp.wasm' module 0 is placed at a non-zero shared-memory
base but carries no relocation metadata … its absolute addresses cannot be
rebased safely"*. `build.sh` therefore passes `-C link-arg=--emit-relocs` on the
final link. A consumer that does not is pushed toward the boundary-preserving
path by default, which is a good failure direction.

## Three components (2026-09-30) — the case meld#427 asked for

meld established that handle tables are allocated only for components that
**re-export** a resource interface, so the two-component fixture above allocated
zero of them and the claim *"components sharing a memory domain have mutually
addressable handle tables"* was read off allocation code rather than observed.
`middle/` is that missing component: it imports `caps`, **re-exports** `caps`
(each `Task` owning an imported handle and forwarding to it), and holds a handle
of its own, so a table must exist and be populated.

Measured on **meld 0.55.1**, the version gale pins:

| | boundaries | memories | size |
|---|---|---|---|
| `--memory multi` | 8 × `memory-copy`, cross-memory, **0** interposition-free | **3** | 7588 B |
| `--memory shared --address-rebase` | 8 × `direct`/`inlined-direct`, same-memory, **8 wired with nothing interposed** | **1** | 7454 B |

Three memories under `multi` — one per component, i.e. three domains — collapsing
to one under `shared`. The handle operations erase exactly like the scalar `now`,
as in the two-component case; adding a re-exporter did not create a special case.

### The finding that was not planned, and it is structural

meld warns that `middle.comp.wasm` and `provider.comp.wasm` **carry no relocation
metadata**, so under shared memory they "may silently alias another component's
memory" (#326/#339). `consumer.comp.wasm` is not flagged. All three are built by
`build.sh` with `-C link-arg=--emit-relocs`, and the relocs *do* survive
componentization — `provider.comp.wasm` carries `reloc.CODE`, `reloc.DATA` and
`linking`.

The explanation is meld's own caveat, which is easy to read past: *"Names are of
FUSED components, which include ones synthesized from an input's nested
structure."* Each component that **exports a resource** contains **three** core
modules — the crate's own, plus two synthesized by `wasm-tools component new`
(the dtor/realloc shims) — and only the first carries relocations:

| component | core modules | exports a resource | flagged |
|---|---|---|---|
| `consumer.comp.wasm` | 1 | no | no |
| `middle.comp.wasm` | 3 | yes | **yes** |
| `provider.comp.wasm` | 3 | yes | **yes** |

So `--emit-relocs` on the final link is **not sufficient**, and the earlier note
in this file saying it addresses the warning is incomplete. The modules without
relocations are ones the toolchain synthesizes, not ones the build controls.

**Why this matters beyond the fixture:** the components affected are exactly the
ones that *carry handles*. The shared-memory path — the one that erases the
boundary — is flagged unsafe precisely for handle-carrying components, which is
the case gale's syscall seam depends on (gale#408). That is an argument for
keeping the boundary rather than a reason to work around the warning.

Correlation is over three components with a plausible mechanism, not an
established cause; meld owns the question of whether the synthesized modules
could carry relocations.

## Not established here
- Whether two components sharing a domain can in fact address each other's handle
  **tables**. The tables are allocated for the re-exporter, but reading one from
  the other requires an executable experiment, not an inspection of the artifact.
- Anything about MCU lowering: `--pack-rebase` refuses outside shared memory, so
  the two-domains-each-packed shape cannot be built yet.
