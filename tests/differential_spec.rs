//! Differential specification testing — REQ-TRACTOR-003.
//!
//! Verifies that Gale's ASIL-D properties are not just Zephyr-specific
//! but hold as universal properties of the abstract data types. Tests the
//! same specifications against independent reference models derived from
//! POSIX/FreeRTOS semantics.
//!
//! If a property passes on Gale but FAILS on a reference model, the spec
//! may be wrong (derived from a C bug). If it passes on both, the spec
//! is independently validated.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::error::*;

// =====================================================================
// Reference model: POSIX/FreeRTOS counting semaphore
// =====================================================================
//
// Independent implementation from POSIX sem_init/sem_post/sem_wait semantics
// and FreeRTOS xSemaphoreCreateCounting/xSemaphoreGive/xSemaphoreTake.

mod posix_sem {
    /// POSIX/FreeRTOS counting semaphore model.
    pub struct Semaphore {
        pub count: u32,
        pub limit: u32,
    }

    impl Semaphore {
        pub fn init(count: u32, limit: u32) -> Result<Self, ()> {
            // POSIX: sem_init with value; FreeRTOS: xSemaphoreCreateCounting(max, initial)
            if limit == 0 || count > limit {
                Err(())
            } else {
                Ok(Semaphore { count, limit })
            }
        }

        /// POSIX: sem_post / FreeRTOS: xSemaphoreGive
        pub fn give(&mut self) -> bool {
            if self.count < self.limit {
                self.count += 1;
                true // incremented
            } else {
                false // saturated
            }
        }

        /// POSIX: sem_trywait / FreeRTOS: xSemaphoreTake(0)
        pub fn try_take(&mut self) -> bool {
            if self.count > 0 {
                self.count -= 1;
                true // acquired
            } else {
                false // would block
            }
        }

        pub fn reset(&mut self) {
            self.count = 0;
        }
    }
}

// =====================================================================
// Reference model: POSIX/FreeRTOS recursive mutex
// =====================================================================

mod posix_mutex {
    pub struct Mutex {
        pub owner: Option<u32>,
        pub lock_count: u32,
    }

    impl Mutex {
        pub fn init() -> Self {
            Mutex {
                owner: None,
                lock_count: 0,
            }
        }

        /// pthread_mutex_lock (PTHREAD_MUTEX_RECURSIVE) / xSemaphoreTakeRecursive
        pub fn try_lock(&mut self, thread_id: u32) -> bool {
            match self.owner {
                None => {
                    self.owner = Some(thread_id);
                    self.lock_count = 1;
                    true
                }
                Some(id) if id == thread_id => {
                    self.lock_count += 1;
                    true
                }
                _ => false, // EBUSY
            }
        }

        /// pthread_mutex_unlock / xSemaphoreGiveRecursive
        pub fn unlock(&mut self, thread_id: u32) -> Result<bool, i32> {
            match self.owner {
                None => Err(-22),                       // EINVAL
                Some(id) if id != thread_id => Err(-1), // EPERM
                Some(_) => {
                    self.lock_count -= 1;
                    if self.lock_count == 0 {
                        self.owner = None;
                        Ok(false) // fully unlocked
                    } else {
                        Ok(true) // still held
                    }
                }
            }
        }
    }
}

// =====================================================================
// Reference model: bounded buffer / ring queue
// =====================================================================

mod posix_msgq {
    pub struct MsgQ {
        pub max_msgs: u32,
        pub used: u32,
    }

    impl MsgQ {
        pub fn init(max_msgs: u32) -> Result<Self, ()> {
            if max_msgs == 0 {
                Err(())
            } else {
                Ok(MsgQ { max_msgs, used: 0 })
            }
        }

        /// mq_send / xQueueSend
        pub fn put(&mut self) -> bool {
            if self.used < self.max_msgs {
                self.used += 1;
                true
            } else {
                false
            }
        }

