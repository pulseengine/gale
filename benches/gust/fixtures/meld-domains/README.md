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

## Not established here
- What happens to handle tables when **several** components share one domain —
  that is the case meld#427 is generalising for, and it needs per-domain packing
  to exist before it can be measured.
- Anything about MCU lowering: `--pack-rebase` refuses outside shared memory, so
  the two-domains-each-packed shape cannot be built yet.
