# ASPICE Traceability Completion Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete ASPICE V-model artifacts for all 6 kernel primitives, fix drift, achieve 100% rivet coverage.

**Architecture:** Edit 3 existing YAML files (provenance.yaml, design.yaml, verification.yaml) to add ground-truthed artifacts. All data comes from the verified spec at `docs/superpowers/specs/2026-03-14-aspice-traceability-completion-design.md`.

**Tech Stack:** YAML (rivet artifacts), `rivet validate` for verification.

**Spec:** `docs/superpowers/specs/2026-03-14-aspice-traceability-completion-design.md`

---

### Task 1: Fix semaphore artifact drift

**Files:**
- Modify: `artifacts/design.yaml` (lines 23, 25, 58-59)
- Modify: `artifacts/verification.yaml` (line 14)

- [ ] **Step 1: Fix SWARCH-SEM-001 try_take return type**

In `artifacts/design.yaml`, change:
```yaml
          - "Semaphore::try_take() -> i32"
```
to:
```yaml
          - "Semaphore::try_take() -> TakeResult"
```

- [ ] **Step 2: Fix SWARCH-SEM-001 reset return type**

In `artifacts/design.yaml`, change:
```yaml
          - "Semaphore::reset() -> usize"
```
to:
```yaml
          - "Semaphore::reset() -> u32"
```

- [ ] **Step 3: Fix SWARCH-WQ-001 unpend_all and len return types**

In `artifacts/design.yaml`, change:
```yaml
          - "WaitQueue::unpend_all(i32) -> usize"
          - "WaitQueue::len() -> usize"
```
to:
```yaml
          - "WaitQueue::unpend_all(i32) -> u32"
          - "WaitQueue::len() -> u32"
```

- [ ] **Step 4: Fix UV-SEM-001 test count**

In `artifacts/verification.yaml`, change:
```yaml
      25 unit tests in plain/src/sem.rs and plain/src/wait_queue.rs.
```
to:
```yaml
      20 unit tests (15 in plain/src/sem.rs, 5 in plain/src/wait_queue.rs).
```

- [ ] **Step 5: Commit drift fixes**

```bash
git add artifacts/design.yaml artifacts/verification.yaml
git commit -m "Fix semaphore artifact drift: return types and test count"
```

---

### Task 2: Add provenance records for 5 primitives

**Files:**
- Modify: `artifacts/provenance.yaml` (append after PROV-WQ-001)

- [ ] **Step 1: Append 5 provenance records**

Append to `artifacts/provenance.yaml` after the PROV-WQ-001 block. One record per upstream C file. All SHA256 checksums verified against live Zephyr files on 2026-03-14.

Records to add: PROV-MUT-001 (kernel/mutex.c), PROV-CV-001 (kernel/condvar.c), PROV-MQ-001 (kernel/msg_q.c), PROV-SK-001 (kernel/stack.c), PROV-PP-001 (kernel/pipe.c).

Use the exact PROV-SEM-001 format. Field values from spec Part 2.

Key fields per record:
- PROV-MUT-001: mutex.c, sha256=4ae6ed30..., 337 lines, functions: z_impl_k_mutex_init/lock/unlock, gale-file=plain/src/mutex.rs, scope=partial-functions
- PROV-CV-001: condvar.c, sha256=a2821164..., 171 lines, functions: z_impl_k_condvar_signal/broadcast/wait, gale-file=plain/src/condvar.rs, scope=partial-functions
- PROV-MQ-001: msg_q.c, sha256=4531c765..., 516 lines, functions: k_msgq_init/put/get/peek_at/purge, gale-file=plain/src/msgq.rs, scope=partial-functions
- PROV-SK-001: stack.c, sha256=0e237426..., 222 lines, functions: k_stack_init/push/pop, gale-file=plain/src/stack.rs, scope=partial-functions
- PROV-PP-001: pipe.c, sha256=af7da715..., 358 lines, functions: z_impl_k_pipe_init/write/read/reset/close, gale-file=plain/src/pipe.rs, scope=partial-functions

- [ ] **Step 2: Commit**

```bash
git add artifacts/provenance.yaml
git commit -m "Add provenance records for mutex, condvar, msgq, stack, pipe"
```