        /// mq_receive / xQueueReceive
        pub fn get(&mut self) -> bool {
            if self.used > 0 {
                self.used -= 1;
                true
            } else {
                false
            }
        }

        pub fn purge(&mut self) -> u32 {
            let old = self.used;
            self.used = 0;
            old
        }
    }
}

// =====================================================================
// Reference model: bounded stack (LIFO)
// =====================================================================

mod posix_stack {
    pub struct Stack {
        pub capacity: u32,
        pub count: u32,
    }

    impl Stack {
        pub fn init(capacity: u32) -> Result<Self, ()> {
            if capacity == 0 {
                Err(())
            } else {
                Ok(Stack { capacity, count: 0 })
            }
        }

        pub fn push(&mut self) -> bool {
            if self.count < self.capacity {
                self.count += 1;
                true
            } else {
                false
            }
        }

        pub fn pop(&mut self) -> bool {
            if self.count > 0 {
                self.count -= 1;
                true
            } else {
                false
            }
        }
    }
}

// =====================================================================
// Reference model: POSIX timer_create/timer_settime (status counter)
// =====================================================================

mod posix_timer {
    /// POSIX timer model — status counter only.
    ///
    /// Based on POSIX timer_create/timer_settime/timer_getoverrun semantics.
    /// The overrun count is analogous to Zephyr's status counter: it tracks
    /// how many expiry events occurred since the last read.
    pub struct Timer {
        pub status: u32,
        pub running: bool,
    }

    impl Timer {
        pub fn init() -> Self {
            Timer {
                status: 0,
                running: false,
            }
        }

        /// timer_settime: arm the timer, reset overrun count.
        pub fn start(&mut self) {
            self.status = 0;
            self.running = true;
        }

        /// timer_delete / disarm: stop the timer, reset status.
        pub fn stop(&mut self) {
            self.status = 0;
            self.running = false;
        }

        /// Signal handler / expiry callback: increment overrun count.
        /// Returns Err on overflow (checked_add).
        pub fn expire(&mut self) -> Result<u32, i32> {
            if self.status == u32::MAX {
                Err(-75) // EOVERFLOW
            } else {
                self.status += 1;
                Ok(self.status)
            }
        }

        /// timer_getoverrun: read and reset the overrun/status counter.
        pub fn status_get(&mut self) -> u32 {
            let old = self.status;
            self.status = 0;
            old
        }
    }
}

// =====================================================================
// Reference model: FreeRTOS xEventGroupSetBits/WaitBits
// =====================================================================

mod freertos_event {
    /// FreeRTOS event group model — 32-bit bitmask.
    ///
    /// Based on xEventGroupCreate, xEventGroupSetBits,
    /// xEventGroupClearBits, xEventGroupWaitBits semantics.
    pub struct EventGroup {
        pub bits: u32,
    }

    impl EventGroup {
        pub fn init() -> Self {
            EventGroup { bits: 0 }
        }

        /// xEventGroupSetBits: OR new bits in.
        pub fn set_bits(&mut self, bits_to_set: u32) -> u32 {
            self.bits |= bits_to_set;
            self.bits
        }

        /// xEventGroupClearBits: AND with complement.
        pub fn clear_bits(&mut self, bits_to_clear: u32) -> u32 {
            self.bits &= !bits_to_clear;
            self.bits
        }

        /// xEventGroupWaitBits with xWaitForAllBits=false: any-bit match.
        pub fn wait_bits_any(&self, bits_to_wait: u32) -> bool {
            (self.bits & bits_to_wait) != 0
        }

        /// xEventGroupWaitBits with xWaitForAllBits=true: all-bits match.
        pub fn wait_bits_all(&self, bits_to_wait: u32) -> bool {
            (self.bits & bits_to_wait) == bits_to_wait
        }
    }
}

// =====================================================================
// Reference model: POSIX fixed-block memory pool
// =====================================================================

