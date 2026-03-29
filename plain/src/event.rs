//! Verified event bitmask model for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/events.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **32-bit event bitmask** of Zephyr's event object.
//! Wait queue management remains in C — only the bitmask operations cross
//! the FFI boundary.
//!
//! Source mapping:
//!   k_event_init       -> Event::init           (events.c: init to 0)
//!   k_event_post       -> Event::post           (events.c: events |= new)
//!   k_event_set        -> Event::set            (events.c: events = new)
//!   k_event_set_masked -> Event::set_masked     (events.c: selective set)
//!   k_event_clear      -> Event::clear          (events.c: events &= ~clear)
//!   k_event_wait       -> Event::wait_check_any (events.c: any-bit match)
//!   k_event_wait_all   -> Event::wait_check_all (events.c: all-bits match)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_OBJ_CORE_EVENT — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - Timeout / wait queue blocking — handled in C
//!
//! ASIL-D verified properties:
//!   EV1: post ORs bits: events |= new
//!   EV2: set replaces: events = new
//!   EV3: clear ANDs complement: events &= !clear_bits
//!   EV4: set_masked: events = (events & !mask) | (new & mask)
//!   EV5: wait_any: returns true when (events & desired) != 0
//!   EV6: wait_all: returns true when (events & desired) == desired
//!   EV7: events is always a valid u32
//!   EV8: post is monotonic (never clears bits)
/// 32-bit event bitmask model.
///
/// Corresponds to Zephyr's struct k_event {
///     _wait_q_t wait_q;
///     uint32_t  events;
///     uint32_t  events_mask;
/// };
///
/// We model only the `events` bitmask. Wait queue management stays in C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Event {
    /// Current 32-bit event bitmask.
    pub events: u32,
}
impl Event {
    /// Initialize an event object with all bits cleared.
    ///
    /// events.c: event->events = 0;
    pub fn init() -> Event {
        Event { events: 0 }
    }
    /// Post (OR) new event bits into the bitmask.
    ///
    /// events.c: event->events |= new_events;
    ///
    /// Returns the resulting event bitmask.
    pub fn post(&mut self, new_events: u32) -> u32 {
        let old_events = self.events;
        self.events = self.events | new_events;
        let new_val = self.events;
        self.events
    }
    /// Set the event bitmask to an exact value, replacing all bits.
    ///
    /// events.c: old = event->events; event->events = new_events;
    ///
    /// Returns the previous event bitmask.
    pub fn set(&mut self, new_events: u32) -> u32 {
        let old_events = self.events;
        self.events = new_events;
        old_events
    }
    /// Set only the bits selected by a mask, leaving other bits unchanged.
    ///
    /// events.c: event->events = (event->events & ~mask) | (events & mask);
    ///
    /// Returns the previous event bitmask.
    pub fn set_masked(&mut self, new_events: u32, mask: u32) -> u32 {
        let old_events = self.events;
        self.events = (self.events & !mask) | (new_events & mask);
        old_events
    }
    /// Clear specific event bits.
    ///
    /// events.c: event->events &= ~clear_events;
    ///
    /// Returns the resulting event bitmask.
    pub fn clear(&mut self, clear_events: u32) -> u32 {
        self.events = self.events & !clear_events;
        self.events
    }
    /// Check if any of the desired event bits are set.
    ///
    /// events.c: match = (event->events & desired) != 0
    pub fn wait_check_any(&self, desired: u32) -> bool {
        (self.events & desired) != 0
    }
    /// Check if all of the desired event bits are set.
    ///
    /// events.c: match = (event->events & desired) == desired
    pub fn wait_check_all(&self, desired: u32) -> bool {
        (self.events & desired) == desired
    }
    /// Get the current event bitmask.
    pub fn events_get(&self) -> u32 {
        self.events
    }
}
// =================================================================
// Lightweight decision functions — scalar-only, no WaitQueue allocation.
// Used by FFI to delegate safety-critical logic to the verified model.
// =================================================================

/// Lightweight event wait decision — no WaitQueue allocation.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WaitDecision {
    /// Wait condition met: matched events returned.
    Matched = 0,
    /// Condition not met, willing to wait: pend current thread.
    Pend = 1,
    /// Condition not met, no-wait: return immediately.
    Timeout = 2,
}

/// Result of a wait decision with matched event bits.
#[derive(Debug)]
pub struct WaitDecideResult {
    pub decision: WaitDecision,
    pub matched_events: u32,
}

/// Wait type: ANY (at least one bit) or ALL (all bits).
pub const WAIT_ANY: u8 = 0;
pub const WAIT_ALL: u8 = 1;

/// Lightweight event wait decision — takes scalars, no WaitQueue allocation.
///
/// Verified properties (EV5, EV6):
/// - wait_type==ANY: matched when (events & desired) != 0
/// - wait_type==ALL: matched when (events & desired) == desired
/// - no match && is_no_wait ==> Timeout
/// - no match && !is_no_wait ==> Pend
pub fn wait_decide(
    current_events: u32,
    desired: u32,
    wait_type: u8,
    is_no_wait: bool,
) -> WaitDecideResult {
    let matched = current_events & desired;

    let condition_met = if wait_type == WAIT_ALL {
        (current_events & desired) == desired
    } else {
        matched != 0
    };

    if condition_met {
        WaitDecideResult {
            decision: WaitDecision::Matched,
            matched_events: matched,
        }
    } else if is_no_wait {
        WaitDecideResult {
            decision: WaitDecision::Timeout,
            matched_events: 0,
        }
    } else {
        WaitDecideResult {
            decision: WaitDecision::Pend,
            matched_events: 0,
        }
    }
}

/// Lightweight event post decision — takes scalars, no WaitQueue allocation.
///
/// Verified property (EV4): set_masked computes (current & ~mask) | (new & mask)
pub fn post_decide(
    current_events: u32,
    new_events: u32,
    mask: u32,
) -> u32 {
    (current_events & !mask) | (new_events & mask)
}
