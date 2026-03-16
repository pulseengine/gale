# ASPICE Traceability Completion Design

**Date:** 2026-03-14
**Goal:** Ground-truth and complete ASPICE V-model artifacts for all 6 kernel primitives.

## Problem

Semaphore has full ASPICE traceability (PROV, SWARCH, SWDD, UV, IV, FV) but 5 drift issues.
Mutex, condvar, msgq, stack, pipe have only SYSREQ + SWREQ — missing all right-side V-model artifacts.
No system verification (SYS.5) measures exist for any primitive.

## Scope

### Part 1: Fix semaphore drift

| Artifact | Issue | Fix |
|----------|-------|-----|
| SWARCH-SEM-001 | `try_take() -> i32` | Change to `-> TakeResult` |
| SWARCH-SEM-001 | `reset() -> usize` | Change to `-> u32` |
| SWARCH-WQ-001 | `unpend_all(i32) -> usize` | Change to `-> u32` |
| SWARCH-WQ-001 | `len() -> usize` | Change to `-> u32` |
| UV-SEM-001 | "25 unit tests" | Change to "20 unit tests (15 in sem.rs + 5 in wait_queue.rs)" |

### Part 2: Provenance records

One per upstream C file being replaced. All SHA256 verified against live files on 2026-03-14.

| ID | Upstream File | SHA256 | Lines | Gale File |
|----|---------------|--------|-------|-----------|
| PROV-MUT-001 | kernel/mutex.c | `4ae6ed3099b536c54186d8358ec17e04ba63ef16eb751af2663c832d6424de74` | 337 | plain/src/mutex.rs |
| PROV-CV-001 | kernel/condvar.c | `a2821164552bcc8bd2dd7b89c018cb223647c74570ff0b857bdc221e19bc18f2` | 171 | plain/src/condvar.rs |
| PROV-MQ-001 | kernel/msg_q.c | `4531c7655786d6f6b7e5b95bb4aaf91682606efb8fef3feedf24b4e8ac7fb6ed` | 516 | plain/src/msgq.rs |
| PROV-SK-001 | kernel/stack.c | `0e23742673ad9ec6b403af6971a0aa6a99ac046e3cfeb9a7ce1ffa093499354c` | 222 | plain/src/stack.rs |
| PROV-PP-001 | kernel/pipe.c | `af7da7150bdfe28318a89ebe840068cb04904725ea615c4a040e1578b596aba8` | 358 | plain/src/pipe.rs |

### Part 3: Software architecture (SWE.2)

One SWARCH per primitive. Interfaces taken from exact function signatures in plain/ source.

**SWARCH-MUT-001** — Mutex module (gale::mutex)
- Struct: `Mutex { wait_q: WaitQueue, owner: Option<ThreadId>, lock_count: u32 }`
- Provided: `init() -> Self`, `try_lock(ThreadId) -> LockResult`, `lock_blocking(Thread) -> bool`, `unlock(ThreadId) -> Result<UnlockResult, i32>`, `is_locked() -> bool`, `lock_count_get() -> u32`, `owner_get() -> Option<ThreadId>`, `num_waiters() -> u32`
- Required: `gale::wait_queue::WaitQueue`, `gale::thread::{Thread, ThreadId}`
- allocated-from: SWREQ-MUT-M01, SWREQ-MUT-M03, SWREQ-MUT-M05
- replaces: PROV-MUT-001

**SWARCH-CV-001** — CondVar module (gale::condvar)
- Struct: `CondVar { wait_q: WaitQueue }`
- Provided: `init() -> Self`, `signal() -> SignalResult`, `broadcast() -> u32`, `wait_blocking(Thread) -> bool`, `num_waiters() -> u32`, `has_waiters() -> bool`
- Required: `gale::wait_queue::WaitQueue`, `gale::thread::Thread`
- allocated-from: SWREQ-CV-C01, SWREQ-CV-C02, SWREQ-CV-C04
- replaces: PROV-CV-001

**SWARCH-MQ-001** — MsgQ module (gale::msgq)
- Struct: `MsgQ { msg_size: u32, max_msgs: u32, read_idx: u32, write_idx: u32, used_msgs: u32 }`
- Provided: `init(u32,u32) -> Result<Self,i32>`, `put() -> Result<u32,i32>`, `put_front() -> Result<u32,i32>`, `get() -> Result<u32,i32>`, `peek_at(u32) -> Result<u32,i32>`, `purge() -> u32`, `num_free_get() -> u32`, `num_used_get() -> u32`, `msg_size_get() -> u32`, `max_msgs_get() -> u32`, `is_full() -> bool`, `is_empty() -> bool`, `read_idx_get() -> u32`, `write_idx_get() -> u32`
- Required: (standalone — ring buffer index model only, no WaitQueue)
- allocated-from: SWREQ-MQ-MQ01, SWREQ-MQ-MQ02, SWREQ-MQ-MQ05
- replaces: PROV-MQ-001

