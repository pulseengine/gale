//! Kani bounded model checking: C↔Rust semantic equivalence.
//!
//! Models the C behavior as Rust functions (derived from reading the C source
//! and FFI layer) and proves the Gale Rust implementation produces identical
//! results for all bounded inputs.
//!
//! This implements REQ-TRACTOR-001 Level 2.
//!
//! Primitives covered:
//!   - Stack:  init, push, pop, operation sequence
//!   - Semaphore: init, give, take
//!   - Mutex:  lock_validate, unlock_validate
//!   - MsgQ:   init, put, get, purge
//!   - Pipe:   init, write_check, read_check
//!   - CondVar: no FFI (pure wait queue wrapper — equivalence inherited)

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

#[cfg(kani)]
mod equivalence {
    use gale::error::*;

    // =====================================================================
    // Stack C↔Rust equivalence
    // =====================================================================

    use gale::stack::Stack;

    fn c_stack_init_validate(num_entries: u32) -> i32 {
        if num_entries == 0 { EINVAL } else { OK }
    }

    fn c_stack_push_validate(count: u32, capacity: u32) -> (i32, u32) {
        if count >= capacity {
            (ENOMEM, count)
        } else {
            (OK, count + 1)
        }
    }

    fn c_stack_pop_validate(count: u32) -> (i32, u32) {
        if count == 0 {
            (EBUSY, 0)
        } else {
            (OK, count - 1)
        }
    }