mod posix_mem_pool {
    /// POSIX-style fixed-block memory pool model.
    ///
    /// Based on a simplified version of POSIX shared memory with fixed-size
    /// block allocation (similar to VxWorks memPartAlloc/memPartFree or
    /// a simple pool allocator with fixed block sizes).
    pub struct MemPool {
        pub num_blocks: u32,
        pub num_used: u32,
    }

    impl MemPool {
        pub fn init(num_blocks: u32) -> Result<Self, ()> {
            if num_blocks == 0 {
                Err(())
            } else {
                Ok(MemPool {
                    num_blocks,
                    num_used: 0,
                })
            }
        }

        /// Allocate a block from the pool.
        pub fn alloc(&mut self) -> bool {
            if self.num_used < self.num_blocks {
                self.num_used += 1;
                true
            } else {
                false
            }
        }

        /// Free a block back to the pool.
        pub fn free(&mut self) -> bool {
            if self.num_used > 0 {
                self.num_used -= 1;
                true
            } else {
                false
            }
        }

        /// Number of free blocks.
        pub fn num_free(&self) -> u32 {
            self.num_blocks - self.num_used
        }
    }
}

// =====================================================================
// Differential property tests
// =====================================================================
//
// Each test exercises the SAME property on both Gale and the reference model.
// Properties are the ASIL-D specifications from requirements.yaml.

#[cfg(test)]
mod differential {
    use super::*;
    use gale::msgq::MsgQ;
    use gale::mutex::{LockResult, Mutex, UnlockResult};
    use gale::sem::{GiveResult, Semaphore, TakeResult};
    use gale::stack::Stack;
    use gale::thread::ThreadId;

    // ── Semaphore P1: 0 <= count <= limit (always) ──────────────────

    #[test]
    fn sem_p1_invariant_gale_and_posix() {
        for limit in 1..=10u32 {
            for init in 0..=limit {
                // Gale
                let mut g = Semaphore::init(init, limit).unwrap();
                // POSIX
                let mut p = posix_sem::Semaphore::init(init, limit).unwrap();

                for _ in 0..20 {
                    // Give
                    g.give();
                    p.give();
                    assert!(g.count_get() <= limit, "Gale P1 violated after give");
                    assert!(p.count <= p.limit, "POSIX P1 violated after give");
                    assert_eq!(g.count_get(), p.count, "Gale/POSIX diverge after give");

                    // Take
                    let g_took = matches!(g.try_take(), TakeResult::Acquired);
                    let p_took = p.try_take();
                    assert_eq!(g_took, p_took, "Gale/POSIX diverge on take result");
                    assert_eq!(g.count_get(), p.count, "Gale/POSIX diverge after take");
                }
            }
        }
    }

    // ── Semaphore P3: give increments when count < limit ─────────────

    #[test]
    fn sem_p3_give_increment_gale_and_posix() {
        let mut g = Semaphore::init(0, 5).unwrap();
        let mut p = posix_sem::Semaphore::init(0, 5).unwrap();

        for expected in 1..=5u32 {
            g.give();
            p.give();
            assert_eq!(g.count_get(), expected);
            assert_eq!(p.count, expected);
        }
        // Saturated
        g.give();
        p.give();
        assert_eq!(g.count_get(), 5);
        assert_eq!(p.count, 5);
    }

    // ── Semaphore P8: reset clears everything ────────────────────────

    #[test]
    fn sem_p8_reset_gale_and_posix() {
        let mut g = Semaphore::init(3, 10).unwrap();
        let mut p = posix_sem::Semaphore::init(3, 10).unwrap();
        g.reset();
        p.reset();
        assert_eq!(g.count_get(), 0);
        assert_eq!(p.count, 0);
    }

    // ── Mutex M1: owner ↔ lock_count correspondence ─────────────────

