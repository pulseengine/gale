//! Plain Rust semaphore for Rocq-of-Rust translation.
//!
//! This file contains the same logic as src/sem.rs but without Verus
//! annotations, so coq_of_rust can translate it to Rocq (.v) files.
//! Hand-written Rocq proofs in proofs/sem_proofs.v reason about this code.

/// Zephyr error codes.
pub const EINVAL: i32 = -22;
pub const EBUSY: i32 = -16;
pub const EAGAIN: i32 = -11;
pub const OK: i32 = 0;

/// Thread priority (lower value = higher priority).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Priority {
    pub value: u32,
}

/// Thread execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Suspended,
}

/// Minimal thread model.
#[derive(Debug, Clone)]
pub struct Thread {
    pub id: u32,
    pub priority: Priority,
    pub state: ThreadState,
    pub return_value: i32,
}

impl Thread {
    pub fn new(id: u32, priority: Priority) -> Self {
        Thread {
            id,
            priority,
            state: ThreadState::Ready,
            return_value: 0,
        }
    }

    pub fn block(&mut self) {
        self.state = ThreadState::Blocked;
    }

    pub fn wake(&mut self, return_value: i32) {
        self.return_value = return_value;
        self.state = ThreadState::Ready;
    }
}

/// Priority-ordered wait queue.
pub struct WaitQueue {
    entries: Vec<Thread>,
}

impl WaitQueue {
    pub fn new() -> Self {
        WaitQueue { entries: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove and return the highest-priority (first) waiting thread.
    pub fn unpend_first(&mut self, return_value: i32) -> Option<Thread> {
        if self.entries.is_empty() {
            return None;
        }
        let mut thread = self.entries.remove(0);
        thread.state = ThreadState::Ready;
        thread.return_value = return_value;
        Some(thread)
    }

    /// Insert a thread in priority order.
    pub fn pend(&mut self, thread: Thread) {
        let pos = self
            .entries
            .iter()
            .position(|e| thread.priority < e.priority)
            .unwrap_or(self.entries.len());
        self.entries.insert(pos, thread);
    }

    /// Remove all threads, waking each with return_value.
    pub fn unpend_all(&mut self, return_value: i32) -> usize {
        let count = self.entries.len();
        for thread in self.entries.iter_mut() {
            thread.state = ThreadState::Ready;
            thread.return_value = return_value;
        }
        self.entries.clear();
        count
    }
}

/// Result of a give operation.
pub enum GiveResult {
    Incremented,
    WokeThread(Thread),
    Saturated,
}

/// Counting semaphore — port of Zephyr kernel/sem.c.
pub struct Semaphore {
    wait_q: WaitQueue,
    count: u32,
    limit: u32,
}

impl Semaphore {
    /// z_impl_k_sem_init (sem.c:45-73)
    pub fn init(initial_count: u32, limit: u32) -> Result<Self, i32> {
        if limit == 0 || initial_count > limit {
            return Err(EINVAL);
        }
        Ok(Semaphore {
            wait_q: WaitQueue::new(),
            count: initial_count,
            limit,
        })
    }

    /// z_impl_k_sem_give (sem.c:95-121)
    pub fn give(&mut self) -> GiveResult {
        if let Some(thread) = self.wait_q.unpend_first(OK) {
            GiveResult::WokeThread(thread)
        } else if self.count != self.limit {
            self.count += 1;
            GiveResult::Incremented
        } else {
            GiveResult::Saturated
        }
    }

    /// z_impl_k_sem_take — non-blocking (sem.c:132-164 with K_NO_WAIT)
    pub fn try_take(&mut self) -> i32 {
        if self.count > 0 {
            self.count -= 1;
            OK
        } else {
            EBUSY
        }
    }

    /// z_impl_k_sem_take — blocking path
    pub fn take_blocking(&mut self, mut thread: Thread) -> bool {
        if self.count > 0 {
            self.count -= 1;
            return true;
        }
        thread.block();
        self.wait_q.pend(thread);
        false
    }

    /// z_impl_k_sem_reset (sem.c:166-192)
    pub fn reset(&mut self) -> usize {
        let woken = self.wait_q.unpend_all(EAGAIN);
        self.count = 0;
        woken
    }

    /// k_sem_count_get (kernel.h inline)
    pub fn count_get(&self) -> u32 {
        self.count
    }

    pub fn limit_get(&self) -> u32 {
        self.limit
    }
}