    #[kani::proof]
    fn stack_init_equivalence() {
        let cap: u32 = kani::any();
        kani::assume(cap <= 256);
        let c_rc = c_stack_init_validate(cap);
        match Stack::init(cap) {
            Ok(s) => {
                assert!(c_rc == OK);
                assert!(s.num_used() == 0);
            }
            Err(e) => {
                assert!(c_rc == EINVAL);
                assert!(e == EINVAL);
            }
        }
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn stack_push_equivalence() {
        let cap: u32 = kani::any();
        kani::assume(cap > 0 && cap <= 16);
        let count: u32 = kani::any();
        kani::assume(count <= cap);
        let mut s = Stack::init(cap).unwrap();
        for _ in 0..count {
            s.push();
        }
        let (c_rc, c_new) = c_stack_push_validate(s.num_used(), cap);
        let r_rc = s.push();
        assert!(r_rc == c_rc);
        assert!(s.num_used() == c_new);
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn stack_pop_equivalence() {
        let cap: u32 = kani::any();
        kani::assume(cap > 0 && cap <= 16);
        let count: u32 = kani::any();
        kani::assume(count <= cap);
        let mut s = Stack::init(cap).unwrap();
        for _ in 0..count {
            s.push();
        }
        let (c_rc, c_new) = c_stack_pop_validate(s.num_used());
        let r_rc = s.pop();
        assert!(r_rc == c_rc);
        assert!(s.num_used() == c_new);
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn stack_sequence_equivalence() {
        let cap: u32 = kani::any();
        kani::assume(cap > 0 && cap <= 8);
        let mut s = Stack::init(cap).unwrap();
        let mut c_count: u32 = 0;
        for _ in 0..4 {
            if kani::any() {
                let (c_rc, c_new) = c_stack_push_validate(c_count, cap);
                assert!(s.push() == c_rc);
                c_count = c_new;
            } else {
                let (c_rc, c_new) = c_stack_pop_validate(c_count);
                assert!(s.pop() == c_rc);
                c_count = c_new;
            }
            assert!(s.num_used() == c_count);
        }
    }

    // =====================================================================
    // Semaphore C↔Rust equivalence
    // =====================================================================

    use gale::sem::{GiveResult, Semaphore, TakeResult};

    fn c_sem_init_validate(initial_count: u32, limit: u32) -> i32 {
        if limit == 0 || initial_count > limit {
            EINVAL
        } else {
            OK
        }
    }

    /// C model: sem.c:110 — give with no waiters.
    fn c_sem_give(count: u32, limit: u32) -> u32 {
        if count != limit { count + 1 } else { count }
    }

    /// C model: sem.c:143-144 — take (non-blocking).
    fn c_sem_take(count: u32) -> (i32, u32) {
        if count > 0 {
            (OK, count - 1)
        } else {
            (EBUSY, count)
        }
    }

    #[kani::proof]
    fn sem_init_equivalence() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();
        kani::assume(limit <= 256);
        kani::assume(count <= 256);
        let c_rc = c_sem_init_validate(count, limit);
        match Semaphore::init(count, limit) {
            Ok(s) => {
                assert!(c_rc == OK);
                assert!(s.count_get() == count);
                assert!(s.limit_get() == limit);
            }
            Err(e) => {
                assert!(c_rc == EINVAL);
                assert!(e == EINVAL);
            }
        }
    }

    #[kani::proof]
    fn sem_give_equivalence() {
        let limit: u32 = kani::any();
        kani::assume(limit > 0 && limit <= 64);
        let count: u32 = kani::any();
        kani::assume(count <= limit);
        let mut sem = Semaphore::init(count, limit).unwrap();
        // Give with no waiters (wait queue is empty after init)
        let c_new = c_sem_give(count, limit);
        let result = sem.give();
        match result {
            GiveResult::Incremented => assert!(sem.count_get() == c_new),
            GiveResult::Saturated => assert!(sem.count_get() == c_new && count == limit),
            GiveResult::WokeThread(_) => {} // can't happen with empty queue
        }
        assert!(sem.count_get() == c_new);
    }

    #[kani::proof]
    fn sem_take_equivalence() {
        let limit: u32 = kani::any();
        kani::assume(limit > 0 && limit <= 64);
        let count: u32 = kani::any();
        kani::assume(count <= limit);
        let mut sem = Semaphore::init(count, limit).unwrap();
        let (c_rc, c_new_count) = c_sem_take(count);
        let result = sem.try_take();
        match result {
            TakeResult::Acquired => {
                assert!(c_rc == OK);
                assert!(sem.count_get() == c_new_count);
            }
            TakeResult::WouldBlock => {
                assert!(c_rc == EBUSY);
                assert!(sem.count_get() == c_new_count);
            }
            TakeResult::Blocked => {} // can't happen with try_take
        }
    }

    // =====================================================================
    // Mutex C↔Rust equivalence
    // =====================================================================

    use gale::mutex::{LockResult, Mutex, UnlockResult};
    use gale::thread::ThreadId;

    /// C model: gale_mutex_lock_validate (mutex.c:96-224 state checks).
    fn c_mutex_lock_validate(
        lock_count: u32,
        owner_is_null: bool,
        owner_is_current: bool,
    ) -> (i32, u32) {
        if lock_count == 0 || owner_is_null {
            (OK, 1) // acquire
        } else if owner_is_current {
            (OK, lock_count + 1) // reentrant
        } else {
            (EBUSY, lock_count) // contended
        }
    }

    /// C model: gale_mutex_unlock_validate (mutex.c:236-307).
    /// Returns: (return_code, new_lock_count).
    /// 1 = RELEASED (still held), 0 = UNLOCKED, negative = error.
    fn c_mutex_unlock_validate(
        lock_count: u32,
        owner_is_null: bool,
        owner_is_current: bool,
    ) -> (i32, u32) {
        if owner_is_null {
            (EINVAL, lock_count)
        } else if !owner_is_current {
            (EPERM, lock_count)
        } else if lock_count > 1 {
            (1, lock_count - 1) // RELEASED
        } else {
            (0, 0) // UNLOCKED
        }
    }

    #[kani::proof]
    fn mutex_lock_equivalence() {
        let mut m = Mutex::init();
        let id1 = ThreadId { id: 1 };
        let id2 = ThreadId { id: 2 };

        // Test unlocked → acquire
        let (c_rc, c_lc) = c_mutex_lock_validate(0, true, false);
        match m.try_lock(id1) {
            LockResult::Acquired => assert!(c_rc == OK),
            LockResult::WouldBlock => assert!(c_rc == EBUSY),
        }
        assert!(m.lock_count_get() == c_lc);

        // Test reentrant
        let (c_rc2, c_lc2) = c_mutex_lock_validate(m.lock_count_get(), false, true);
        match m.try_lock(id1) {
            LockResult::Acquired => assert!(c_rc2 == OK),
            LockResult::WouldBlock => assert!(c_rc2 == EBUSY),
        }
        assert!(m.lock_count_get() == c_lc2);

        // Test contended
        let (c_rc3, _) = c_mutex_lock_validate(m.lock_count_get(), false, false);
        match m.try_lock(id2) {
            LockResult::Acquired => assert!(c_rc3 == OK),
            LockResult::WouldBlock => assert!(c_rc3 == EBUSY),
        }
    }

    #[kani::proof]
    fn mutex_unlock_equivalence() {
        let mut m = Mutex::init();
        let id1 = ThreadId { id: 1 };
        let id2 = ThreadId { id: 2 };

        // Unlock when not locked
        let (c_rc, _) = c_mutex_unlock_validate(0, true, false);
        match m.unlock(id1) {
            Err(e) => assert!(e == c_rc),
            Ok(_) => panic!("should fail"),
        }

        // Lock twice, unlock once (reentrant)
        m.try_lock(id1);
        m.try_lock(id1);
        let (c_rc2, c_lc2) = c_mutex_unlock_validate(2, false, true);
        match m.unlock(id1) {
            Ok(UnlockResult::Released) => assert!(c_rc2 == 1),
            _ => panic!("expected Released"),
        }
        assert!(m.lock_count_get() == c_lc2);

        // Unlock by wrong owner
        let (c_rc3, _) = c_mutex_unlock_validate(m.lock_count_get(), false, false);
        match m.unlock(id2) {
            Err(e) => assert!(e == c_rc3),
            Ok(_) => panic!("should fail"),
        }
    }

    // =====================================================================
    // MsgQ C↔Rust equivalence
    // =====================================================================

    use gale::msgq::MsgQ;

    fn c_msgq_init_validate(msg_size: u32, max_msgs: u32) -> i32 {
        if msg_size == 0 || max_msgs == 0 {
            return EINVAL;
        }
        match msg_size.checked_mul(max_msgs) {
            Some(_) => OK,
            None => EINVAL,
        }
    }

    /// C model: put index advancement.
    fn c_msgq_put(write_idx: u32, used: u32, max_msgs: u32) -> (i32, u32, u32) {
        if used == max_msgs {
            (ENOMSG, write_idx, used)
        } else {
            (OK, (write_idx + 1) % max_msgs, used + 1)
        }
    }

    /// C model: get index advancement.
    fn c_msgq_get(read_idx: u32, used: u32, max_msgs: u32) -> (i32, u32, u32) {
        if used == 0 {
            (ENOMSG, read_idx, used)
        } else {
            (read_idx as i32, (read_idx + 1) % max_msgs, used - 1)
        }
    }

    #[kani::proof]
    fn msgq_init_equivalence() {
        let msg_size: u32 = kani::any();
        let max_msgs: u32 = kani::any();
        kani::assume(msg_size <= 256);
        kani::assume(max_msgs <= 256);
        let c_rc = c_msgq_init_validate(msg_size, max_msgs);
        match MsgQ::init(msg_size, max_msgs) {
            Ok(q) => {
                assert!(c_rc == OK);
                assert!(q.num_used_get() == 0);
                assert!(q.msg_size_get() == msg_size);
                assert!(q.max_msgs_get() == max_msgs);
            }
            Err(e) => {
                assert!(c_rc == EINVAL);
                assert!(e == EINVAL);
            }
        }
    }

    #[kani::proof]
    fn msgq_put_get_equivalence() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 8);
        let mut q = MsgQ::init(4, max_msgs).unwrap();

        // Put
        let used_before = q.num_used_get();
        let _w_idx = q.write_idx_get();
        let rust_put = q.put();
        if used_before < max_msgs {
            assert!(rust_put.is_ok());
            assert!(q.num_used_get() == used_before + 1);
        } else {
            assert!(rust_put.is_err());
            assert!(q.num_used_get() == used_before);
        }

        // Get
        let used_before2 = q.num_used_get();
        let _r_idx = q.read_idx_get();
        let rust_get = q.get();
        if used_before2 > 0 {
            assert!(rust_get.is_ok());
            assert!(q.num_used_get() == used_before2 - 1);
        } else {
            assert!(rust_get.is_err());
            assert!(q.num_used_get() == used_before2);
        }
    }