    #[test]
    fn mutex_m1_owner_correspondence_gale_and_posix() {
        let mut g = Mutex::init();
        let mut p = posix_mutex::Mutex::init();
        let id = ThreadId { id: 42 };

        // Both unlocked
        assert!(g.owner_get().is_none());
        assert!(p.owner.is_none());

        // Lock
        g.try_lock(id);
        p.try_lock(42);
        assert!(g.owner_get().is_some());
        assert!(p.owner.is_some());
        assert_eq!(g.lock_count_get(), 1);
        assert_eq!(p.lock_count, 1);

        // Reentrant
        g.try_lock(id);
        p.try_lock(42);
        assert_eq!(g.lock_count_get(), 2);
        assert_eq!(p.lock_count, 2);

        // Unlock once (still held)
        g.unlock(id).unwrap();
        p.unlock(42).unwrap();
        assert_eq!(g.lock_count_get(), 1);
        assert_eq!(p.lock_count, 1);

        // Unlock fully
        g.unlock(id).unwrap();
        p.unlock(42).unwrap();
        assert_eq!(g.lock_count_get(), 0);
        assert_eq!(p.lock_count, 0);
        assert!(g.owner_get().is_none());
        assert!(p.owner.is_none());
    }

    // ── MsgQ: put/get FIFO + conservation ────────────────────────────

    #[test]
    fn msgq_conservation_gale_and_posix() {
        let mut g = MsgQ::init(4, 8).unwrap();
        let mut p = posix_msgq::MsgQ::init(8).unwrap();

        // Fill
        for _ in 0..8 {
            let g_ok = g.put().is_ok();
            let p_ok = p.put();
            assert_eq!(g_ok, p_ok);
        }
        // Full
        assert!(g.put().is_err());
        assert!(!p.put());

        // Drain
        for _ in 0..8 {
            let g_ok = g.get().is_ok();
            let p_ok = p.get();
            assert_eq!(g_ok, p_ok);
        }
        // Empty
        assert!(g.get().is_err());
        assert!(!p.get());
    }

    // ── Stack: push/pop bounds ───────────────────────────────────────

    #[test]
    fn stack_bounds_gale_and_posix() {
        let mut g = Stack::init(4).unwrap();
        let mut p = posix_stack::Stack::init(4).unwrap();

        // Fill
        for _ in 0..4 {
            let g_ok = g.push() == OK;
            let p_ok = p.push();
            assert_eq!(g_ok, p_ok);
        }
        // Full
        assert_ne!(g.push(), OK);
        assert!(!p.push());

        // Drain
        for _ in 0..4 {
            let g_ok = g.pop() == OK;
            let p_ok = p.pop();
            assert_eq!(g_ok, p_ok);
        }
        // Empty
        assert_ne!(g.pop(), OK);
        assert!(!p.pop());
    }

    // ── Cross-implementation fuzz: random operations ─────────────────

