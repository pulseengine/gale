# UCA Remaining Fix Plan — U-7 through U-10

Scoping pass only (read-only). Source in `artifacts/stpa_controllers_ucas.yaml`
(LS-7..LS-10 for reproductions). Sibling U-5/U-6 (MMU/MPU) is orthogonal —
none of U-7..U-10 is invalidated.

| UCA  | Area              | ABI break | Verus refresh            | Rough size | Order |
|------|-------------------|-----------|--------------------------|------------|-------|
| U-7  | smp_state         | yes (2)   | yes (add cpu_id, SM3)    | 1 day      | 3     |
| U-8  | spinlock          | yes (5)   | medium (Option<u32> enc) | 1–2 days   | 4     |
| U-9  | spinlock_validate | no        | minor (BUILD_ASSERT)     | 1–2 hours  | 1     |
| U-10 | priority / thread | yes (3)   | yes (signed Priority)    | 1 day      | 2     |

## U-7 — SMP coordinator omits CPU identity

**Source:** `src/smp_state.rs:360` (`stop_cpu_decide`); FFI wrappers
`ffi/src/lib.rs:4998` and `:5160`. Shim `zephyr/gale_smp_state.c:54`
already receives `id` but drops it.

**Fix shape:** Add `cpu_id: u32` to `stop_cpu_decide` and the two
extern wrappers; reject `cpu_id == 0` with EINVAL in addition to the
`active_cpus <= 1` guard. Extend SM3 ensures:
`cpu_id == 0 ==> rc == EINVAL`. SM4 lock-counter coarseness is
out of scope for U-7 (LS-7 only targets CPU identity).

**Blast radius:** `src/smp_state.rs` (+~15), `ffi/src/lib.rs` (2 sigs,
~10), header `gale_smp_state.h` (+1 param × 2), shim (pass `id`),
`tests/differential_smp_state.rs` (293 lines — 6 tests updated, 1 new).

**Safety:** Strictly stricter; only LS-7 behavior changes. Add Kani
harness `stop_cpu_rejects_cpu0` + Verus `lemma_cpu0_rejected`.

**Risk:** ~1 day — touches both ABIs, shim, diff tests.

## U-8 — Spinlock collapses owner tid 0 onto unowned

**Source:** Model `src/spinlock.rs` uses `Option<u32>` (correct); FFI
`ffi/src/lib.rs:9860-9970` collapses `None` onto `0`. Shim
`zephyr/gale_spinlock.c:50-83` uses plain `uint32_t owner_tid`. Kani
harnesses at 9988-10043 paper over it via `assume(tid != 0)` (×3).

**Fix shape (preferred):** Add an explicit `held: u8` valid-flag
alongside `owner_tid`. All 5 extern entry points grow an
`in_held`/`out_held` parameter; headers, shim, and `k_spinlock`
caller-side struct gain the flag byte. Lift Kani `assume(tid != 0)`.
Cheaper alternative: reserve tid 0 and reject at every entry (no sig
change, caller contract narrows) — reject.

**Blast radius:** `ffi/src/lib.rs` (5 sigs, ~60), `gale_spinlock.h`
(5 decls), `zephyr/gale_spinlock.c` (~40), diff test
`tests/differential_spinlock.rs` (212, rework), Kani (drop asserts +
new `release_by_tid0_roundtrip`). Check Zephyr k_spinlock field
layout impact before adding a byte.

**Safety:** Cannot regress U-9 if we keep U-9's fix independent of
the tid encoding. Add Verus `lemma_tid0_distinguishable_from_unowned`.

**Risk:** ~1–2 days. Widest surface of the four — dedicate a session.

## U-9 — Validator accepts same-CPU reacquire on cpu_id ≥ 4

**Source:** `src/spinlock_validate.rs:41-53` hard-codes `MAX_CPUS=4`,
`CPU_MASK=3`. FFI `ffi/src/lib.rs:8902-8945` erases the Verus
`requires cpu_id_valid(...)` at the extern boundary. Shim
`zephyr/gale_spinlock_validate.c:33` has no check either.

**Fix shape:** Two layers: (a) `BUILD_ASSERT(CONFIG_MP_MAX_NUM_CPUS <=
GALE_SPINLOCK_MAX_CPUS)` in the shim, mirroring the Verus precondition
at compile time; (b) runtime fast-reject `if current_cpu_id >= MAX_CPUS
{ return 0 }` in `gale_spin_lock_valid` + `gale_spin_unlock_valid`.
Widening MAX_CPUS to 8 is a separate follow-up (needs thread-ptr
alignment audit). No ABI change.

**Blast radius:** `src/spinlock_validate.rs` (~4 lines), `ffi/src/lib.rs`
(+2 lines × 2 fns), `zephyr/gale_spinlock_validate.c` (+1
BUILD_ASSERT), diff test (311 lines; +2 OOB cpu_id cases).

**Safety:** Strictly additive rejections — cannot regress other UCAs.
Add Kani `spin_lock_valid_rejects_oob_cpu_id`.

**Risk:** ~1–2 hours. **Most tractable of the four.**

## U-10 — Thread priority accepts OOR, rejects coop

**Source:** `src/priority.rs:13` (`MAX_PRIORITY: u32 = 32`, `Priority{
value: u32 }`). `src/thread_lifecycle.rs:569` `priority_set_decide`
takes `u32`. FFI at `ffi/src/lib.rs:3957` and :4264 expose `u32`.
No `zephyr/*.c` shim calls these yet — churn is internal.

**Fix shape:** Switch priority to `i32` with runtime params
`num_coop: u32`, `num_preempt: u32`. Validator:
`-(num_coop as i32) <= prio < (num_preempt as i32)`. `Priority.value:
i32`, `inv` updated. `priority_set_decide(prio: i32, num_coop, num_preempt)`.
`ThreadInfo.priority: i32`. Scheduler-side OOB-indexing fix (LS-10's
consumer) is a follow-up — out of scope for priority.rs.

**Blast radius:** `src/priority.rs` (~30, lemmas re-proved),
`src/thread_lifecycle.rs` (~50, 4 lemmas, ThreadInfo + decide),
`ffi/src/lib.rs` (3 sigs, ~30), header (~10), diff test (542 lines,
mass-update priority literals). `priority.rs` is imported by
`thread.rs` and `sched.rs` — audit after edit.

**Safety:** Could regress TH1/TH2 if the new `inv` is weaker. Add
Verus `lemma_priority_signed_range` + Kani cases `priority == -1`
accepted, `priority == num_preempt` rejected.

**Risk:** ~1 day. No external C caller yet = cheapest time to break
this ABI; diff is wide but self-contained.

## Recommended ordering

1. **U-9** — hours, strictly additive; morning warm-up.
2. **U-10** — medium, no external C callers yet (ABI-change free-pass).
3. **U-7** — wide but shallow; 1 day.
4. **U-8** — widest radius + struct-layout design call; dedicate a session.

## Invalidation check

None of U-7..U-10 is subsumed by U-1..U-6 nor by the in-flight U-5/U-6
MMU/MPU work. All four reproduce on main per LS-7..LS-10.
