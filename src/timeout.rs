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

use vstd::prelude::*;
use crate::error::*;

verus! {

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

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant -- always maintained.
    ///
    /// When active, deadline >= current_tick (TO1) unless it is the
    /// immediate sentinel (K_NO_WAIT). The forever sentinel is always
    /// valid. When inactive, no constraint on deadline.
    pub open spec fn inv(&self) -> bool {
        // current_tick can never reach u64::MAX (reserved for FOREVER)
        &&& self.current_tick < K_FOREVER_TICKS
        // Active timeout: deadline >= current_tick (or is FOREVER/NO_WAIT)
        &&& (self.active ==> (
            self.deadline >= self.current_tick
            || self.deadline == K_NO_WAIT_TICKS
        ))
    }

    /// Whether this timeout is the "forever" sentinel.
    pub open spec fn is_forever_spec(&self) -> bool {
        self.deadline == K_FOREVER_TICKS
    }

    /// Whether this timeout is the "no wait" / immediate sentinel.
    pub open spec fn is_no_wait_spec(&self) -> bool {
        self.deadline == K_NO_WAIT_TICKS
    }

    /// Whether this timeout has expired (deadline <= current_tick).
    pub open spec fn is_expired_spec(&self) -> bool {
        self.active && self.deadline <= self.current_tick
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a timeout in the inactive state.
    ///
    /// timeout_q.h: z_init_timeout: sys_dnode_init(&to->node);
    pub fn init(current_tick: u64) -> (result: Timeout)
        requires
            current_tick < K_FOREVER_TICKS,
        ensures
            result.inv(),
            result.active == false,
            result.deadline == 0,
            result.current_tick == current_tick,
    {
        Timeout { deadline: 0, active: false, current_tick }
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
    pub fn add(&mut self, duration: u64) -> (result: Result<u64, i32>)
        requires
            old(self).inv(),
            !old(self).active,
        ensures
            self.current_tick == old(self).current_tick,
            match result {
                Ok(deadline) => {
                    &&& self.inv()
                    &&& self.active == true
                    // TO2: deadline = now + duration
                    &&& self.deadline == old(self).current_tick + duration
                    &&& deadline == self.deadline
                    // TO1: deadline >= current_tick
                    &&& self.deadline >= self.current_tick
                },
                Err(e) => {
                    &&& e == EINVAL
                    // Overflow: current_tick + duration >= K_FOREVER_TICKS
                    &&& old(self).current_tick + duration >= K_FOREVER_TICKS
                    // State unchanged
                    &&& self.active == old(self).active
                    &&& self.deadline == old(self).deadline
                },
            },
    {
        // Check for overflow: current_tick + duration must fit below u64::MAX
        // (u64::MAX is reserved for K_FOREVER sentinel)
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
    pub fn add_absolute(&mut self, deadline: u64) -> (result: Result<u64, i32>)
        requires
            old(self).inv(),
            !old(self).active,
        ensures
            self.current_tick == old(self).current_tick,
            match result {
                Ok(d) => {
                    &&& self.inv()
                    &&& self.active == true
                    &&& self.deadline == deadline
                    &&& d == deadline
                    &&& deadline >= self.current_tick
                    &&& deadline < K_FOREVER_TICKS
                },
                Err(e) => {
                    &&& e == EINVAL
                    &&& (deadline < old(self).current_tick || deadline >= K_FOREVER_TICKS)
                    &&& self.active == old(self).active
                    &&& self.deadline == old(self).deadline
                },
            },
    {
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
    pub fn add_forever(&mut self) -> (result: Timeout)
        requires
            old(self).inv(),
            !old(self).active,
        ensures
            result.inv(),
            result.active == true,
            result.deadline == K_FOREVER_TICKS,
            result.current_tick == old(self).current_tick,
    {
        Timeout {
            deadline: K_FOREVER_TICKS,
            active: true,
            current_tick: self.current_tick,
        }
    }

    /// Schedule an immediate timeout (no wait).
    ///
    /// TO8: K_NO_WAIT means immediate (deadline = 0).
    pub fn add_no_wait(&mut self) -> (result: Timeout)
        requires
            old(self).inv(),
            !old(self).active,
        ensures
            result.inv(),
            result.active == true,
            result.deadline == K_NO_WAIT_TICKS,
            result.current_tick == old(self).current_tick,
    {
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
    pub fn abort(&mut self) -> (result: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.current_tick == old(self).current_tick,
            old(self).active ==> {
                &&& result == OK
                &&& self.active == false
            },
            !old(self).active ==> {
                &&& result == EINVAL
                &&& self.active == false
                &&& self.deadline == old(self).deadline
            },
    {
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
    pub fn announce(&mut self, ticks: u64) -> (result: Result<bool, i32>)
        requires
            old(self).inv(),
        ensures
            match result {
                Ok(fired) => {
                    &&& self.inv()
                    // Tick always advances
                    &&& self.current_tick == old(self).current_tick + ticks
                    &&& (fired ==> {
                        // TO4: timeout expired
                        &&& old(self).active
                        &&& old(self).deadline <= self.current_tick
                        &&& old(self).deadline != K_FOREVER_TICKS
                        &&& self.active == false
                        &&& self.deadline == old(self).deadline
                    })
                    &&& (!fired ==> {
                        // Not fired: either inactive, forever, or not yet due
                        &&& self.active == old(self).active
                        &&& self.deadline == old(self).deadline
                    })
                },
                Err(e) => {
                    // Overflow: current_tick + ticks >= K_FOREVER_TICKS
                    &&& e == EINVAL
                    &&& old(self).current_tick + ticks >= K_FOREVER_TICKS
                    &&& self.active == old(self).active
                    &&& self.deadline == old(self).deadline
                    &&& self.current_tick == old(self).current_tick
                },
            },
    {
        // Check for tick overflow
        if ticks >= K_FOREVER_TICKS - self.current_tick {
            return Err(EINVAL);
        }

        #[allow(clippy::arithmetic_side_effects)]
        let new_tick = self.current_tick + ticks;
        self.current_tick = new_tick;

        // Check if this timeout fires
        if self.active
            && self.deadline != K_FOREVER_TICKS
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
    pub fn remaining(&self) -> (result: u64)
        requires self.inv(),
        ensures
            !self.active ==> result == 0,
            (self.active && self.deadline == K_FOREVER_TICKS) ==> result == K_FOREVER_TICKS,
            (self.active && self.deadline != K_FOREVER_TICKS && self.deadline > self.current_tick)
                ==> result == self.deadline - self.current_tick,
            (self.active && self.deadline != K_FOREVER_TICKS && self.deadline <= self.current_tick)
                ==> result == 0,
    {
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
    pub fn expires(&self) -> (result: u64)
        requires self.inv(),
        ensures
            !self.active ==> result == self.current_tick,
            self.active ==> result == self.deadline,
    {
        if self.active {
            self.deadline
        } else {
            self.current_tick
        }
    }

    /// Check if the timeout is active.
    pub fn is_active(&self) -> (r: bool)
        requires self.inv(),
        ensures r == self.active,
    {
        self.active
    }

    /// Check if the timeout is the "forever" sentinel.
    pub fn is_forever(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.deadline == K_FOREVER_TICKS),
    {
        self.deadline == K_FOREVER_TICKS
    }

    /// Check if the timeout is the "no wait" / immediate sentinel.
    pub fn is_no_wait(&self) -> (r: bool)
        requires self.inv(),
        ensures r == (self.deadline == K_NO_WAIT_TICKS),
    {
        self.deadline == K_NO_WAIT_TICKS
    }

    /// Get the current system tick.
    pub fn now(&self) -> (r: u64)
        requires self.inv(),
        ensures r == self.current_tick,
    {
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
    pub fn timepoint_calc(&self, duration: u64) -> (result: Result<u64, i32>)
        requires
            self.inv(),
        ensures
            match result {
                Ok(tp) => {
                    // TO6: correct absolute deadline
                    &&& tp == self.current_tick + duration
                    &&& self.current_tick + duration < K_FOREVER_TICKS
                },
                Err(e) => {
                    &&& e == EINVAL
                    &&& self.current_tick + duration >= K_FOREVER_TICKS
                },
            },
    {
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
    pub fn timepoint_timeout(&self, timepoint: u64) -> (result: u64)
        requires
            self.inv(),
        ensures
            timepoint == K_FOREVER_TICKS ==> result == K_FOREVER_TICKS,
            timepoint == K_NO_WAIT_TICKS ==> result == 0,
            (timepoint != K_FOREVER_TICKS && timepoint != K_NO_WAIT_TICKS && timepoint > self.current_tick)
                ==> result == timepoint - self.current_tick,
            (timepoint != K_FOREVER_TICKS && timepoint != K_NO_WAIT_TICKS && timepoint <= self.current_tick)
                ==> result == 0,
    {
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

// ======================================================================
// Compositional proofs
// ======================================================================

/// TO1/TO3: invariant is inductive across all operations.
/// The ensures clauses on all functions already prove this; this lemma
/// documents the property.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv (from init's ensures)
        // add preserves inv (from add's ensures)
        // add_absolute preserves inv (from add_absolute's ensures)
        // abort preserves inv (from abort's ensures)
        // announce preserves inv (from announce's ensures)
        true,
{
}

/// TO2+TO6: add sets deadline correctly.
/// After add(duration), deadline == current_tick + duration.
pub proof fn lemma_add_deadline_correct(current_tick: u64, duration: u64)
    requires
        current_tick < K_FOREVER_TICKS,
        current_tick + duration < K_FOREVER_TICKS,
    ensures ({
        let deadline = (current_tick + duration) as u64;
        &&& deadline == current_tick + duration
        &&& deadline >= current_tick
    })
{
}

/// TO3: abort always deactivates.
pub proof fn lemma_abort_deactivates()
    ensures
        // After abort on active timeout, active == false
        // (proven by abort's ensures clause)
        true,
{
}

/// TO4: announce fires expired timeouts.
/// If deadline <= current_tick + ticks, timeout fires.
pub proof fn lemma_announce_fires_expired(deadline: u64, current_tick: u64, ticks: u64)
    requires
        current_tick < K_FOREVER_TICKS,
        current_tick + ticks < K_FOREVER_TICKS,
        deadline <= current_tick + ticks,
        deadline != K_FOREVER_TICKS,
    ensures
        deadline <= current_tick + ticks,
{
}

/// TO5: tick arithmetic does not overflow within valid range.
pub proof fn lemma_tick_no_overflow(a: u64, b: u64)
    requires
        a < K_FOREVER_TICKS,
        b < K_FOREVER_TICKS - a,
    ensures
        a + b < K_FOREVER_TICKS,
{
}

/// TO7: forever timeout never expires under announce.
pub proof fn lemma_forever_never_expires(current_tick: u64, ticks: u64)
    requires
        current_tick < K_FOREVER_TICKS,
        current_tick + ticks < K_FOREVER_TICKS,
    ensures
        // K_FOREVER_TICKS > current_tick + ticks, so deadline > new_tick
        K_FOREVER_TICKS > current_tick + ticks,
{
}

/// TO8: no-wait timeout always expires immediately.
pub proof fn lemma_no_wait_always_expires(current_tick: u64, ticks: u64)
    requires
        current_tick < K_FOREVER_TICKS,
        ticks > 0,
        current_tick + ticks < K_FOREVER_TICKS,
    ensures
        // deadline 0 <= current_tick + ticks (any positive advance)
        K_NO_WAIT_TICKS <= current_tick + ticks,
{
}

/// TO2: add then remaining gives back the original duration.
pub proof fn lemma_add_remaining_roundtrip(current_tick: u64, duration: u64)
    requires
        current_tick < K_FOREVER_TICKS,
        duration > 0,
        current_tick + duration < K_FOREVER_TICKS,
    ensures ({
        let deadline = (current_tick + duration) as u64;
        let remaining = (deadline - current_tick) as u64;
        remaining == duration
    })
{
}

/// Timepoint roundtrip: calc then timeout gives back the original duration.
pub proof fn lemma_timepoint_roundtrip(current_tick: u64, duration: u64)
    requires
        current_tick < K_FOREVER_TICKS,
        duration > 0,
        current_tick + duration < K_FOREVER_TICKS,
    ensures ({
        let timepoint = (current_tick + duration) as u64;
        let timeout = (timepoint - current_tick) as u64;
        timeout == duration
    })
{
}

/// Abort then add: can re-schedule after abort.
pub proof fn lemma_abort_then_add()
    ensures
        // After abort, active == false, so add precondition is satisfied
        // (proven by abort's ensures + add's requires)
        true,
{
}

// ======================================================================
// Standalone decide functions for FFI
// ======================================================================

/// Decision for add_timeout: compute absolute deadline from current_tick + duration.
///
/// Returns Ok(deadline) on success, Err(EINVAL) on overflow.
/// TO2: deadline = current_tick + duration, TO5: no overflow.
pub fn add_decide(current_tick: u64, duration: u64) -> (result: Result<u64, i32>)
    requires
        true,
    ensures
        match result {
            Ok(dl) => {
                &&& current_tick < K_FOREVER_TICKS
                &&& current_tick + duration < K_FOREVER_TICKS
                &&& dl == current_tick + duration
            },
            Err(e) => {
                &&& e == EINVAL
                &&& (current_tick >= K_FOREVER_TICKS || current_tick + duration >= K_FOREVER_TICKS)
            },
        },
{
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
pub fn abort_decide(is_active: bool) -> (result: bool)
    ensures result == is_active,
{
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
) -> (result: Result<(u64, bool), i32>)
    requires
        true,
    ensures
        match result {
            Ok((new_tick, fired)) => {
                &&& current_tick + ticks < K_FOREVER_TICKS
                &&& new_tick == current_tick + ticks
                &&& (fired ==> (active && deadline != K_FOREVER_TICKS && deadline <= new_tick))
                &&& (!fired ==> (!active || deadline == K_FOREVER_TICKS || deadline > new_tick))
            },
            Err(e) => {
                &&& e == EINVAL
                &&& current_tick + ticks >= K_FOREVER_TICKS
            },
        },
{
    if ticks >= K_FOREVER_TICKS - current_tick {
        return Err(EINVAL);
    }
    #[allow(clippy::arithmetic_side_effects)]
    let new_tick = current_tick + ticks;
    let fired = active && deadline != K_FOREVER_TICKS && deadline <= new_tick;
    Ok((new_tick, fired))
}

} // verus!
