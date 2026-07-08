# Driver framework direction — RTIO / io_uring, not a bespoke seam

**Decision (2026-07-08):** the gust driver framework adopts the **submission/
completion-queue (SQ/CQ) model** — Zephyr **RTIO** on the embedded side, Linux
**io_uring** on the host side — rather than inventing a bespoke async-driver API.
The guiding constraint: *don't implement something no one else would use.* SQ/CQ is
the converged state of the art for asynchronous I/O, and — the point that makes this
cheap for us — **gust already independently built every piece of it**; this decision
just names the shape and makes it explicit.

## State of the art (why SQ/CQ is the answer)

**Linux io_uring** is the reference design: two lock-free ring buffers shared across
the boundary — a submission queue of entries (SQEs) that can be chained, and a
completion queue of events (CQEs). Its modern high-performance path is **registered
(fixed) buffers**: memory is registered once, pinned, and reused across operations
for **zero-copy** I/O (`IORING_OP_READ_FIXED`/`WRITE_FIXED`, `SEND_ZC`; Linux 6.15
added zero-copy receive). The governing rule is an **ownership lifecycle**:
"registered buffers must remain valid until all operations using them are complete,
and io_uring provides mechanisms to track this." Measured payoff is real for bulk
transfers (up to ~3.5× efficiency for large messages; overhead-dominated below ~1 KiB).

**Zephyr RTIO** is io_uring brought to embedded — "a framework for doing
asynchronous operation chains with event driven I/O [that] takes a lot of
inspiration from Linux's io_uring … because that API matches up well with hardware
transfer queues and descriptions such as DMA transfer lists." Its pieces:
- **SQ/SQE** — ordered operation descriptions; chained via a bitflag so "the next
  sqe must wait on the current one" (models CS→clock→DMA→disable hardware sequencing,
  with transactional succeed-together/fail-together semantics).
- **CQ/CQE** — "a sqe once completed results in a cqe being pushed into the cq."
- **iodev (IO device) API** — "turning submission queue entries (sqe) into completion
  queue events (cqe) is the job of objects implementing the iodev API"; iodevs batch
  into hardware descriptors (DMA transfer lists).
- **executor** — "a low overhead concurrent I/O task scheduler."

RTIO landed in Zephyr 3.4.0 and "has quickly become the norm for defining new APIs
for asynchronous I/O operations in Zephyr" (I2C, SPI, sensors today). The same SQ/CQ
shape recurs across the industry (Windows IoRing, SPDK, DPDK) — so building to it is
riding the standard, not a detour.

## gust already converged on this — the mapping is 1:1

The reason this is low-cost: gust's existing, **already-verified** primitives *are*
the RTIO/io_uring components. We don't build a framework; we relabel and connect
proven parts.

| RTIO / io_uring concept | gust primitive (already built, already verified) |
|---|---|
| SQ/CQ lock-free rings | **`gale::msgq` ring** — Verus + Rocq + Kani-proven ring buffer |
| Executor (concurrent I/O scheduler) | **kiln-async** — fuel-bounded cooperative executor, Kani-proven |
| iodev (SQE → CQE) | **`gust:hal` thin-seam driver** (UART/GPIO/SPI/timer) — verified-wasm protocol logic dissolved to native |
| Registered/fixed buffer (pinned, ownership-tracked, zero-copy) | **`dma-own` `own<buffer>` handoff** — Kani-proven 6/6 ownership FSM: buffer valid iff owned, returned on completion via `future<own<buffer>>` |
| Chained SQEs / transaction (CS→clock→DMA→disable) | the SPI transfer FSM + Component-Model ownership sequencing |
| CQE completion notification | **kiln waitable / `future<own<buffer>>`** — split-phase, no host `Future` crosses the wasm boundary |

The deepest correspondence is the buffer model: **io_uring's registered-buffer
ownership lifecycle and gust's `own<buffer>` handoff are the same idea** — a buffer
that must stay valid and untouched by the submitter until the engine returns it. gust
expresses it in the **Component Model's ownership type system** (`own<T>`, move
semantics, `future<own<T>>`), which makes the "valid until complete" rule a *type
error to violate* rather than a runtime-tracked convention. That is the gust
differentiator over both io_uring and Zephyr RTIO, which enforce it in C by
discipline: **the same SQ/CQ model, proof-carrying.**

## What this means for the roadmap

- **v0.3.0 driver breadth** — the streaming/DMA-class drivers (**SPI**, later
  sensors, UART-block) present an **RTIO-shaped iodev**: they turn SQEs into CQEs and
  reuse `dma-own` as the registered buffer. Trivial synchronous peripherals (GPIO,
  timer) stay direct register ops — matching RTIO's own scope (it targets I2C/SPI/
  sensors/DMA streaming, not bit-banged GPIO). So SPI is where the SQ/CQ shape first
  appears on the bench.
- **v0.4.0 `gust:os` capability seam** — the I/O capability is an **RTIO/io_uring-
  shaped `submit(sqe)` / `poll-completion() → cqe` WIT interface** over `own<buffer>`,
  not ad-hoc per-driver imports. An app written to it is portable to any node whose
  TCB + iodevs satisfy it, and — because the shape is RTIO — conceptually portable to
  a Zephyr RTIO backend and shape-compatible with a host io_uring backend for
  replay/simulation.
- **Non-bespoke guarantee** — every external touchpoint is a standard: RTIO
  (Zephyr-native), io_uring (Linux SOTA), Component-Model `own<T>` (WASI/BA). gust
  contributes the *verified* implementation, not a new API surface.

## Sources

- [Real Time I/O (RTIO) — Zephyr Project Documentation](https://docs.zephyrproject.org/latest/services/rtio/index.html)
- [RTIO: Or io_uring for Zephyr — Tom Burdick, Intel (Zephyr Dev Summit 2022)](https://static.sched.com/hosted_files/zephyr2022/60/RTIO.pdf)
- [Sensors Async API Support: RTIO-based — zephyr#77099](https://github.com/zephyrproject-rtos/zephyr/issues/77099)
- [io_uring_register(2) — registered buffers, Linux manual page](https://man7.org/linux/man-pages/man2/io_uring_register.2.html)
- [io_uring zero-copy Rx — Linux Kernel documentation](https://docs.kernel.org/networking/iou-zcrx.html)
- [IO_uring Network Zero-Copy Receive Lands In Linux 6.15 — Phoronix](https://www.phoronix.com/news/Linux-6.15-IO_uring)
- [io_uring for High-Performance DBMSs: When and How to Use It (arXiv 2512.04859)](https://arxiv.org/html/2512.04859v2)