**SWARCH-SK-001** — Stack module (gale::stack)
- Struct: `Stack { capacity: u32, count: u32 }`
- Provided: `init(u32) -> Result<Self,i32>`, `push() -> i32`, `pop() -> i32`, `num_free() -> u32`, `num_used() -> u32`, `is_full() -> bool`, `is_empty() -> bool`, `capacity() -> u32`
- Required: (standalone — bounded counter model)
- allocated-from: SWREQ-SK-SK01, SWREQ-SK-SK02, SWREQ-SK-SK03
- replaces: PROV-SK-001

**SWARCH-PP-001** — Pipe module (gale::pipe)
- Struct: `Pipe { size: u32, used: u32, flags: u8 }` with `FLAG_OPEN=1, FLAG_RESET=2`
- Provided: `init(u32) -> Result<Self,i32>`, `write_check(u32) -> Result<u32,i32>`, `read_check(u32) -> Result<u32,i32>`, `reset()`, `close()`, `clear_reset()`, `space_get() -> u32`, `data_get() -> u32`, `is_empty() -> bool`, `is_full() -> bool`, `is_open() -> bool`, `is_resetting() -> bool`, `size() -> u32`
- Required: (standalone — state machine + byte count model)
- allocated-from: SWREQ-PP-PP01, SWREQ-PP-PP02, SWREQ-PP-PP03
- replaces: PROV-PP-001

### Part 4: Detailed design (SWE.3)

One SWDD per key operation. Algorithm text derived from reading plain/ source.
Each SWDD has `refines` link to its SWARCH and `satisfies` links to the SWREQs it implements.

**Mutex (4 SWDD):**
- SWDD-MUT-INIT: `Mutex::init` — empty WaitQueue, owner=None, lock_count=0. Refines SWARCH-MUT-001. Satisfies M01.
- SWDD-MUT-LOCK: `Mutex::try_lock` — if unlocked: acquire (owner=id, lock_count=1); if owner matches: reentrant (lock_count+=1); else: WouldBlock. Refines SWARCH-MUT-001. Satisfies M03, M04, M05.
- SWDD-MUT-LOCK-BLOCKING: `Mutex::lock_blocking` — block thread, insert into wait queue. Refines SWARCH-MUT-001. Satisfies M05, M11.
- SWDD-MUT-UNLOCK: `Mutex::unlock` — if not owner: EPERM; if not locked: EINVAL; if lock_count>1: Released (decrement); if waiters: Transferred (ownership transfer); else: Unlocked (owner=None, lock_count=0). Refines SWARCH-MUT-001. Satisfies M06, M07, M08, M09, M10.

**CondVar (4 SWDD):**
- SWDD-CV-INIT: `CondVar::init` — empty WaitQueue. Refines SWARCH-CV-001. Satisfies C01.
- SWDD-CV-SIGNAL: `CondVar::signal` — unpend_first from wait_q, or Empty if none. Refines SWARCH-CV-001. Satisfies C02, C03, C07.
- SWDD-CV-BROADCAST: `CondVar::broadcast` — unpend_all, return count. Refines SWARCH-CV-001. Satisfies C04, C05.
- SWDD-CV-WAIT: `CondVar::wait_blocking` — block thread, insert into wait queue. Refines SWARCH-CV-001. Satisfies C06, C08.

**MsgQ (6 SWDD):**
- SWDD-MQ-INIT: `MsgQ::init` — validate msg_size>0, max_msgs>0, no overflow; set indices to 0. Refines SWARCH-MQ-001. Satisfies MQ01.
- SWDD-MQ-PUT: `MsgQ::put` — if not full: return write_idx, advance write_idx = (write_idx+1)%max_msgs, used_msgs+=1. Refines SWARCH-MQ-001. Satisfies MQ02, MQ03, MQ06.
- SWDD-MQ-PUT-FRONT: `MsgQ::put_front` — if not full: retreat read_idx = (read_idx+max_msgs-1)%max_msgs, used_msgs+=1. Refines SWARCH-MQ-001. Satisfies MQ04.
- SWDD-MQ-GET: `MsgQ::get` — if not empty: return read_idx, advance read_idx = (read_idx+1)%max_msgs, used_msgs-=1. Refines SWARCH-MQ-001. Satisfies MQ05, MQ06, MQ07.
- SWDD-MQ-PEEK: `MsgQ::peek_at` — compute slot = (read_idx + idx) % max_msgs; if idx >= used_msgs: ENOMSG. Refines SWARCH-MQ-001. Satisfies MQ10.
- SWDD-MQ-PURGE: `MsgQ::purge` — reset used_msgs=0, read_idx=write_idx, return old used. Refines SWARCH-MQ-001. Satisfies MQ11, MQ12.

