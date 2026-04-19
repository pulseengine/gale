# STPA GAP-2 Audit: Decision Struct -> Verified Model Delegation

**Original date:** 2026-03-29
**Last snapshot:** 2026-04-19
**Scope:** All `pub extern "C" fn gale_*` functions in `ffi/src/lib.rs`
**Pattern:** GAP-2 requires that FFI decision structs delegate to Verus-verified
model functions (`src/*.rs`) rather than reimplementing logic inline.

## Current status (2026-04-19)

Automated scan of `ffi/src/lib.rs` — count of functions that contain a
`use gale::<module>::` statement inside their body (indicating delegation):

| Metric        | Value |
|---            |---    |
| Total FFI fns | 208   |
| **GREEN**     | **140** (67%) |
| RED           | 68    |

RED breakdown by family (lines = non-empty, non-comment body):

| Family     | Count | Cheapest to wire |
|---         | ---   | --- |
| atomic     | 7 | passthrough RMWs; arguably GAP-2 N/A |
| sys_heap   | 7 | needs `heap` model decision fns (26–36 line bodies) |
| spinlock   | 6 | stateful; defer until spinlock model matures |
| bitarray   | 4 | no model exists — new `bitarray.rs` needed |
| mem_domain | 4 | complex partition arithmetic (45–51 lines) |
| k_object   | 4 | flag ops; small model already in `userspace.rs` |
| ring_buf   | 4 | `ring_buf.rs` exists; 4/8 already wired |
| thread     | 3 | `thread_lifecycle.rs` exists |
| timer      | 3 | `timer.rs` exists |
| timeout    | 3 | `timeout.rs` exists |
| condvar    | ~~3~~ **0** | wired in 2026-04-19 commit |
| sched      | 2 | `sched.rs` exists |
| kheap      | 2 | `kheap.rs` exists; pattern matches mbox |
| mbox       | 2 | `mbox.rs` exists |
| mempool    | 2 | `mempool.rs` exists |
| poll       | 2 | `poll.rs` exists |
| rb (tree)  | 2 | no model; red-black invariants |
| work       | 2 | `work.rs` exists |
| other      | 8 | see detail tables below |