---

### Task 3: Add design artifacts (SWARCH + SWDD) for 5 primitives

**Files:**
- Modify: `artifacts/design.yaml` (append after SWDD-WQ-PEND)

- [ ] **Step 1: Append SWARCH and SWDD for mutex**

Add SWARCH-MUT-001 and 4 SWDDs (INIT, LOCK, LOCK-BLOCKING, UNLOCK).
Follow exact SWARCH-SEM-001 format. Interface list from spec Part 3.
Each SWDD has `refines` → SWARCH-MUT-001, `satisfies` → SWREQs per spec Part 4, and `function-maps-to` → PROV-MUT-001.

- [ ] **Step 2: Append SWARCH and SWDD for condvar**

Add SWARCH-CV-001 and 4 SWDDs (INIT, SIGNAL, BROADCAST, WAIT).
Note: condvar replaces PROV-CV-001 but has no FFI (pure wait queue wrapper).

- [ ] **Step 3: Append SWARCH and SWDD for msgq**

Add SWARCH-MQ-001 and 6 SWDDs (INIT, PUT, PUT-FRONT, GET, PEEK, PURGE).

- [ ] **Step 4: Append SWARCH and SWDD for stack**

Add SWARCH-SK-001 and 3 SWDDs (INIT, PUSH, POP).

- [ ] **Step 5: Append SWARCH and SWDD for pipe**

Add SWARCH-PP-001 and 5 SWDDs (INIT, WRITE, READ, CLOSE, RESET).

- [ ] **Step 6: Commit**

```bash
git add artifacts/design.yaml
git commit -m "Add SWARCH and SWDD artifacts for mutex, condvar, msgq, stack, pipe"
```

---

### Task 4: Add verification artifacts (UV + IV + FV + SV) for all primitives

**Files:**
- Modify: `artifacts/verification.yaml` (append after FV-SEM-003)

- [ ] **Step 1: Append UV for mutex (5 artifacts)**

UV-MUT-001: 14 unit tests in plain/src/mutex.rs. Verifies SWDD-MUT-INIT, SWDD-MUT-LOCK, SWDD-MUT-LOCK-BLOCKING, SWDD-MUT-UNLOCK.
UV-MUT-002: 6 proptest in tests/proptest_mutex.rs. Verifies SWDD-MUT-LOCK, SWDD-MUT-UNLOCK.
UV-MUT-003: 11 kani harnesses in tests/kani_harnesses.rs. Verifies all 4 SWDDs.
UV-MUT-004: Miri UB detection on full suite. Verifies all 4 SWDDs.
UV-MUT-005: 2 fuzz targets (mutex_fuzz.rs, mutex_api_fuzz.rs). Verifies SWDD-MUT-LOCK, SWDD-MUT-UNLOCK.

- [ ] **Step 2: Append UV for condvar (5 artifacts)**

UV-CV-001: 12 unit tests in plain/src/condvar.rs. Verifies SWDD-CV-INIT, SWDD-CV-SIGNAL, SWDD-CV-BROADCAST, SWDD-CV-WAIT.
UV-CV-002: 5 proptest in tests/proptest_condvar.rs.
UV-CV-003: 8 kani harnesses.
UV-CV-004: Miri.
UV-CV-005: 1 fuzz target (condvar_fuzz.rs).

- [ ] **Step 3: Append UV for msgq (5 artifacts)**

UV-MQ-001: 28 unit tests in plain/src/msgq.rs. Verifies SWDD-MQ-INIT, SWDD-MQ-PUT, SWDD-MQ-PUT-FRONT, SWDD-MQ-GET, SWDD-MQ-PEEK, SWDD-MQ-PURGE.
UV-MQ-002: 6 proptest in tests/proptest_msgq.rs.
UV-MQ-003: 8 kani harnesses.
UV-MQ-004: Miri.
UV-MQ-005: 1 fuzz target (msgq_fuzz.rs).

- [ ] **Step 4: Append UV for stack (5 artifacts)**

