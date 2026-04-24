# Gale

**Build & test**
[![Rust CI](https://github.com/pulseengine/gale/actions/workflows/bazel-tests.yml/badge.svg)](https://github.com/pulseengine/gale/actions/workflows/bazel-tests.yml)
[![Zephyr Tests](https://github.com/pulseengine/gale/actions/workflows/zephyr-tests.yml/badge.svg)](https://github.com/pulseengine/gale/actions/workflows/zephyr-tests.yml)
[![Renode Tests](https://github.com/pulseengine/gale/actions/workflows/renode-tests.yml/badge.svg)](https://github.com/pulseengine/gale/actions/workflows/renode-tests.yml)
[![codecov](https://codecov.io/gh/pulseengine/gale/graph/badge.svg)](https://codecov.io/gh/pulseengine/gale)

**Verification & dynamic analysis**
[![Formal Verification](https://github.com/pulseengine/gale/actions/workflows/formal-verification.yml/badge.svg)](https://github.com/pulseengine/gale/actions/workflows/formal-verification.yml)
[![Sanitizers](https://github.com/pulseengine/gale/actions/workflows/sanitizers.yml/badge.svg)](https://github.com/pulseengine/gale/actions/workflows/sanitizers.yml)
[![Fuzz Smoke](https://github.com/pulseengine/gale/actions/workflows/fuzz-smoke.yml/badge.svg)](https://github.com/pulseengine/gale/actions/workflows/fuzz-smoke.yml)
[![Nightly](https://github.com/pulseengine/gale/actions/workflows/nightly.yml/badge.svg)](https://github.com/pulseengine/gale/actions/workflows/nightly.yml)

Formally verified Rust replacement for Zephyr RTOS kernel primitives. ASIL-D targeted, triple-track verification: Verus (SMT/Z3) + Rocq (theorem proving) + Lean (scheduler/priority proofs).

Part of the [PulseEngine](https://github.com/pulseengine) toolchain.

## Modules

39 Rust modules covering the full Zephyr kernel surface. All 39 pass Verus SMT verification (805 verified, 0 errors):

**Synchronization**

| Module | Zephyr Source | Properties | Status |
|--------|---------------|------------|--------|
| sem | kernel/sem.c | P01-P10 | Verified + Zephyr tested |
| mutex | kernel/mutex.c | M01-M11 | Verified + Zephyr tested |
| condvar | kernel/condvar.c | C01-C08 | Verified + Zephyr tested |
| spinlock | kernel/spinlock | SL01-SL05 | Verified |
| futex | kernel/futex.c | FX01-FX06 | Verified |
| atomic | kernel/atomic.c | AT01-AT06 | Verified |

**IPC**

| Module | Zephyr Source | Properties | Status |
|--------|---------------|------------|--------|
| msgq | kernel/msg_q.c | MQ01-MQ13 | Verified + Zephyr tested |
| stack (k_stack) | kernel/stack.c | SK01-SK09 | Verified + Zephyr tested |
| pipe | kernel/pipe.c | PP01-PP10 | Verified + Zephyr tested |
| mbox | kernel/mailbox.c | MB01-MB06 | Verified |
| fifo | kernel/fifo | FI01-FI06 | Verified |
| lifo | kernel/lifo | LI01-LI06 | Verified |
| queue | kernel/queue.c | QU01-QU06 | Verified |
| ring_buf | sys/ring_buffer | RB01-RB08 | Verified |
| poll | kernel/poll.c | PL01-PL08 | Verified (external_body for array mutation) |

**Timing**

| Module | Zephyr Source | Properties | Status |
|--------|---------------|------------|--------|
| timer | kernel/timer.c | TM01-TM08 | Verified |
| event | kernel/events.c | EV01-EV08 | Verified |
| timeout | kernel/timeout.c | TO01-TO08 | Verified |
| timeslice | kernel/timeslice | TS01-TS06 | Verified |
| work | kernel/work.c | WK01-WK06 | Verified |

**Memory**

| Module | Zephyr Source | Properties | Status |
|--------|---------------|------------|--------|
| mem_slab | kernel/mem_slab.c | MS01-MS08 | Verified |
| heap | lib/os/heap | HP01-HP08 | Verified |
| kheap | kernel/kheap | KH01-KH06 | Verified |
| mempool | kernel/mempool | MP01-MP06 | Verified |
| mem_domain | kernel/mem_domain.c | MD01-MD06 | Verified |
| stack_config | arch/stack | SKS01-SKS05 | Verified |

**Scheduling**

| Module | Zephyr Source | Properties | Status |
|--------|---------------|------------|--------|
| sched | kernel/sched.c | SC01-SC16 | Verified (external_body for array mutation) + Lean proofs |
| thread | kernel/thread.c | TH01-TH06 | Verified |
| thread_lifecycle | kernel/thread.c | TL01-TL06 | Verified |
| priority | kernel/priority | - | Verified + Lean proofs |
| wait_queue | kernel/waitq | - | Verified |
| smp_state | kernel/smp | SM01-SM04 | Verified |

**Safety**

| Module | Zephyr Source | Properties | Status |
|--------|---------------|------------|--------|
| error | kernel/errno | - | Verified |
| fatal | kernel/fatal.c | FT01-FT04 | Verified |
| fault_decode | arch/fault | FD/FH01-FH03 | Verified |
| device_init | drivers/init | DI01-DI05 | Verified |
| dynamic | kernel/dynamic.c | DY01-DY04 | Verified |
| userspace | kernel/userspace.c | US01-US08 | Verified |

## Architecture

```
src/*.rs          Verus-annotated Rust (39 modules, single source of truth)
    |
    +---> Verus verification (39/39 modules, 805 verified, SMT/Z3)
    |
    +---> verus-strip ---> plain/src/*.rs (auto-generated plain Rust)
    |       |                |
    |       |                +---> cargo test (unit + integration + proptest)
    |       |                +---> Kani BMC (185 harnesses)
    |       |                +---> clippy ASIL-D lint profile
    |       |
    |       +--standalone--> plain/*.rs ---> Rocq proofs (9 modules)
    |
    +---> proofs/lean/*.lean ---> Lean 4 proofs (3 files, scheduler + priority)
    |
    +---> ffi/ ---> C shim ---> Zephyr kernel (qemu_cortex_m3/m4f/m33)
```

## Verification

### Formal verification (via Bazel + Nix, CI-gated on source changes):

- **Verus (SMT/Z3):** 39/39 modules, 805 properties verified by Z3 (0 errors). Includes poll and sched with `external_body` trusted array helpers.
- **Rocq:** 9 abstract invariant proofs over Z-valued math (0 Admitted). NOT connected to the Rust code — proofs reason about hand-written mathematical models, not the rocq-of-rust translation.
- **Lean 4:** 3 mathematical proofs — RMA bound, priority ceiling protocol, priority queue ordering. Pure scheduling theory, not implementation proofs.
- **Kani BMC:** 185 bounded model checking harnesses (87 model + 98 FFI). Not in CI until Bazel workflow is confirmed working.

### Functional testing (CI-enforced on every commit):

- **cargo test:** ~1015 runtime tests on stripped (non-Verus) code
- **Zephyr integration:** 36 upstream test suites on QEMU (M3 + MPS2/AN385 with MPU)
- **Renode emulation:** 3 boards (Cortex-M4F, M33, R5)
- **Coverage:** Rust + Zephyr C line coverage → Codecov

> **Honesty note:** Formal verification requires Bazel + Nix and runs via the Formal Verification
> workflow (triggered on src/proofs/ffi changes + weekly cron). Functional tests run on every commit.
> See [verification honesty assessment](docs/safety/verification-honesty.md) for the full gap analysis.
- **Kani BMC:** 185 bounded model checking harnesses (87 model + 98 FFI, Bazel-only)
- **Differential testing:** POSIX/FreeRTOS reference models validate spec independence
- **Property-based testing:** Proptest with random operation sequences
- **Fuzz testing:** Coverage-guided mutation via cargo-fuzz
- **Miri:** Undefined behavior detection

## CI

3 workflows:

| Workflow | Scope | Status |
|----------|-------|--------|
| Rust CI | cargo test, clippy, verus-strip gate | 995 tests |
| Zephyr Kernel Tests | 20 upstream test suites on qemu_cortex_m3 | 20/20 pass |
| Renode Emulation Tests | Cortex-M4F (STM32F4) + Cortex-M33 (STM32L552) + Cortex-R5 (ZynqMP) | 3 boards |

## Traceability

ASPICE V-model traceability managed by [Rivet](https://github.com/pulseengine/rivet):

```
rivet validate    # PASS (33 warnings — see Known Gaps)
rivet coverage    # 97.3% weighted coverage
rivet stats       # 660 artifacts
```

## Known Gaps

`rivet validate` produces 33 warnings and 277 lifecycle coverage gaps. All are categorized below.

**Expected -- KILN Phase 2 (8 warnings)**

SWREQ-KILN-001 through 006 and SYSREQ-KILN-001/002 are draft requirements for the Kiln build-system integration phase. No implementation exists yet; these warnings are expected and will resolve when Phase 2 begins.

**Lifecycle -- verification measures link to SWDD/SWARCH, not individual SWREQs (25 warnings + 277 gaps)**

The remaining 25 warnings are system-level requirements (SYSREQ-*) for newer modules (fifo, lifo, queue, mempool, futex, timeout, poll, scheduler, timeslice, kheap, thread_lifecycle, heap, mem_domain, stack_config, device_init, fault_decode, ring_buf, userspace, work, fatal, dynamic, smp_state, atomic, mbox) that lack `sys-verification` and/or `sys-integration-verification` artifacts. This is a design choice: verification measures are linked at the SWDD and SWARCH levels, and roll up via traceability rather than duplicating at every requirement.

The 277 lifecycle gaps break down as:
- **243 SWREQ gaps:** All missing `unit-verification` and `sw-integration-verification` direct links. These SWREQs *are* verified (97.6% swe1-has-verification coverage) but through SWDD-level verification measures, not direct SWREQ-to-test links.
- **34 SYSREQ gaps:** 9 have only `sys-integration-verification` missing (sem, mutex, condvar, event, msgq, stack, pipe, mem_slab, timer -- the original 9 primitives with Zephyr tests). The other 25 are missing both `sys-verification` and `sys-integration-verification` (the newer Phase 2 modules).

**Legitimate gaps: 0.** All warnings are either expected (KILN Phase 2) or are a consequence of the lifecycle architecture where verification measures attach to design artifacts rather than being duplicated per-requirement.

**Orphan artifacts: 2** (ZEP-SRS-5-3, PROV-SEM-002) -- known out-of-scope Zephyr upstream items.

## Build

```bash
# Rust tests (995 tests)
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