The table below this section is the original 2026-03-29 classification.
Entries marked "RED" there may now be GREEN — the summary above reflects
the current automated scan. Full re-classification is tracked in
[issue #9](https://github.com/pulseengine/gale/issues/9).

## Background

The GAP-2 pattern (STPA control action: "decision struct calls verified model")
ensures that safety-critical logic lives in a single place (the Verus-verified
`gale` crate) and the FFI layer is a thin translation shim. When FFI functions
reimplement logic inline, there is a divergence risk: the FFI and the model can
drift apart, and the Verus proofs no longer cover the code that actually executes
at runtime.

**Historical note (2026-03-29):** When this audit was written, only the two
`gale_k_sem_*_decide` functions followed this pattern. As of the 2026-04-19
snapshot above, 140 of 208 FFI functions delegate to a verified `gale::`
model.

## Classification Criteria

- **GREEN**: FFI function delegates to a `gale::` model function.
- **YELLOW**: FFI logic is trivially correct (single comparison, constant return,
  null check, bitwise op with no arithmetic, or thin wrapper around another FFI
  function that is itself classified). No divergence risk.
- **RED**: FFI contains non-trivial arithmetic, branching, or state machine logic
  that duplicates what exists (or should exist) in the verified model. Divergence
  risk is real.

---

## Module-by-Module Audit

### 1. Semaphore (`gale_sem_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_sem_count_init` | 255 | YELLOW | Single comparison: `limit == 0 \|\| initial_count > limit` |
| `gale_sem_count_give` | 271 | RED | Saturating increment logic. Model has `give_decide` but this older API reimplements it inline. |
| `gale_sem_count_take` | 290 | RED | Decrement-or-fail with pointer write. Model has `take_decide` but this older API reimplements it inline. |
| `gale_k_sem_give_decide` | 330 | **GREEN** | Calls `gale::sem::give_decide` |
| `gale_k_sem_take_decide` | 376 | **GREEN** | Calls `gale::sem::take_decide` |

### 2. Mutex (`gale_mutex_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_mutex_lock_validate` | 447 | RED | Ownership check + `checked_add` + 3-way branch. Reimplements mutex lock state machine. |
| `gale_mutex_unlock_validate` | 508 | RED | Ownership check + decrement + 4-way branch. Reimplements mutex unlock state machine. |
| `gale_k_mutex_lock_decide` | 571 | RED | 4-way decision (acquire/reentrant/busy/pend) with `checked_add`. No model call. |
| `gale_k_mutex_unlock_decide` | 641 | RED | 4-way decision (EINVAL/EPERM/released/unlocked) with arithmetic. No model call. |

### 3. Message Queue (`gale_msgq_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_msgq_init_validate` | 724 | YELLOW | Zero checks + `checked_mul`. Trivial validation. |
| `gale_msgq_put` | 767 | RED | Ring buffer index advancement with wrap + used count increment. |
| `gale_msgq_put_front` | 820 | RED | Reverse ring index retreat with wrap + used count increment. |
| `gale_msgq_get` | 873 | RED | Ring buffer index advancement with wrap + used count decrement. |
| `gale_msgq_peek_at` | 925 | RED | Modular index computation: `(read_idx + idx) % max_msgs`. |
| `gale_k_msgq_put_decide` | 980 | RED | 4-way decision with ring index arithmetic. No model call. |
| `gale_k_msgq_get_decide` | 1049 | RED | 4-way decision with ring index arithmetic. No model call. |

### 4. Pipe (`gale_pipe_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_pipe_write_check` | 1169 | RED | Flag checks + byte count computation (`min(request, free)`). |
| `gale_pipe_read_check` | 1224 | RED | Flag checks + byte count computation (`min(request, used)`). |
| `gale_k_pipe_write_decide` | 1283 | RED | 4-way decision with flag checks + byte arithmetic. No model call. |
| `gale_k_pipe_read_decide` | 1362 | RED | 4-way decision with flag checks + byte arithmetic. No model call. |

### 5. Stack (`gale_stack_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_stack_init_validate` | 1428 | YELLOW | Single zero check. |
| `gale_stack_push_validate` | 1451 | RED | Capacity check + increment with pointer write. |
| `gale_stack_pop_validate` | 1489 | RED | Empty check + decrement with pointer write. |
| `gale_k_stack_push_decide` | 1538 | RED | 3-way decision (wake/store/full) with increment. No model call. |
| `gale_k_stack_pop_decide` | 1587 | RED | 3-way decision (pop/busy/pend) with decrement. No model call. |

### 6. Timer (`gale_timer_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_timer_init_validate` | 1623 | YELLOW | Always returns OK. |
| `gale_timer_expire` | 1643 | RED | Overflow-checked increment. Model has `expire()`. |
| `gale_timer_status_get` | 1680 | YELLOW | Trivial: return old value, write 0. |
| `gale_k_timer_expire_decide` | 1711 | RED | Saturating increment + period classification. No model call. |
| `gale_k_timer_status_decide` | 1746 | YELLOW | Trivial: return status, new_status = 0. |

### 7. Memory Slab (`gale_mem_slab_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_mem_slab_init_validate` | 1768 | YELLOW | Two zero checks. |
| `gale_mem_slab_alloc_validate` | 1790 | RED | Capacity check + increment. Model has `alloc()`. |
| `gale_mem_slab_free_validate` | 1826 | RED | Zero check + decrement. Model has `free()`. |
| `gale_k_mem_slab_alloc_decide` | 1872 | RED | 3-way decision (alloc/pend/nomem) with increment. No model call. |
| `gale_k_mem_slab_free_decide` | 1919 | RED | 2-way decision (free/wake) with decrement. No model call. |

### 8. Event (`gale_event_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_event_post` | 1985 | YELLOW | Single bitwise OR. |
| `gale_event_set` | 2014 | YELLOW | Store old value. |
| `gale_event_clear` | 2045 | YELLOW | Single AND-complement. |
| `gale_event_set_masked` | 2075 | YELLOW | Single masked-set expression. |
| `gale_event_wait_check_any` | 2104 | YELLOW | Single `(events & desired) != 0`. |
| `gale_event_wait_check_all` | 2128 | YELLOW | Single `(events & desired) == desired`. |
| `gale_k_event_post_decide` | 2166 | YELLOW | Single masked-set expression in struct. |
| `gale_k_event_wait_decide` | 2195 | RED | 3-way decision (matched/pend/timeout) with bitwise match logic. |

### 9. FIFO (`gale_fifo_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_fifo_put_validate` | 2250 | RED | Overflow check + increment. Model has `put()`. |
| `gale_fifo_get_validate` | 2285 | RED | Zero check + decrement. Model has `get()`. |
| `gale_k_fifo_put_decide` | 2328 | YELLOW | Single boolean branch (wake vs insert). |
| `gale_k_fifo_get_decide` | 2365 | RED | 3-way decision (get/pend/nodata). |

### 10. LIFO (`gale_lifo_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_lifo_put_validate` | 2405 | RED | Overflow check + increment. Model has `put()`. |
| `gale_lifo_get_validate` | 2440 | RED | Zero check + decrement. Model has `get()`. |
| `gale_k_lifo_put_decide` | 2483 | YELLOW | Single boolean branch (wake vs insert). |
| `gale_k_lifo_get_decide` | 2520 | RED | 3-way decision (get/pend/nodata). |

### 11. Queue (`gale_queue_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_queue_append_validate` | 2559 | RED | Overflow check + increment. Model has `append()`. |
| `gale_queue_prepend_validate` | 2594 | RED | Overflow check + increment. Model has `prepend()`. |
| `gale_queue_get_validate` | 2629 | RED | Zero check + decrement. Model has `get()`. |
| `gale_k_queue_insert_decide` | 2739 | YELLOW | Single boolean branch (wake vs insert). |
| `gale_k_queue_get_decide` | 2772 | YELLOW | 3-way but each branch is a trivial constant. No arithmetic. |

### 12. Mailbox (`gale_mbox_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_mbox_validate_send` | 2667 | YELLOW | Single zero check. |
| `gale_mbox_match_check` | 2691 | YELLOW | `send_id == 0 \|\| recv_id == 0 \|\| send_id == recv_id`. |
| `gale_mbox_data_exchange` | 2711 | YELLOW | `min(tx_size, rx_buf_size)`. |
| `gale_k_mbox_put_decide` | 2812 | YELLOW | 3-way constant branch (matched/enomsg/pend). No arithmetic. |
| `gale_k_mbox_get_decide` | 2850 | YELLOW | 3-way constant branch (consume/enomsg/pend). No arithmetic. |

### 13. Timeout (`gale_timeout_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_timeout_add_decide` | 2913 | RED | Overflow check + u64 addition. Model has `add()`. |
| `gale_timeout_abort_decide` | 2959 | YELLOW | Single boolean branch. |
| `gale_timeout_announce_decide` | 2996 | RED | Overflow check + u64 addition + deadline comparison. Model has `announce()`. |
| `gale_timeout_add` | 3035 | YELLOW | Thin wrapper around `gale_timeout_add_decide`. |
| `gale_timeout_abort` | 3059 | YELLOW | Thin wrapper around `gale_timeout_abort_decide`. |
| `gale_timeout_announce` | 3067 | YELLOW | Thin wrapper around `gale_timeout_announce_decide`. |

### 14. Poll (`gale_poll_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_poll_event_init` | 3118 | YELLOW | Write 0 to state. |
| `gale_poll_check_sem` | 3146 | YELLOW | `type == SEM_AVAILABLE && count > 0`. |
| `gale_poll_signal_raise` | 3171 | YELLOW | Set two values through pointers. |
| `gale_poll_signal_reset` | 3198 | YELLOW | Set one value to 0. |
| `gale_k_poll_signal_raise_decide` | 3242 | YELLOW | 2-way branch, both set signaled=1. No arithmetic. |

### 15. Futex (`gale_futex_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_futex_wait_check` | 3293 | YELLOW | Single equality comparison. |
| `gale_futex_wake` | 3316 | RED | 3-way branch with arithmetic (`num_waiters - 1`). Model has `wake()`. |
| `gale_k_futex_wait_decide` | 3365 | YELLOW | 3-way branch, no arithmetic. |
| `gale_k_futex_wake_decide` | 3403 | YELLOW | 3-way branch, no arithmetic (just selects wake_limit). |

### 16. Timeslice (`gale_timeslice_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_timeslice_reset` | 3452 | YELLOW | Write `slice_max_ticks` through pointer. |
| `gale_timeslice_tick` | 3481 | RED | Decrement + expiry detection. Model has `tick()`. |
| `gale_k_timeslice_tick_decide` | 3539 | RED | 4-way decision with expiry detection. No model call. |

### 17. KHeap (`gale_kheap_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_kheap_alloc_validate` | 3604 | RED | Remaining capacity check + addition. Model has `alloc()`. |
| `gale_kheap_free_validate` | 3640 | RED | Underflow check + subtraction. Model has `free()`. |
| `gale_k_kheap_alloc_decide` | 3693 | YELLOW | 3-way constant branch (ptr/pend/null). No arithmetic. |
| `gale_k_kheap_free_decide` | 3735 | YELLOW | 2-way constant branch (free/reschedule). No arithmetic. |

### 18. Thread Lifecycle (`gale_thread_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_thread_create_validate` | 3779 | RED | Capacity check + increment. |
| `gale_thread_exit_validate` | 3810 | RED | Zero check + decrement. |
| `gale_thread_priority_validate` | 3843 | YELLOW | Single range check. |
| `gale_k_thread_create_decide` | 3887 | RED | 4-way validation (stack/priority/count). No model call. |
| `gale_k_thread_abort_decide` | 3958 | YELLOW | 3-way branch on flag bits. No arithmetic. |
| `gale_k_thread_join_decide` | 4011 | YELLOW | 4-way constant branch. No arithmetic. |

### 19. Work (`gale_work_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_k_work_submit_decide` | 4107 | RED | Flag manipulation with bitwise OR. Model has `submit()`. |
| `gale_k_work_cancel_decide` | 4181 | RED | Flag manipulation with clear/set. Model has `cancel()`. |
| `gale_work_submit_validate` | 4237 | YELLOW | Thin wrapper around `gale_k_work_submit_decide`. |
| `gale_work_cancel_validate` | 4260 | YELLOW | Thin wrapper around `gale_k_work_cancel_decide`. |

### 20. Fatal (`gale_fatal_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_fatal_classify` | 4326 | YELLOW | Thin wrapper around `gale_k_fatal_decide`. |
| `gale_k_fatal_decide` | 4346 | RED | Multi-way classification (reason x context x test_mode). Model has `classify()`. |

### 21. MemPool (`gale_mempool_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_mempool_alloc_validate` | 4418 | RED | Capacity check + increment. Model has `alloc()`. |
| `gale_mempool_free_validate` | 4450 | RED | Zero check + decrement. Model has `free()`. |
| `gale_k_mempool_alloc_decide` | 4499 | YELLOW | 2-way constant branch. No arithmetic. |
| `gale_k_mempool_free_decide` | 4536 | YELLOW | 2-way constant branch. No arithmetic. |

### 22. Dynamic (`gale_dynamic_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_dynamic_alloc_validate` | 4578 | RED | Capacity check + increment. Model has `alloc()`. |
| `gale_dynamic_free_validate` | 4610 | RED | Zero check + decrement. Model has `free()`. |
| `gale_dynamic_alloc_decide` | 4732 | RED | Capacity check + increment. No model call. |
| `gale_dynamic_free_decide` | 4770 | RED | Zero check + decrement. No model call. |

### 23. SMP State (`gale_smp_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_smp_start_cpu_validate` | 4658 | RED | Capacity check + increment. Model has `start_cpu()`. |
| `gale_smp_stop_cpu_validate` | 4690 | RED | Min-1 check + decrement. Model has `stop_cpu()`. |
| `gale_smp_start_cpu_decide` | 4809 | RED | Capacity check + increment. No model call. |
| `gale_smp_stop_cpu_decide` | 4847 | RED | Min-1 check + decrement. No model call. |

### 24. Scheduler (`gale_sched_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_sched_next_up` | 4894 | YELLOW | Single `u32::MAX` comparison. |
| `gale_sched_should_preempt` | 4930 | YELLOW | 3-way boolean branch. No arithmetic. |
| `gale_k_sched_next_up_decide` | 4977 | RED | 4-way decision with metairq preemption logic. Model has `next_up()`, `should_preempt()`. |
| `gale_k_sched_preempt_decide` | 5037 | RED | 4-way preemption decision. Model has `should_preempt()`. |

### 25. Memory Domain (`gale_mem_domain_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_mem_domain_check_partition` | 5123 | RED | Overlap detection loop over 16 partitions with u64 arithmetic. Model has `add_partition()`. |
| `gale_k_mem_domain_add_partition_decide` | 5202 | RED | Validation + free slot search loop. No model call. |
| `gale_k_mem_domain_remove_partition_decide` | 5291 | RED | Linear search + match with decrement. No model call. |
| `gale_mem_domain_init_validate_partition` | 5361 | YELLOW | Thin wrapper around `gale_mem_domain_check_partition`. |

### 26. Userspace (`gale_k_object_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_k_object_access_decide` | 5432 | YELLOW | 2-way flag check. No arithmetic. |
| `gale_k_object_validate_decide` | 5483 | RED | 4-way validation (type/perm/init). Model has `validate()`. |
| `gale_k_object_init_decide` | 5532 | YELLOW | Single bitwise OR. |
| `gale_k_object_uninit_decide` | 5553 | YELLOW | Single bitwise AND. |
| `gale_k_object_recycle_decide` | 5579 | YELLOW | Bitwise OR + constant. |
| `gale_k_object_make_public_decide` | 5601 | YELLOW | Single bitwise OR. |

### 27. Sys Heap (`gale_sys_heap_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_sys_heap_alloc_decide` | 7295 | RED | 4-way split/whole/fail decision with size comparison. Model has `alloc()`. |
| `gale_sys_heap_free_decide` | 7370 | RED | Double-free detection + coalesce strategy. Model has `free()`. |
| `gale_sys_heap_aligned_alloc_decide` | 7426 | RED | Power-of-2 validation + padding overflow check. Model has `aligned_alloc()`. |
| `gale_sys_heap_realloc_decide` | 7497 | RED | 4-way shrink/grow/copy/reject with u64 arithmetic. Model has `realloc()`. |
| `gale_sys_heap_init_validate` | 7541 | YELLOW | Two comparisons. |
| `gale_sys_heap_split_validate` | 7566 | RED | Conservation check: `left > 0 && left < original`. Model has `split()`. |
| `gale_sys_heap_merge_validate` | 7595 | RED | Conservation + overflow check with u64 addition. Model has `merge()`. |

### 28. Ring Buffer (`gale_ring_buf_*`)

| FFI Function | Lines | Classification | Notes |
|---|---|---|---|
| `gale_ring_buf_claim_decide` | 7892 | RED | Modular offset computation with wrapping_sub + clamping. Model has `put_n()`, `get_n()`. |
| `gale_ring_buf_finish_validate` | 7952 | YELLOW | Single `size > claimed_size` comparison. |
| `gale_ring_buf_space_get` | 7983 | YELLOW | `wrapping_sub` + clamp. |
| `gale_ring_buf_size_get` | 8013 | YELLOW | Single `wrapping_sub`. |

---

## Summary Totals

| Classification | Count | Percentage |
|---|---|---|
| **GREEN** | 2 | 1.7% |
| **YELLOW** | 54 | 46.6% |
| **RED** | 60 | 51.7% |
| **Total** | 116 | 100% |

---

## RED Function Remediation Plan

For each RED function, the model already contains the equivalent verified logic.
The delegation pattern requires no WaitQueue allocation -- only scalar parameters
are passed. The model functions that use `&self`/`&mut self` can be adapted by
adding standalone `*_decide` functions to the model (as was done for `sem::give_decide`
and `sem::take_decide`).

### Priority 1: Decision API functions (Phase 2 pattern)

These are the most important because they are the primary runtime code path.
The legacy `_validate` functions (Priority 2) are kept for backward compatibility
and can be made into thin wrappers once the decision functions are fixed.

| RED FFI Function | Proposed Model Target | Delegation Signature |
|---|---|---|
| `gale_k_mutex_lock_decide` | `mutex::lock_decide(lock_count, owner_is_null, owner_is_current, is_no_wait) -> LockDecision` | New standalone fn |
| `gale_k_mutex_unlock_decide` | `mutex::unlock_decide(lock_count, owner_is_null, owner_is_current) -> UnlockDecision` | New standalone fn |
| `gale_k_msgq_put_decide` | `msgq::put_decide(write_idx, used_msgs, max_msgs, has_waiter, is_no_wait) -> PutDecision` | New standalone fn |
| `gale_k_msgq_get_decide` | `msgq::get_decide(read_idx, used_msgs, max_msgs, has_waiter, is_no_wait) -> GetDecision` | New standalone fn |
| `gale_k_pipe_write_decide` | `pipe::write_decide(used, size, flags, request_len, has_reader) -> WriteDecision` | New standalone fn |
| `gale_k_pipe_read_decide` | `pipe::read_decide(used, size, flags, request_len, has_writer) -> ReadDecision` | New standalone fn |
| `gale_k_stack_push_decide` | `stack::push_decide(count, capacity, has_waiter) -> PushDecision` | New standalone fn |
| `gale_k_stack_pop_decide` | `stack::pop_decide(count, is_no_wait) -> PopDecision` | New standalone fn |
| `gale_k_timer_expire_decide` | `timer::expire_decide(status, period) -> ExpireDecision` | New standalone fn |
| `gale_k_mem_slab_alloc_decide` | `mem_slab::alloc_decide(num_used, num_blocks, is_no_wait) -> AllocDecision` | New standalone fn |
| `gale_k_mem_slab_free_decide` | `mem_slab::free_decide(num_used, has_waiter) -> FreeDecision` | New standalone fn |
| `gale_k_event_wait_decide` | `event::wait_decide(events, desired, wait_type, is_no_wait) -> WaitDecision` | New standalone fn |
| `gale_k_fifo_get_decide` | `fifo::get_decide(count, is_no_wait) -> GetDecision` | New standalone fn |
| `gale_k_lifo_get_decide` | `lifo::get_decide(count, is_no_wait) -> GetDecision` | New standalone fn |
| `gale_k_work_submit_decide` | `work::submit_decide(flags, is_queued, is_running) -> SubmitDecision` | New standalone fn |
| `gale_k_work_cancel_decide` | `work::cancel_decide(flags, is_queued, is_running) -> CancelDecision` | New standalone fn |
| `gale_k_fatal_decide` | `fatal::classify_decide(reason, is_isr, test_mode) -> FatalDecision` | New standalone fn |
| `gale_k_timeslice_tick_decide` | `timeslice::tick_decide(ticks_remaining, slice_ticks, is_cooperative) -> TickDecision` | New standalone fn |
| `gale_k_sched_next_up_decide` | `sched::next_up_decide(has_runq, runq_is_metairq, has_preempted, preempted_ready) -> NextUpDecision` | New standalone fn |
| `gale_k_sched_preempt_decide` | `sched::preempt_decide(is_coop, is_metairq, swap_ok, is_prevented) -> PreemptDecision` | New standalone fn |
| `gale_k_thread_create_decide` | `thread_lifecycle::create_decide(stack_size, priority, active_count) -> CreateDecision` | New standalone fn |
| `gale_k_object_validate_decide` | `userspace::validate_decide(obj_type, expected, flags, has_access, init_check) -> ValidateDecision` | New standalone fn |
| `gale_k_mem_domain_add_partition_decide` | `mem_domain::add_decide(part_start, part_size, domain_starts, domain_sizes, num) -> AddDecision` | New standalone fn |
| `gale_k_mem_domain_remove_partition_decide` | `mem_domain::remove_decide(part_start, part_size, domain_starts, domain_sizes, num) -> RemoveDecision` | New standalone fn |
| `gale_dynamic_alloc_decide` | `dynamic::alloc_decide(active, max_threads) -> AllocDecision` | New standalone fn |
| `gale_dynamic_free_decide` | `dynamic::free_decide(active) -> FreeDecision` | New standalone fn |
| `gale_smp_start_cpu_decide` | `smp_state::start_decide(active_cpus, max_cpus) -> StartDecision` | New standalone fn |
| `gale_smp_stop_cpu_decide` | `smp_state::stop_decide(active_cpus) -> StopDecision` | New standalone fn |
| `gale_sys_heap_alloc_decide` | `heap::alloc_decide(found, found_sz, needed_sz) -> AllocDecision` | New standalone fn |
| `gale_sys_heap_free_decide` | `heap::free_decide(is_used, right_free, left_free, bounds_ok) -> FreeDecision` | New standalone fn |
| `gale_sys_heap_aligned_alloc_decide` | `heap::aligned_alloc_decide(bytes, align, header_bytes) -> AlignedDecision` | New standalone fn |
| `gale_sys_heap_realloc_decide` | `heap::realloc_decide(cur_sz, needed_sz, right_free, right_sz) -> ReallocDecision` | New standalone fn |
| `gale_ring_buf_claim_decide` | `ring_buf::claim_decide(head, base, buf_size, requested) -> ClaimDecision` | New standalone fn |

### Priority 2: Legacy validate functions

These can be turned into thin wrappers around the Priority 1 decision functions
once those are wired up, exactly as `gale_timeout_add` already wraps
`gale_timeout_add_decide`.

| RED FFI Function | Wrap Target |
|---|---|
| `gale_sem_count_give` | `gale_k_sem_give_decide` (already GREEN) |
| `gale_sem_count_take` | `gale_k_sem_take_decide` (already GREEN) |
| `gale_mutex_lock_validate` | `gale_k_mutex_lock_decide` (after Priority 1) |
| `gale_mutex_unlock_validate` | `gale_k_mutex_unlock_decide` (after Priority 1) |
| `gale_msgq_put` | `gale_k_msgq_put_decide` (after Priority 1) |
| `gale_msgq_put_front` | New `gale_k_msgq_put_front_decide` |
| `gale_msgq_get` | `gale_k_msgq_get_decide` (after Priority 1) |
| `gale_msgq_peek_at` | New `gale_k_msgq_peek_decide` |
| `gale_pipe_write_check` | `gale_k_pipe_write_decide` (after Priority 1) |
| `gale_pipe_read_check` | `gale_k_pipe_read_decide` (after Priority 1) |
| `gale_stack_push_validate` | `gale_k_stack_push_decide` (after Priority 1) |
| `gale_stack_pop_validate` | `gale_k_stack_pop_decide` (after Priority 1) |
| `gale_timer_expire` | `gale_k_timer_expire_decide` (after Priority 1) |
| `gale_mem_slab_alloc_validate` | `gale_k_mem_slab_alloc_decide` (after Priority 1) |
| `gale_mem_slab_free_validate` | `gale_k_mem_slab_free_decide` (after Priority 1) |
| `gale_fifo_put_validate` | Already trivial but contains arithmetic -- wrap fifo put_decide |
| `gale_fifo_get_validate` | Wrap `gale_k_fifo_get_decide` |
| `gale_lifo_put_validate` | Wrap lifo put_decide |
| `gale_lifo_get_validate` | Wrap `gale_k_lifo_get_decide` |
| `gale_queue_append_validate` | Wrap queue insert_decide |
| `gale_queue_prepend_validate` | Wrap queue insert_decide |
| `gale_queue_get_validate` | Wrap `gale_k_queue_get_decide` |
| `gale_futex_wake` | Wrap `gale_k_futex_wake_decide` |
| `gale_timeslice_tick` | Wrap `gale_k_timeslice_tick_decide` |
| `gale_kheap_alloc_validate` | Wrap kheap model |
| `gale_kheap_free_validate` | Wrap kheap model |
| `gale_thread_create_validate` | Wrap `gale_k_thread_create_decide` |
| `gale_thread_exit_validate` | Wrap thread exit model |
| `gale_mempool_alloc_validate` | Wrap mempool model |
| `gale_mempool_free_validate` | Wrap mempool model |
| `gale_dynamic_alloc_validate` | Wrap `gale_dynamic_alloc_decide` |
| `gale_dynamic_free_validate` | Wrap `gale_dynamic_free_decide` |
| `gale_smp_start_cpu_validate` | Wrap `gale_smp_start_cpu_decide` |
| `gale_smp_stop_cpu_validate` | Wrap `gale_smp_stop_cpu_decide` |
| `gale_mem_domain_check_partition` | Wrap `gale_k_mem_domain_add_partition_decide` |
| `gale_sys_heap_split_validate` | Wrap heap split model |
| `gale_sys_heap_merge_validate` | Wrap heap merge model |
| `gale_timeout_add_decide` | Add model delegation (currently inline u64 arithmetic) |
| `gale_timeout_announce_decide` | Add model delegation (currently inline u64 arithmetic) |

### Delegation pattern example

For each RED decision function, the delegation looks like this (using mutex as
example):

```rust
// In src/mutex.rs (Verus-verified):
pub fn lock_decide(
    lock_count: u32,
    owner_is_null: bool,
    owner_is_current: bool,
    is_no_wait: bool,
) -> (result: LockDecision)
    ensures
        owner_is_null ==> result == LockDecision::Acquire,
        // ... full spec
{ /* verified implementation */ }

// In ffi/src/lib.rs (thin shim):
pub extern "C" fn gale_k_mutex_lock_decide(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    is_no_wait: u32,
) -> GaleMutexLockDecision {
    use gale::mutex::{LockDecision, lock_decide};
    let d = lock_decide(lock_count, owner_is_null != 0, owner_is_current != 0, is_no_wait != 0);
    match d {
        LockDecision::Acquire => GaleMutexLockDecision { ... },
        LockDecision::Reentrant(n) => GaleMutexLockDecision { ... },
        LockDecision::Busy => GaleMutexLockDecision { ... },
        LockDecision::Pend => GaleMutexLockDecision { ... },
    }
}
```

No WaitQueue or heap allocation required. Each standalone `*_decide` function
takes only scalar inputs and returns an enum. The Verus proof covers the logic;
the FFI shim only translates types.

---

## Risk Assessment

The 60 RED functions represent 51.7% of all FFI entry points. While many of them
implement logic that is structurally identical to the verified model, the
duplication means:

1. **Proof coverage gap**: Verus proofs cover `src/*.rs` but the actually-executed
   code in `ffi/src/lib.rs` is not proven. The proofs cover the intent, not the
   implementation.

2. **Maintenance divergence**: Any bug fix or behavior change must be made in two
   places. If only the model is updated, the FFI continues running the old logic.

3. **ASIL-D concern**: For ISO 26262 ASIL-D, the gap between verified model and
   executed code is a qualification risk. The two GREEN functions demonstrate the
   correct pattern; the 60 RED functions need the same treatment.

The 54 YELLOW functions are acceptable as-is. Their logic is trivially correct
(single comparison, constant return, bitwise operation) and delegating them would
add complexity without meaningful safety benefit.