**Stack (3 SWDD):**
- SWDD-SK-INIT: `Stack::init` — validate capacity>0, set count=0. Refines SWARCH-SK-001. Satisfies SK01.
- SWDD-SK-PUSH: `Stack::push` — if count<capacity: count+=1, OK; else ENOMEM. Refines SWARCH-SK-001. Satisfies SK02, SK04, SK05.
- SWDD-SK-POP: `Stack::pop` — if count>0: count-=1, OK; else EBUSY. Refines SWARCH-SK-001. Satisfies SK03, SK06, SK07.

**Pipe (5 SWDD):**
- SWDD-PP-INIT: `Pipe::init` — validate size>0, set used=0, flags=FLAG_OPEN. Refines SWARCH-PP-001. Satisfies PP01.
- SWDD-PP-WRITE: `Pipe::write_check` — if closed: EPIPE; if resetting: ECANCELED; if request_len==0: ENOMSG; clamp to free space, used+=actual. Refines SWARCH-PP-001. Satisfies PP02, PP04, PP06, PP09.
- SWDD-PP-READ: `Pipe::read_check` — if resetting: ECANCELED; if closed && empty: EPIPE; if request_len==0: ENOMSG; clamp to available data, used-=actual. Refines SWARCH-PP-001. Satisfies PP03, PP05, PP07, PP10.
- SWDD-PP-CLOSE: `Pipe::close` — clear all flags (flags=0). Refines SWARCH-PP-001. Satisfies PP08.
- SWDD-PP-RESET: `Pipe::reset` — set used=0, flags|=FLAG_RESET. Refines SWARCH-PP-001. Satisfies PP09.

### Part 5: Unit verification (SWE.4)

Per primitive, 5 artifacts matching the semaphore pattern. All test counts verified via `grep -c '#\[test\]'` on 2026-03-14.

| Primitive | UV-*-001 (unit) | UV-*-002 (proptest) | UV-*-003 (kani) | UV-*-004 (miri) | UV-*-005 (fuzz) |
|-----------|-----------------|---------------------|-----------------|-----------------|-----------------|
| Mutex | 14 inline tests in plain/src/mutex.rs | 6 proptest in tests/proptest_mutex.rs | 11 kani in tests/kani_harnesses.rs | miri on full suite | 2 fuzz targets (mutex_fuzz.rs, mutex_api_fuzz.rs) |
| CondVar | 12 inline tests in plain/src/condvar.rs | 5 proptest in tests/proptest_condvar.rs | 8 kani in tests/kani_harnesses.rs | miri on full suite | 1 fuzz target (condvar_fuzz.rs) |
| MsgQ | 28 inline tests in plain/src/msgq.rs | 6 proptest in tests/proptest_msgq.rs | 8 kani in tests/kani_harnesses.rs | miri on full suite | 1 fuzz target (msgq_fuzz.rs) |
| Stack | 9 inline tests in plain/src/stack.rs | 5 proptest in tests/proptest_stack.rs | 6 kani in tests/kani_harnesses.rs | miri on full suite | 1 fuzz target (stack_fuzz.rs) |
| Pipe | 17 inline tests in plain/src/pipe.rs | 5 proptest in tests/proptest_pipe.rs | 7 kani in tests/kani_harnesses.rs | miri on full suite | 1 fuzz target (pipe_fuzz.rs) |

Each UV-*-001 verifies the SWDD-*-* artifacts for its primitive.
Each UV-*-002..005 verifies the corresponding SWDD-*-* artifacts.

### Part 6: Integration verification (SWE.5)

One IV per primitive. Test counts from audit.

| ID | Tests | File | Verifies |
|----|-------|------|----------|
| IV-MUT-001 | 16 integration tests | tests/mutex_integration.rs | SWARCH-MUT-001 |
| IV-CV-001 | 12 integration tests | tests/condvar_integration.rs | SWARCH-CV-001 |
| IV-MQ-001 | 26 integration tests | tests/msgq_integration.rs | SWARCH-MQ-001 |
| IV-SK-001 | 13 integration tests | tests/stack_integration.rs | SWARCH-SK-001 |
| IV-PP-001 | 18 integration tests | tests/pipe_integration.rs | SWARCH-PP-001 |