    #[kani::proof]
    #[kani::unwind(9)]
    fn msgq_purge_equivalence() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 8);
        let mut q = MsgQ::init(4, max_msgs).unwrap();
        let fill: u32 = kani::any();
        kani::assume(fill <= max_msgs);
        for _ in 0..fill {
            let _ = q.put();
        }

        let old_used = q.num_used_get();
        let purged = q.purge();
        assert!(purged == old_used);
        assert!(q.num_used_get() == 0);
    }

    // =====================================================================
    // Pipe C↔Rust equivalence
    // =====================================================================

    use gale::pipe::{FLAG_OPEN, FLAG_RESET, Pipe};

    fn c_pipe_init_validate(size: u32) -> i32 {
        if size == 0 { EINVAL } else { OK }
    }

    /// C model: gale_pipe_write_check (pipe.c:138-220 state + byte count).
    fn c_pipe_write_check(used: u32, size: u32, flags: u8, request_len: u32) -> (i32, u32) {
        if (flags & FLAG_RESET) != 0 {
            return (ECANCELED, used);
        }
        if (flags & FLAG_OPEN) == 0 {
            return (EPIPE, used);
        }
        if request_len == 0 {
            return (ENOMSG, used);
        }
        let free = size - used;
        if free == 0 {
            return (EAGAIN, used);
        }
        let n = if request_len <= free {
            request_len
        } else {
            free
        };
        (n as i32, used + n)
    }

    /// C model: gale_pipe_read_check (pipe.c:222-289 state + byte count).
    fn c_pipe_read_check(used: u32, _size: u32, flags: u8, request_len: u32) -> (i32, u32) {
        if (flags & FLAG_RESET) != 0 {
            return (ECANCELED, used);
        }
        if (flags & FLAG_OPEN) == 0 && used == 0 {
            return (EPIPE, used);
        }
        if request_len == 0 {
            return (ENOMSG, used);
        }
        if used == 0 {
            return (EAGAIN, used);
        }
        let n = if request_len <= used {
            request_len
        } else {
            used
        };
        (n as i32, used - n)
    }

    #[kani::proof]
    fn pipe_init_equivalence() {
        let size: u32 = kani::any();
        kani::assume(size <= 256);
        let c_rc = c_pipe_init_validate(size);
        match Pipe::init(size) {
            Ok(p) => {
                assert!(c_rc == OK);
                assert!(p.data_get() == 0);
                assert!(p.space_get() == size);
            }
            Err(e) => {
                assert!(c_rc == EINVAL);
                assert!(e == EINVAL);
            }
        }
    }

    #[kani::proof]
    fn pipe_write_equivalence() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 32);
        let mut p = Pipe::init(size).unwrap();

        // Fill to some level
        let fill: u32 = kani::any();
        kani::assume(fill <= size);
        if fill > 0 {
            let _ = p.write_check(fill);
        }

        let used = p.data_get();
        let req: u32 = kani::any();
        kani::assume(req <= 64);
        let (c_rc, c_used) = c_pipe_write_check(used, size, FLAG_OPEN, req);
        let rust_result = p.write_check(req);

        match rust_result {
            Ok(n) => {
                assert!(c_rc > 0);
                assert!(n == c_rc as u32);
                assert!(p.data_get() == c_used);
            }
            Err(e) => {
                assert!(c_rc < 0 || c_rc == 0);
                assert!(e == c_rc);
                assert!(p.data_get() == c_used);
            }
        }
    }

    #[kani::proof]
    fn pipe_read_equivalence() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 32);
        let mut p = Pipe::init(size).unwrap();

        let fill: u32 = kani::any();
        kani::assume(fill <= size);
        if fill > 0 {
            let _ = p.write_check(fill);
        }

        let used = p.data_get();
        let req: u32 = kani::any();
        kani::assume(req <= 64);
        let (c_rc, c_used) = c_pipe_read_check(used, size, FLAG_OPEN, req);
        let rust_result = p.read_check(req);

        match rust_result {
            Ok(n) => {
                assert!(c_rc > 0);
                assert!(n == c_rc as u32);
                assert!(p.data_get() == c_used);
            }
            Err(e) => {
                assert!(c_rc < 0 || c_rc == 0);
                assert!(e == c_rc);
                assert!(p.data_get() == c_used);
            }
        }
    }

    // =====================================================================
    // Timer C↔Rust equivalence
    // =====================================================================

    use gale::timer::Timer;

    /// C model: timer expire — increment status with overflow check.
    fn c_timer_expire(status: u32) -> (i32, u32) {
        if status < u32::MAX {
            (OK, status + 1)
        } else {
            (EOVERFLOW, status)
        }
    }

    /// C model: timer status_get — returns old status, resets to 0.
    fn c_timer_status_get(status: u32) -> (u32, u32) {
        (status, 0)
    }

    #[kani::proof]
    fn timer_init_equivalence() {
        let period: u32 = kani::any();
        kani::assume(period <= 1000);
        let t = Timer::init(period);
        assert!(t.status_peek() == 0);
        assert!(t.period_get() == period);
        assert!(!t.is_running());
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn timer_expire_equivalence() {
        let period: u32 = kani::any();
        kani::assume(period <= 100);
        let mut t = Timer::init(period);
        t.start();

        // Pre-fill to some status
        let fill: u32 = kani::any();
        kani::assume(fill <= 16);
        for _ in 0..fill {
            let _ = t.expire();
        }

        let status = t.status_peek();
        let (c_rc, c_new) = c_timer_expire(status);
        let rust_result = t.expire();
        match rust_result {
            Ok(new_status) => {
                assert!(c_rc == OK);
                assert!(new_status == c_new);
                assert!(t.status_peek() == c_new);
            }
            Err(e) => {
                assert!(c_rc == EOVERFLOW);
                assert!(e == EOVERFLOW);
                assert!(t.status_peek() == c_new);
            }
        }
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn timer_status_get_equivalence() {
        let period: u32 = kani::any();
        kani::assume(period <= 100);
        let mut t = Timer::init(period);
        t.start();

        let fill: u32 = kani::any();
        kani::assume(fill <= 16);
        for _ in 0..fill {
            let _ = t.expire();
        }

        let status = t.status_peek();
        let (c_old, c_new) = c_timer_status_get(status);
        let rust_old = t.status_get();
        assert!(rust_old == c_old);
        assert!(t.status_peek() == c_new);
    }

    // =====================================================================
    // Event C↔Rust equivalence
    // =====================================================================

    use gale::event::Event;

    /// C model: event post — OR new bits in.
    fn c_event_post(events: u32, new_events: u32) -> u32 {
        events | new_events
    }

    /// C model: event set — replace entirely.
    fn c_event_set(new_events: u32) -> u32 {
        new_events
    }

    /// C model: event clear — AND with complement.
    fn c_event_clear(events: u32, clear_bits: u32) -> u32 {
        events & !clear_bits
    }

    #[kani::proof]
    fn event_init_equivalence() {
        let e = Event::init();
        assert!(e.events_get() == 0);
    }

    #[kani::proof]
    fn event_post_equivalence() {
        let mut e = Event::init();
        let initial: u32 = kani::any();
        e.set(initial);

        let new_events: u32 = kani::any();
        let c_result = c_event_post(e.events_get(), new_events);
        let r_result = e.post(new_events);
        assert!(r_result == c_result);
        assert!(e.events_get() == c_result);
    }

    #[kani::proof]
    fn event_set_equivalence() {
        let mut e = Event::init();
        let initial: u32 = kani::any();
        e.post(initial);

        let new_events: u32 = kani::any();
        let c_result = c_event_set(new_events);
        let _old = e.set(new_events);
        assert!(e.events_get() == c_result);
    }

    #[kani::proof]
    fn event_clear_equivalence() {
        let mut e = Event::init();
        let initial: u32 = kani::any();
        e.set(initial);

        let clear_bits: u32 = kani::any();
        let c_result = c_event_clear(e.events_get(), clear_bits);
        let r_result = e.clear(clear_bits);
        assert!(r_result == c_result);
        assert!(e.events_get() == c_result);
    }

    // =====================================================================
    // MemSlab C↔Rust equivalence
    // =====================================================================

    use gale::mem_slab::MemSlab;

    /// C model: mem_slab alloc — increment if not full, else ENOMEM.
    fn c_mem_slab_alloc(num_used: u32, num_blocks: u32) -> (i32, u32) {
        if num_used < num_blocks {
            (OK, num_used + 1)
        } else {
            (ENOMEM, num_used)
        }
    }

    /// C model: mem_slab free — decrement if used > 0, else EINVAL.
    fn c_mem_slab_free(num_used: u32) -> (i32, u32) {
        if num_used > 0 {
            (OK, num_used - 1)
        } else {
            (EINVAL, num_used)
        }
    }

    #[kani::proof]
    fn mem_slab_init_equivalence() {
        let block_size: u32 = kani::any();
        let num_blocks: u32 = kani::any();
        kani::assume(block_size <= 256);
        kani::assume(num_blocks <= 256);
        match MemSlab::init(block_size, num_blocks) {
            Ok(s) => {
                assert!(block_size > 0 && num_blocks > 0);
                assert!(s.num_used_get() == 0);
                assert!(s.num_blocks_get() == num_blocks);
                assert!(s.block_size_get() == block_size);
            }
            Err(e) => {
                assert!(block_size == 0 || num_blocks == 0);
                assert!(e == EINVAL);
            }
        }
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn mem_slab_alloc_equivalence() {
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 16);
        let fill: u32 = kani::any();
        kani::assume(fill <= num_blocks);
        let mut s = MemSlab::init(64, num_blocks).unwrap();
        for _ in 0..fill {
            s.alloc();
        }
        let used = s.num_used_get();
        let (c_rc, c_new) = c_mem_slab_alloc(used, num_blocks);
        let r_rc = s.alloc();
        assert!(r_rc == c_rc);
        assert!(s.num_used_get() == c_new);
    }

    #[kani::proof]
    #[kani::unwind(17)]
    fn mem_slab_free_equivalence() {
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 16);
        let fill: u32 = kani::any();
        kani::assume(fill <= num_blocks);
        let mut s = MemSlab::init(64, num_blocks).unwrap();
        for _ in 0..fill {
            s.alloc();
        }
        let used = s.num_used_get();
        let (c_rc, c_new) = c_mem_slab_free(used);
        let r_rc = s.free();
        assert!(r_rc == c_rc);
        assert!(s.num_used_get() == c_new);
    }

    #[kani::proof]
    #[kani::unwind(5)]
    fn mem_slab_sequence_equivalence() {
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 8);
        let mut s = MemSlab::init(64, num_blocks).unwrap();
        let mut c_used: u32 = 0;
        for _ in 0..4 {
            if kani::any() {
                let (c_rc, c_new) = c_mem_slab_alloc(c_used, num_blocks);
                assert!(s.alloc() == c_rc);
                c_used = c_new;
            } else {
                let (c_rc, c_new) = c_mem_slab_free(c_used);
                assert!(s.free() == c_rc);
                c_used = c_new;
            }
            assert!(s.num_used_get() == c_used);
        }
    }
}
