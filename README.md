# Gale

Formally verified Rust port of Zephyr RTOS kernel primitives. ASIL-D targeted, dual-track verification: Verus (SMT/Z3) + Rocq (theorem proving).

Part of the [PulseEngine](https://github.com/pulseengine) toolchain.

## Primitives

| Primitive | Properties | Zephyr Source | Status |
|-----------|-----------|---------------|--------|
| Semaphore | P1-P10 | kernel/sem.c | Verified + Zephyr tested |
| Mutex | M1-M11 | kernel/mutex.c | Verified + Zephyr tested |
| CondVar | C1-C8 | kernel/condvar.c | Verified + Zephyr tested |
| MsgQ | MQ1-MQ13 | kernel/msg_q.c | Verified + Zephyr tested |
| Stack | SK1-SK9 | kernel/stack.c | Verified + Zephyr tested |
| Pipe | PP1-PP10 | kernel/pipe.c | Verified + Zephyr tested |
| Timer | TM1-TM8 | kernel/timer.c | Verified |
| Event | EV1-EV8 | kernel/events.c | Verified |
| Mem Slab | MS1-MS8 | kernel/mem_slab.c | Verified |

## Architecture

```
src/*.rs          Verus-annotated Rust (single source of truth)
    │
    ├──► Verus verification (SMT/Z3)
    │
    ├──► verus-strip ──► plain/src/*.rs (auto-generated plain Rust)
    │                        │
    │                        ├──► cargo test (unit + integration + proptest)
    │                        ├──► Kani BMC (bounded model checking)
    │                        ├──► Rocq proofs (theorem proving)
    │                        └──► clippy ASIL-D lint profile
    │
    └──► ffi/ ──► C shim ──► Zephyr kernel (qemu_cortex_m3)
```

## Verification

- **Verus (SMT/Z3):** All properties formally proven via requires/ensures contracts
- **Rocq (theorem proving):** Independent proof track via coq_of_rust translation
- **Kani BMC:** Bounded model checking for C↔Rust semantic equivalence
- **Differential testing:** POSIX/FreeRTOS reference models validate spec independence
- **Property-based testing:** Proptest with random operation sequences
- **Fuzz testing:** Coverage-guided mutation via cargo-fuzz
- **Miri:** Undefined behavior detection

## Traceability

ASPICE V-model traceability managed by [Rivet](https://github.com/pulseengine/rivet):

```
rivet validate    # 0 errors
rivet coverage    # 97%+ weighted coverage
rivet stats       # 268 artifacts
```

## Build

```bash
# Rust tests
cargo test

# Verus verification
bazel test //:verus_test

# verus-strip gate (plain/ sync check)
cargo test --manifest-path tools/verus-strip/Cargo.toml --test gate

# Zephyr integration (requires west + Zephyr SDK)
source .venv/bin/activate
export ZEPHYR_BASE=/path/to/zephyr
west build -b qemu_cortex_m3 zephyr/tests/kernel/semaphore/semaphore \
  -- -DZEPHYR_EXTRA_MODULES=$(pwd) -DOVERLAY_CONFIG=$(pwd)/zephyr/gale_overlay.conf
west build -t run
```

## License

Apache-2.0
