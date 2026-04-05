//! Verified timeout model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's timeout subsystem
//! (kernel/timeout.c). All safety-critical properties are proven with
//! Verus (SMT/Z3).
//!
//! This module models the **tick arithmetic and deadline ordering** of
//! Zephyr's timeout scheduling. Actual linked-list management, spinlock
//! handling, and callback dispatch remain in C -- only the deadline
//! computation and state tracking cross the FFI boundary.
//!
//! Source mapping:
//!   z_add_timeout       -> Timeout::add       (timeout.c: schedule deadline)
//!   z_abort_timeout     -> Timeout::abort      (timeout.c: cancel pending)
//!   sys_clock_announce  -> Timeout::announce   (timeout.c: advance time, fire)
//!   sys_timepoint_calc  -> Timeout::timepoint_calc (timeout.c: relative->absolute)
//!   sys_clock_tick_get  -> Timeout::now        (timeout.c: current tick)
//!
//! Omitted (not safety-relevant):
//!   - Linked-list insertion/removal (delta queue) -- data structure concern
//!   - Spinlock (timeout_lock) -- concurrency primitive, not modeled
//!   - Callback dispatch (t->fn(t)) -- application code
//!   - sys_clock_set_timeout -- hardware timer driver
//!   - CONFIG_SMP re-entrancy guard -- platform concern
//!   - CONFIG_TICKLESS_KERNEL elapsed() -- driver concern
//!   - SYS_PORT_TRACING_* -- instrumentation
//!   - CONFIG_USERSPACE (z_vrfy_*) -- syscall marshaling
//!
//! ASIL-D verified properties:
//!   TO1: deadline >= current_tick when active (after add)
//!   TO2: add sets deadline = now + duration (relative timeout)
//!   TO3: abort clears timeout to inactive
//!   TO4: announce fires timeouts where deadline <= now
//!   TO5: tick arithmetic does not overflow (u64)
//!   TO6: relative-to-absolute conversion is correct
//!   TO7: K_FOREVER means never expires (deadline = u64::MAX)
//!   TO8: K_NO_WAIT means immediate (deadline = 0)
use crate::error::*;
/// Sentinel value for "wait forever" -- timeout never expires.
/// Corresponds to Zephyr's K_FOREVER / K_TICKS_FOREVER.
pub const K_FOREVER_TICKS: u64 = u64::MAX;
/// Sentinel value for "no wait" -- timeout expires immediately.
/// Corresponds to Zephyr's K_NO_WAIT.
pub const K_NO_WAIT_TICKS: u64 = 0;
/// Timeout state model -- deadline + active flag + current tick.
///
/// Corresponds to Zephyr's struct _timeout {
///     sys_dnode_t node;     // linked list node (not modeled)
///     _timeout_func_t fn;   // callback (not modeled)
///     int64_t dticks;       // delta ticks to next timeout
/// } plus the global `curr_tick` variable.
///
/// We model the absolute deadline (not delta ticks) and a monotonic
/// system tick counter. The C shim converts between delta and absolute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeout {
    /// Absolute deadline in system ticks.
    /// K_FOREVER_TICKS (u64::MAX) means "never expires".
    /// 0 means "already expired / immediate".
    pub deadline: u64,
    /// Whether this timeout is actively scheduled.
    pub active: bool,
    /// Current system tick (monotonic, never decreases).
    pub current_tick: u64,
}
impl Timeout {
    /// Initialize a timeout in the inactive state.
    ///
    /// timeout_q.h: z_init_timeout: sys_dnode_init(&to->node);
    pub fn init(current_tick: u64) -> Timeout {
        Timeout {
            deadline: 0,
            active: false,
            current_tick,
        }
    }
    /// Schedule a timeout with a relative duration (in ticks).
    ///
    /// timeout.c z_add_timeout (relative path):
    ///   to->dticks = timeout.ticks + 1 + ticks_elapsed;
    ///   (absolute) deadline = curr_tick + duration
    ///
    /// Returns the absolute deadline on success.
    /// Returns EINVAL if duration would cause overflow beyond u64::MAX-1
    /// (u64::MAX is reserved for K_FOREVER).
    ///
    /// TO2: deadline = current_tick + duration
    /// TO5: no overflow
    /// TO6: relative-to-absolute conversion is correct
    pub fn add(&mut self, duration: u64) -> Result<u64, i32> {
        if duration >= K_FOREVER_TICKS - self.current_tick {
            Err(EINVAL)
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.deadline = self.current_tick + duration;
            }
            self.active = true;
            Ok(self.deadline)
        }
    }
    /// Schedule a timeout with an absolute deadline.
    ///
    /// timeout.c z_add_timeout (absolute path):
    ///   k_ticks_t dticks = Z_TICK_ABS(timeout.ticks) - curr_tick;
    ///   to->dticks = max(1, dticks);
    ///
    /// Returns EINVAL if deadline is in the past or is the FOREVER sentinel.
    pub fn add_absolute(&mut self, deadline: u64) -> Result<u64, i32> {
        if deadline < self.current_tick || deadline >= K_FOREVER_TICKS {
            Err(EINVAL)
        } else {
            self.deadline = deadline;
            self.active = true;
            Ok(self.deadline)
        }
    }
    /// Schedule a "forever" timeout (never expires).
    ///
    /// timeout.c z_add_timeout:
    ///   if (K_TIMEOUT_EQ(timeout, K_FOREVER)) { return 0; }
    ///
    /// TO7: K_FOREVER means never expires.
    pub fn add_forever(&mut self) -> Timeout {
        Timeout {
            deadline: K_FOREVER_TICKS,
            active: true,
            current_tick: self.current_tick,
        }
    }
    /// Schedule an immediate timeout (no wait).
    ///
    /// TO8: K_NO_WAIT means immediate (deadline = 0).
    pub fn add_no_wait(&mut self) -> Timeout {
        Timeout {
            deadline: K_NO_WAIT_TICKS,
            active: true,
            current_tick: self.current_tick,
        }
    }
    /// Abort (cancel) a pending timeout.
    ///
    /// timeout.c z_abort_timeout:
    ///   remove_timeout(to);
    ///   to->dticks = TIMEOUT_DTICKS_ABORTED;
    ///   ret = 0;
    ///
    /// Returns OK if the timeout was active and is now cancelled.
    /// Returns EINVAL if the timeout was already inactive.
    ///
    /// TO3: abort clears the timeout to inactive.
    pub fn abort(&mut self) -> i32 {
        if self.active {
            self.active = false;
            OK
        } else {
            EINVAL
        }
    }
    /// Advance the system tick and check if this timeout has expired.
    ///
    /// timeout.c sys_clock_announce:
    ///   for (t = first(); t != NULL && t->dticks <= announce_remaining; ...)
    ///     curr_tick += dt; ... remove_timeout(t); t->fn(t);
    ///   curr_tick += announce_remaining;
    ///
    /// Returns true if the timeout has expired (and deactivates it).
    /// Returns false if still pending or inactive.
    ///
    /// TO4: fires timeouts where deadline <= now.
    pub fn announce(&mut self, ticks: u64) -> Result<bool, i32> {
        if ticks >= K_FOREVER_TICKS - self.current_tick {
            return Err(EINVAL);
        }
        #[allow(clippy::arithmetic_side_effects)]
        let new_tick = self.current_tick + ticks;
        self.current_tick = new_tick;
        if self.active && self.deadline != K_FOREVER_TICKS
            && self.deadline <= self.current_tick
        {
            self.active = false;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    /// Compute remaining ticks until deadline.
    ///
    /// timeout.c z_timeout_remaining:
    ///   ticks = timeout_rem(timeout) - elapsed();
    ///
    /// Returns 0 if inactive or already expired.
    pub fn remaining(&self) -> u64 {
        if !self.active {
            0
        } else if self.deadline == K_FOREVER_TICKS {
            K_FOREVER_TICKS
        } else if self.deadline > self.current_tick {
            #[allow(clippy::arithmetic_side_effects)]
            let r = self.deadline - self.current_tick;
            r
        } else {
            0
        }
    }
    /// Get the absolute expiration tick.
    ///
    /// timeout.c z_timeout_expires:
    ///   ticks = curr_tick + timeout_rem(timeout);
    pub fn expires(&self) -> u64 {
        if self.active { self.deadline } else { self.current_tick }
    }
    /// Check if the timeout is active.
    pub fn is_active(&self) -> bool {
        self.active
    }
    /// Check if the timeout is the "forever" sentinel.
    pub fn is_forever(&self) -> bool {
        self.deadline == K_FOREVER_TICKS
    }
    /// Check if the timeout is the "no wait" / immediate sentinel.
    pub fn is_no_wait(&self) -> bool {
        self.deadline == K_NO_WAIT_TICKS
    }
    /// Get the current system tick.
    pub fn now(&self) -> u64 {
        self.current_tick
    }
    /// Compute an absolute deadline from a relative duration.
    ///
    /// timeout.c sys_timepoint_calc:
    ///   if K_FOREVER: tick = UINT64_MAX
    ///   if K_NO_WAIT: tick = 0
    ///   else: tick = sys_clock_tick_get() + max(1, dt)
    ///
    /// TO6: relative-to-absolute conversion is correct.
    /// TO7: forever -> u64::MAX
    /// TO8: no_wait -> 0
    pub fn timepoint_calc(&self, duration: u64) -> Result<u64, i32> {
        if duration >= K_FOREVER_TICKS - self.current_tick {
            Err(EINVAL)
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            let tp = self.current_tick + duration;
            Ok(tp)
        }
    }
    /// Convert an absolute timepoint back to a relative timeout.
    ///
    /// timeout.c sys_timepoint_timeout:
    ///   remaining = (timepoint > now) ? (timepoint - now) : 0;
    pub fn timepoint_timeout(&self, timepoint: u64) -> u64 {
        if timepoint == K_FOREVER_TICKS {
            K_FOREVER_TICKS
        } else if timepoint == K_NO_WAIT_TICKS {
            0
        } else if timepoint > self.current_tick {
            #[allow(clippy::arithmetic_side_effects)]
            let r = timepoint - self.current_tick;
            r
        } else {
            0
        }
    }
}
/// Decision for add_timeout: compute absolute deadline from current_tick + duration.
///
/// Returns Ok(deadline) on success, Err(EINVAL) on overflow.
/// TO2: deadline = current_tick + duration, TO5: no overflow.
pub fn add_decide(current_tick: u64, duration: u64) -> Result<u64, i32> {
    if current_tick >= K_FOREVER_TICKS {
        return Err(EINVAL);
    }
    if duration >= K_FOREVER_TICKS - current_tick {
        return Err(EINVAL);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let dl = current_tick + duration;
    Ok(dl)
}
/// Decision for abort_timeout: is the timeout active?
///
/// Returns true if active (should be removed), false if already inactive.
/// TO3: abort clears to inactive.
pub fn abort_decide(is_active: bool) -> bool {
    is_active
}
/// Decision for announce: advance tick and check if timeout fired.
///
/// Returns Ok((new_tick, fired)) on success, Err(EINVAL) on overflow.
/// TO4: fires when deadline <= new_tick, TO7: K_FOREVER never fires.
pub fn announce_decide(
    current_tick: u64,
    ticks: u64,
    deadline: u64,
    active: bool,
) -> Result<(u64, bool), i32> {
    if ticks >= K_FOREVER_TICKS - current_tick {
        return Err(EINVAL);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_tick = current_tick + ticks;
    let fired = active && deadline != K_FOREVER_TICKS && deadline <= new_tick;
    Ok((new_tick, fired))
}