No benchmarks exist for non-semaphore primitives (only sem_bench.rs exists).

### Part 7: Software verification (SWE.6)

Per primitive: FV-*-001 = Verus, FV-*-002 = reserved for Rocq, FV-*-003 = Clippy.
This matches the semaphore numbering (FV-SEM-001=Verus, FV-SEM-002=Rocq, FV-SEM-003=Clippy).

| ID | Method | Source | Verifies |
|----|--------|--------|----------|
| FV-MUT-001 | Verus SMT/Z3 | src/mutex.rs — 3 proof lemmas | SWREQ-MUT-M01..M11 |
| FV-MUT-003 | Clippy ASIL-D profile | shared Cargo.toml lints | SWREQ-MUT-M01, M09 |
| FV-CV-001 | Verus SMT/Z3 | src/condvar.rs — 3 proof lemmas | SWREQ-CV-C01..C08 |
| FV-CV-003 | Clippy ASIL-D profile | shared | SWREQ-CV-C01 |
| FV-MQ-001 | Verus SMT/Z3 | src/msgq.rs — 4 proof lemmas | SWREQ-MQ-MQ01..MQ13 |
| FV-MQ-003 | Clippy ASIL-D profile | shared | SWREQ-MQ-MQ01 |
| FV-SK-001 | Verus SMT/Z3 | src/stack.rs — 6 proof lemmas | SWREQ-SK-SK01..SK09 |
| FV-SK-003 | Clippy ASIL-D profile | shared | SWREQ-SK-SK01 |
| FV-PP-001 | Verus SMT/Z3 | src/pipe.rs — 5 proof lemmas | SWREQ-PP-PP01..PP10 |
| FV-PP-003 | Clippy ASIL-D profile | shared | SWREQ-PP-PP01 |

### Part 8: System verification (SYS.5)

One SV per system requirement. Verifies the SYSREQ via Zephyr qemu integration test suites.

| ID | Verifies | Evidence | Test Suite |
|----|----------|----------|------------|
| SV-SEM-001 | SYSREQ-SEM-001 | 24/24 pass on qemu_cortex_m3 | zephyr/tests/kernel/semaphore/semaphore |
| SV-MUT-001 | SYSREQ-MUT-001 | 12/12 pass on qemu_cortex_m3 | zephyr/tests/kernel/mutex (mutex_api + mutex_api_1cpu + sys_mutex) |
| SV-CV-001 | SYSREQ-CV-001 | 11/11 pass on qemu_cortex_m3 | zephyr/tests/kernel/condvar |
| SV-MQ-001 | SYSREQ-MQ-001 | 13/13 pass on qemu_cortex_m3 | zephyr/tests/kernel/msgq (msgq_api + msgq_api_1cpu + msgq_usage) |
| SV-SK-001 | SYSREQ-SK-001 | 12/12 pass on qemu_cortex_m3 | zephyr/tests/kernel/stack (contexts + fail + usage + usage_1cpu) |
| SV-PP-001 | SYSREQ-PP-001 | 18/18 pass on qemu_cortex_m3 | zephyr/tests/kernel/pipe (basic + concurrency + stress) |

This closes the sys2-has-verification gap (currently 0%).

## Artifact File Layout

All new artifacts go into existing files:
- **provenance.yaml** — append PROV-MUT/CV/MQ/SK/PP-001 (5 new)
- **design.yaml** — fix SEM/WQ drift + append SWARCH (5) + SWDD (22) for all 5 primitives
- **verification.yaml** — fix UV-SEM-001 + append UV (25) + IV (5) + FV (10) + SV (6) for all primitives
- **requirements.yaml** — no changes needed

## Expected Outcome

| Metric | Before | After |
|--------|--------|-------|
| Total artifacts (design.yaml) | 8 (2 SWARCH + 6 SWDD) | 35 (7 SWARCH + 28 SWDD) |
| Total artifacts (verification.yaml) | 10 (5 UV + 2 IV + 3 FV) | 56 (30 UV + 7 IV + 13 FV + 6 SV) |
| Total artifacts (provenance.yaml) | 3 | 8 |
| Grand total (all files) | 48 rivet artifacts | 126 rivet artifacts |
| Rivet coverage | 98.1% | 100% |
| sys2-has-verification | 0/1 (0%) | 6/6 (100%) |
| Drift issues | 5 | 0 |