    #[test]
    fn sem_random_ops_gale_matches_posix() {
        let mut g = Semaphore::init(0, 100).unwrap();
        let mut p = posix_sem::Semaphore::init(0, 100).unwrap();

        // Deterministic pseudo-random sequence
        let mut rng: u32 = 0xDEAD_BEEF;
        for _ in 0..1000 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            if rng % 3 == 0 {
                g.give();
                p.give();
            } else if rng % 3 == 1 {
                g.try_take();
                p.try_take();
            } else {
                g.reset();
                p.reset();
            }
            assert_eq!(
                g.count_get(),
                p.count,
                "Gale/POSIX sem diverged at iteration with rng={rng}"
            );
        }
    }

    #[test]
    fn stack_random_ops_gale_matches_posix() {
        let mut g = Stack::init(50).unwrap();
        let mut p = posix_stack::Stack::init(50).unwrap();

        let mut rng: u32 = 0xCAFE_BABE;
        for _ in 0..1000 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            if rng % 2 == 0 {
                g.push();
                p.push();
            } else {
                g.pop();
                p.pop();
            }
            assert_eq!(
                g.num_used(),
                p.count,
                "Gale/POSIX stack diverged at rng={rng}"
            );
        }
    }

    // ── Timer: expire status counter ────────────────────────────────────

    #[test]
    fn timer_expire_gale_and_posix() {
        use super::posix_timer;
        use gale::timer::Timer;

        let mut g = Timer::init(100);
        let mut p = posix_timer::Timer::init();

        g.start();
        p.start();

        // Expire 10 times
        for i in 1..=10u32 {
            let g_res = g.expire();
            let p_res = p.expire();
            assert!(g_res.is_ok(), "Gale timer expire failed at iteration {i}");
            assert!(p_res.is_ok(), "POSIX timer expire failed at iteration {i}");
            assert_eq!(
                g.status_peek(),
                p.status,
                "Gale/POSIX timer status diverged at iteration {i}"
            );
        }

        // status_get: read and reset
        let g_old = g.status_get();
        let p_old = p.status_get();
        assert_eq!(g_old, p_old, "Gale/POSIX timer status_get value diverged");
        assert_eq!(g.status_peek(), 0, "Gale timer not reset after status_get");
        assert_eq!(p.status, 0, "POSIX timer not reset after status_get");

        // Stop resets
        g.expire().unwrap();
        p.expire().unwrap();
        g.stop();
        p.stop();
        assert_eq!(g.status_peek(), 0);
        assert_eq!(p.status, 0);
        assert!(!g.is_running());
        assert!(!p.running);
    }

    // ── Event: bitmask operations ───────────────────────────────────────

    #[test]
    fn event_post_gale_and_posix() {
        use super::freertos_event;
        use gale::event::Event;

        let mut g = Event::init();
        let mut p = freertos_event::EventGroup::init();

        // Post bits
        g.post(0x01);
        p.set_bits(0x01);
        assert_eq!(g.events_get(), p.bits, "diverge after post 0x01");

        g.post(0x04);
        p.set_bits(0x04);
        assert_eq!(g.events_get(), p.bits, "diverge after post 0x04");

        // Post is monotonic — old bits preserved
        assert_eq!(g.events_get() & 0x01, 0x01);
        assert_eq!(p.bits & 0x01, 0x01);

        // wait_any
        assert_eq!(g.wait_check_any(0x01), p.wait_bits_any(0x01));
        assert_eq!(g.wait_check_any(0x02), p.wait_bits_any(0x02));
        assert_eq!(g.wait_check_any(0x05), p.wait_bits_any(0x05));

        // wait_all
        assert_eq!(g.wait_check_all(0x05), p.wait_bits_all(0x05));
        assert_eq!(g.wait_check_all(0x07), p.wait_bits_all(0x07));

        // Clear
        let g_after = g.clear(0x01);
        let p_after = p.clear_bits(0x01);
        assert_eq!(g_after, p_after, "diverge after clear 0x01");
        assert_eq!(g.events_get(), p.bits, "state diverge after clear");

        // Set (replace)
        g.set(0xFF);
        p.bits = 0xFF;
        assert_eq!(g.events_get(), p.bits, "diverge after set 0xFF");

        // Idempotent post
        g.post(0xFF);
        p.set_bits(0xFF);
        assert_eq!(g.events_get(), 0xFF);
        assert_eq!(p.bits, 0xFF);
    }

    // ── MemSlab: conservation ───────────────────────────────────────────

    #[test]
    fn mem_slab_conservation_gale_and_posix() {
        use super::posix_mem_pool;
        use gale::error::OK;
        use gale::mem_slab::MemSlab;

        let num_blocks = 8u32;
        let mut g = MemSlab::init(64, num_blocks).unwrap();
        let mut p = posix_mem_pool::MemPool::init(num_blocks).unwrap();

        // Conservation holds initially
        assert_eq!(
            g.num_used_get() + g.num_free_get(),
            num_blocks,
            "Gale conservation violated initially"
        );
        assert_eq!(
            p.num_used + p.num_free(),
            num_blocks,
            "POSIX conservation violated initially"
        );

        // Fill
        for _ in 0..num_blocks {
            let g_ok = g.alloc() == OK;
            let p_ok = p.alloc();
            assert_eq!(g_ok, p_ok, "alloc result diverged");
            // Conservation
            assert_eq!(g.num_used_get() + g.num_free_get(), num_blocks);
            assert_eq!(p.num_used + p.num_free(), num_blocks);
            // Counts match
            assert_eq!(g.num_used_get(), p.num_used);
        }

        // Full — alloc rejected
        assert_ne!(g.alloc(), OK);
        assert!(!p.alloc());

        // Drain
        for _ in 0..num_blocks {
            let g_ok = g.free() == OK;
            let p_ok = p.free();
            assert_eq!(g_ok, p_ok, "free result diverged");
            assert_eq!(g.num_used_get() + g.num_free_get(), num_blocks);
            assert_eq!(p.num_used + p.num_free(), num_blocks);
            assert_eq!(g.num_used_get(), p.num_used);
        }

        // Empty — free rejected
        assert_ne!(g.free(), OK);
        assert!(!p.free());
    }

    // ── Random operations: timer ────────────────────────────────────────

    #[test]
    fn timer_random_ops_gale_matches_posix() {
        use super::posix_timer;
        use gale::timer::Timer;

        let mut g = Timer::init(50);
        let mut p = posix_timer::Timer::init();
        g.start();
        p.start();

        let mut rng: u32 = 0xBAAD_F00D;
        for _ in 0..500 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            match rng % 4 {
                0 => {
                    let _ = g.expire();
                    let _ = p.expire();
                }
                1 => {
                    g.status_get();
                    p.status_get();
                }
                2 => {
                    g.start();
                    p.start();
                }
                _ => {
                    g.stop();
                    p.stop();
                    // Restart so we can keep expiring
                    g.start();
                    p.start();
                }
            }
            assert_eq!(
                g.status_peek(),
                p.status,
                "Gale/POSIX timer diverged at rng={rng}"
            );
        }
    }

    // ── Random operations: event ────────────────────────────────────────

    #[test]
    fn event_random_ops_gale_matches_freertos() {
        use super::freertos_event;
        use gale::event::Event;

        let mut g = Event::init();
        let mut p = freertos_event::EventGroup::init();

        let mut rng: u32 = 0xDEAD_C0DE;
        for _ in 0..1000 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            let bits = rng >> 16; // use upper 16 bits as event mask
            match rng % 3 {
                0 => {
                    g.post(bits);
                    p.set_bits(bits);
                }
                1 => {
                    g.clear(bits);
                    p.clear_bits(bits);
                }
                _ => {
                    g.set(bits);
                    p.bits = bits;
                }
            }
            assert_eq!(
                g.events_get(),
                p.bits,
                "Gale/FreeRTOS event diverged at rng={rng}"
            );
        }
    }

    // ── Random operations: mem_slab ─────────────────────────────────────

    #[test]
    fn mem_slab_random_ops_gale_matches_posix() {
        use super::posix_mem_pool;
        use gale::error::OK;
        use gale::mem_slab::MemSlab;

        let num_blocks = 32u32;
        let mut g = MemSlab::init(64, num_blocks).unwrap();
        let mut p = posix_mem_pool::MemPool::init(num_blocks).unwrap();

        let mut rng: u32 = 0xFACE_CAFE;
        for _ in 0..1000 {
            rng = rng.wrapping_mul(1103515245).wrapping_add(12345);
            if rng % 2 == 0 {
                let g_ok = g.alloc() == OK;
                let p_ok = p.alloc();
                assert_eq!(g_ok, p_ok);
            } else {
                let g_ok = g.free() == OK;
                let p_ok = p.free();
                assert_eq!(g_ok, p_ok);
            }
            assert_eq!(
                g.num_used_get(),
                p.num_used,
                "Gale/POSIX mem_slab diverged at rng={rng}"
            );
        }
    }
}
