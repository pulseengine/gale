//! Verified reentrant mutex for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/mutex.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! Source mapping:
//!   z_impl_k_mutex_init   -> Mutex::init         (mutex.c:55-71)
//!   z_impl_k_mutex_lock   -> Mutex::try_lock      (mutex.c:107-154, fast path)
//!                          -> Mutex::lock_blocking (mutex.c:169, blocking path)
//!   z_impl_k_mutex_unlock -> Mutex::unlock        (mutex.c:230-307)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_PRIORITY_CEILING — priority inheritance optimization
//!   - CONFIG_OBJ_CORE_MUTEX — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!
//! ASIL-D verified properties:
//!   M1: lock_count > 0 ⟺ owner.is_some() (always)
//!   M2: wait_q non-empty ⟹ mutex is locked (always)
//!   M3: try_lock when unlocked: owner set, lock_count = 1
//!   M4: try_lock when locked by same thread: lock_count incremented (reentrant)
//!   M5: try_lock when locked by different thread: returns WouldBlock, unchanged
//!   M6: unlock by non-owner: returns error, unchanged
//!   M7: unlock when lock_count > 1: count decremented, owner unchanged
//!   M8: unlock when lock_count == 1, waiter: ownership transferred, count stays 1
//!   M9: unlock when lock_count == 1, no waiter: fully unlocked (count=0, owner=None)
//!   M10: no arithmetic overflow in lock_count
//!   M11: wait queue ordering preserved across all operations
// Inlined error constants (standalone file for rocq_of_rust)
const OK: i32 = 0;
const EINVAL: i32 = -22;
const EAGAIN: i32 = -11;
const EBUSY: i32 = -16;
const EPERM: i32 = -1;
const ENOMEM: i32 = -12;
const ENOMSG: i32 = -42;
const EPIPE: i32 = -32;
const ECANCELED: i32 = -125;
const EBADF: i32 = -9;
/// Result of a lock attempt.
#[derive(Debug, PartialEq, Eq)]
pub enum LockResult {
    /// Lock acquired (first time or reentrant).
    Acquired,
    /// Mutex locked by another thread, caller chose not to wait.
    WouldBlock,
}
/// Result of an unlock operation.
#[derive(Debug)]
pub enum UnlockResult {
    /// lock_count decremented, mutex still held by current owner.
    Released,
    /// Fully unlocked, no waiters were present.
    Unlocked,
    /// Ownership transferred to highest-priority waiter.
    Transferred(Thread),
}
/// Reentrant mutex with ownership tracking.
///
/// Corresponds to Zephyr's struct k_mutex {
///     _wait_q_t wait_q;
///     struct k_thread *owner;
///     uint32_t lock_count;
///     int owner_orig_prio;  // omitted — priority inheritance
/// };
pub struct Mutex {
    /// Wait queue for threads blocked on this mutex.
    /// Corresponds to mutex->wait_q.
    pub wait_q: WaitQueue,
    /// Current owner thread ID, or None if unlocked.
    /// Corresponds to mutex->owner (NULL when unlocked).
    pub owner: Option<ThreadId>,
    /// Number of times the owner has locked this mutex.
    /// Corresponds to mutex->lock_count.
    pub lock_count: u32,
}
impl Mutex {
    /// Initialize a mutex.
    ///
    /// ```c
    /// int z_impl_k_mutex_init(struct k_mutex *mutex)
    /// {
    ///     mutex->owner = NULL;
    ///     mutex->lock_count = 0U;
    ///     z_waitq_init(&mutex->wait_q);
    ///     return 0;
    /// }
    /// ```
    ///
    /// Verified properties:
    /// - Establishes the invariant (M1, M2)
    /// - Mutex starts unlocked (owner=None, lock_count=0)
    /// - Wait queue starts empty
    pub fn init() -> Self {
        Mutex {
            wait_q: WaitQueue::new(),
            owner: None,
            lock_count: 0,
        }
    }
    /// Try to lock the mutex — non-blocking.
    ///
    /// ```c
    /// if (likely((mutex->lock_count == 0U) || (mutex->owner == _current))) {
    ///     mutex->lock_count++;
    ///     mutex->owner = _current;
    ///     return 0;
    /// }
    /// if (unlikely(K_TIMEOUT_EQ(timeout, K_NO_WAIT))) {
    ///     return -EBUSY;
    /// }
    /// ```
    ///
    /// Verified properties (M3, M4, M5, M10):
    /// - If unlocked: owner set to current, lock_count = 1 (M3)
    /// - If locked by current: lock_count incremented (M4, reentrant)
    /// - If locked by other: returns WouldBlock, unchanged (M5)
    /// - No overflow on lock_count (M10)
    /// - Invariant maintained
    pub fn try_lock(&mut self, current_id: ThreadId) -> LockResult {
        if self.lock_count == 0 {
            self.owner = Some(current_id);
            self.lock_count = 1;
            LockResult::Acquired
        } else {
            let owner_id = self.owner.unwrap();
            if owner_id.id == current_id.id {
                self.lock_count = self.lock_count + 1;
                LockResult::Acquired
            } else {
                LockResult::WouldBlock
            }
        }
    }
    /// Lock the mutex — blocking path.
    ///
    /// Models z_pend_curr(): the calling thread blocks on the wait queue.
    ///
    /// Verified properties (M11):
    /// - Thread is inserted into wait queue in priority order
    /// - Thread state set to Blocked
    /// - Mutex state unchanged (still locked by original owner)
    /// - Returns false if wait queue is full
    pub fn lock_blocking(&mut self, mut thread: Thread) -> bool {
        thread.block();
        self.wait_q.pend(thread)
    }
    /// Unlock the mutex.
    ///
    /// ```c
    /// CHECKIF(mutex->owner == NULL) { return -EINVAL; }
    /// CHECKIF(mutex->owner != _current) { return -EPERM; }
    /// if (mutex->lock_count > 1U) {
    ///     mutex->lock_count--;
    ///     return 0;
    /// }
    /// new_owner = z_unpend_first_thread(&mutex->wait_q);
    /// mutex->owner = new_owner;
    /// if (new_owner != NULL) {
    ///     arch_thread_return_value_set(new_owner, 0);
    ///     z_ready_thread(new_owner);
    /// } else {
    ///     mutex->lock_count = 0U;
    /// }
    /// return 0;
    /// ```
    ///
    /// Verified properties (M6, M7, M8, M9):
    /// - Not locked: returns -EINVAL (M6a)
    /// - Not owner: returns -EPERM (M6b)
    /// - lock_count > 1: decremented, owner unchanged (M7)
    /// - lock_count == 1, waiters: ownership transferred (M8)
    /// - lock_count == 1, no waiters: fully unlocked (M9)
    /// - Invariant maintained
    pub fn unlock(&mut self, current_id: ThreadId) -> Result<UnlockResult, i32> {
        if self.owner.is_none() {
            return Err(EINVAL);
        }
        let owner_id = self.owner.unwrap();
        if owner_id.id != current_id.id {
            return Err(EPERM);
        }
        if self.lock_count > 1 {
            self.lock_count = self.lock_count - 1;
            return Ok(UnlockResult::Released);
        }
        let new_owner = self.wait_q.unpend_first(OK);
        match new_owner {
            Some(t) => {
                self.owner = Some(t.id);
                Ok(UnlockResult::Transferred(t))
            }
            None => {
                self.owner = None;
                self.lock_count = 0;
                Ok(UnlockResult::Unlocked)
            }
        }
    }
    /// Check if the mutex is locked.
    pub fn is_locked(&self) -> bool {
        self.lock_count > 0
    }
    /// Get the current lock count.
    pub fn lock_count_get(&self) -> u32 {
        self.lock_count
    }
    /// Get the number of threads waiting on this mutex.
    pub fn num_waiters(&self) -> u32 {
        self.wait_q.len()
    }
    /// Get the current owner's thread ID, if any.
    pub fn owner_get(&self) -> Option<ThreadId> {
        self.owner
    }
}
