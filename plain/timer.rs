//! Verified timer model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's k_timer kernel object.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **expiry counter and state** of Zephyr's timer.
//! Actual timeout scheduling and callback dispatch remain in C — only
//! the status counter, period, and running flag cross the FFI boundary.
//!
//! Source mapping:
//!   k_timer_init        -> Timer::init        (timer.c init)
//!   k_timer_start       -> Timer::start       (timer.c start)
//!   k_timer_stop        -> Timer::stop        (timer.c stop)
//!   k_timer_status_get  -> Timer::status_get  (timer.c status_get: read + reset)
//!   k_timer_status_sync -> (not modeled)      (waits for expiry — scheduling)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_OBJ_CORE_TIMER — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - k_timer_user_data_set/get — application data pointer
//!   - k_timer_expires_ticks / k_timer_remaining_ticks — timing queries
//!   - k_timer_status_sync — blocking wait (scheduling concern)
//!   - expiry_fn / stop_fn callbacks — dispatched by C scheduler
//!
//! ASIL-D verified properties:
//!   TM1: status >= 0 (trivially true for u32)
//!   TM2: status_get returns old value and sets status = 0
//!   TM3: start sets status = 0
//!   TM4: stop sets status = 0, running = false
//!   TM5: expiry increments status by 1 (checked_add)
//!   TM6: period == 0 after init(_, 0) (one-shot)
//!   TM7: period > 0 after init(_, p>0) (periodic)
//!   TM8: no overflow (checked_add returns error on overflow)
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
const EOVERFLOW: i32 = -75;
/// Timer state model — expiry counter + period + running flag.
///
/// Corresponds to Zephyr's struct k_timer {
///     struct _timeout timeout;   // scheduling (not modeled)
///     struct k_work_delayable work; // (not modeled)
///     uint32_t status;           // expiry count since last read
///     uint32_t period;           // 0 = one-shot, >0 = periodic (ticks)
/// };
///
/// We model the running state explicitly; in Zephyr it is implicit
/// (timeout node linked into the timeout queue).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timer {
    /// Expiry counter: incremented on each expiry, reset on get/start/stop.
    pub status: u32,
    /// Timer period in ticks: 0 = one-shot, >0 = periodic.
    pub period: u32,
    /// Whether the timer is actively running.
    pub running: bool,
}
impl Timer {
    /// Initialize a timer with given period.
    ///
    /// Period 0 means one-shot; period > 0 means periodic.
    /// Timer starts in the stopped state.
    pub fn init(period: u32) -> Timer {
        Timer {
            status: 0,
            period,
            running: false,
        }
    }
    /// Start the timer.
    ///
    /// Resets the status counter and marks the timer as running.
    /// TM3: start sets status = 0.
    pub fn start(&mut self) {
        self.status = 0;
        self.running = true;
    }
    /// Stop the timer.
    ///
    /// Resets the status counter and marks the timer as stopped.
    /// TM4: stop sets status = 0, running = false.
    pub fn stop(&mut self) {
        self.status = 0;
        self.running = false;
    }
    /// Record a timer expiry event.
    ///
    /// Increments the status counter by 1.
    /// Returns the new status value on success, or EOVERFLOW if the
    /// counter would overflow u32::MAX.
    ///
    /// TM5: expiry increments status by 1.
    /// TM8: no overflow (returns error on u32::MAX).
    pub fn expire(&mut self) -> Result<u32, i32> {
        if self.status == u32::MAX {
            Err(EOVERFLOW)
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                self.status = self.status + 1;
            }
            Ok(self.status)
        }
    }
    /// Read and reset the status counter.
    ///
    /// Returns the number of expiry events since the last status_get
    /// (or since start/stop), then resets the counter to 0.
    ///
    /// TM2: returns old value, sets status = 0.
    pub fn status_get(&mut self) -> u32 {
        let old_status = self.status;
        self.status = 0;
        old_status
    }
    /// Check if the timer is currently running.
    pub fn is_running(&self) -> bool {
        self.running
    }
    /// Get the timer period.
    pub fn period_get(&self) -> u32 {
        self.period
    }
    /// Peek at the status counter without resetting it.
    pub fn status_peek(&self) -> u32 {
        self.status
    }
}
