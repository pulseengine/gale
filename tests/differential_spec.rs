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
}