UV-SK-001: 9 unit tests in plain/src/stack.rs. Verifies SWDD-SK-INIT, SWDD-SK-PUSH, SWDD-SK-POP.
UV-SK-002: 5 proptest in tests/proptest_stack.rs.
UV-SK-003: 6 kani harnesses.
UV-SK-004: Miri.
UV-SK-005: 1 fuzz target (stack_fuzz.rs).

- [ ] **Step 5: Append UV for pipe (5 artifacts)**

UV-PP-001: 17 unit tests in plain/src/pipe.rs. Verifies SWDD-PP-INIT, SWDD-PP-WRITE, SWDD-PP-READ, SWDD-PP-CLOSE, SWDD-PP-RESET.
UV-PP-002: 5 proptest in tests/proptest_pipe.rs.
UV-PP-003: 7 kani harnesses.
UV-PP-004: Miri.
UV-PP-005: 1 fuzz target (pipe_fuzz.rs).

- [ ] **Step 6: Append IV for all 5 primitives**

IV-MUT-001: 16 integration tests in tests/mutex_integration.rs. Verifies SWARCH-MUT-001.
IV-CV-001: 12 integration tests in tests/condvar_integration.rs. Verifies SWARCH-CV-001.
IV-MQ-001: 26 integration tests in tests/msgq_integration.rs. Verifies SWARCH-MQ-001.
IV-SK-001: 13 integration tests in tests/stack_integration.rs. Verifies SWARCH-SK-001.
IV-PP-001: 18 integration tests in tests/pipe_integration.rs. Verifies SWARCH-PP-001.

- [ ] **Step 7: Append FV for all 5 primitives**

FV numbering: 001=Verus, 002=reserved for Rocq, 003=Clippy (consistent with SEM).

FV-MUT-001: Verus (11 SWREQ verifies). FV-MUT-003: Clippy.
FV-CV-001: Verus (8 SWREQ verifies). FV-CV-003: Clippy.
FV-MQ-001: Verus (13 SWREQ verifies). FV-MQ-003: Clippy.
FV-SK-001: Verus (9 SWREQ verifies). FV-SK-003: Clippy.
FV-PP-001: Verus (10 SWREQ verifies). FV-PP-003: Clippy.

- [ ] **Step 8: Append SV (system verification) for all 6 primitives**

SV-SEM-001: Verifies SYSREQ-SEM-001. 24/24 Zephyr tests, suite=zephyr/tests/kernel/semaphore/semaphore.
SV-MUT-001: Verifies SYSREQ-MUT-001. 12/12 Zephyr tests, suite=zephyr/tests/kernel/mutex.
SV-CV-001: Verifies SYSREQ-CV-001. 11/11 Zephyr tests, suite=zephyr/tests/kernel/condvar.
SV-MQ-001: Verifies SYSREQ-MQ-001. 13/13 Zephyr tests, suite=zephyr/tests/kernel/msgq.
SV-SK-001: Verifies SYSREQ-SK-001. 12/12 Zephyr tests, suite=zephyr/tests/kernel/stack.
SV-PP-001: Verifies SYSREQ-PP-001. 18/18 Zephyr tests, suite=zephyr/tests/kernel/pipe.

- [ ] **Step 9: Commit**

```bash
git add artifacts/verification.yaml
git commit -m "Add UV, IV, FV, SV verification artifacts for all primitives"
```

---

### Task 5: Validate with rivet and regenerate context

- [ ] **Step 1: Run rivet validate**

```bash
cd /Volumes/Home/git/zephyr/gale && rivet validate
```

Expected: 0 errors, 0 warnings.

- [ ] **Step 2: Run rivet coverage**

```bash
cd /Volumes/Home/git/zephyr/gale && rivet coverage
```

Expected: 100% overall. All rules at 100% including sys2-has-verification.

- [ ] **Step 3: Regenerate agent context**

```bash
cd /Volumes/Home/git/zephyr/gale && rivet context
```

- [ ] **Step 4: Fix any validation errors**

If rivet reports errors, fix the YAML and re-validate until clean.

- [ ] **Step 5: Final commit**

```bash
git add artifacts/ .rivet/
git commit -m "ASPICE traceability complete: 126 artifacts, 100% coverage, 0 drift"
```
