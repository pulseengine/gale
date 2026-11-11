# `wit/` — gale's Component Model interfaces

One directory per WIT package, so a `wit_bindgen::generate!` path names exactly
one package and its `deps/`.

| Directory | Package | What it is |
|---|---|---|
| [`kernel/`](kernel/README.md) | `gale:kernel` | the verified kernel primitives (`sem`, `msgq`, `mutex`, `event`) for the primitives / kiln-host mode |
| [`hal/`](hal/gust-hal.wit) | `gust:hal` | the driver capability seam: what a dissolved driver imports (`mmio`, `irq`, `dma`) and exports |
| [`os/`](os/gust-os.wit) | `gust:os` | the OS syscall seam an app imports; `os/deps/sched` holds the node-internal `gust:sched` seam |

Generated per-target worlds are not here: `targets/generated/world-*.wit` are
emitted from the AADL target model and checked against `hal/` by
`cargo xtask check wit-resolve`.

The layout is step 2c of [the repository rework](../docs/architecture/three-modes-two-axes.md).
