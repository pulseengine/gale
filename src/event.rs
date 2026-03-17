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

use vstd::prelude::*;

verus! {

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

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant — trivially true for a plain u32.
    pub open spec fn inv(&self) -> bool {
        true
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize an event object with all bits cleared.
    ///
    /// events.c: event->events = 0;
    pub fn init() -> (result: Event)
        ensures
            result.inv(),
            result.events == 0u32,
    {
        Event { events: 0 }
    }

    /// Post (OR) new event bits into the bitmask.
    ///
    /// events.c: event->events |= new_events;
    ///
    /// Returns the resulting event bitmask.
    pub fn post(&mut self, new_events: u32) -> (result: u32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // EV1: post ORs bits
            self.events == (old(self).events | new_events),
            result == self.events,
            // EV8: post is monotonic — old bits are preserved
            old(self).events & self.events == old(self).events,
    {
        self.events = self.events | new_events;
        self.events
    }

    /// Set the event bitmask to an exact value, replacing all bits.
    ///
    /// events.c: old = event->events; event->events = new_events;
    ///
    /// Returns the previous event bitmask.
    pub fn set(&mut self, new_events: u32) -> (result: u32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // EV2: set replaces entirely
            self.events == new_events,
            result == old(self).events,
    {
        let old_events = self.events;
        self.events = new_events;
        old_events
    }

    /// Set only the bits selected by a mask, leaving other bits unchanged.
    ///
    /// events.c: event->events = (event->events & ~mask) | (events & mask);
    ///
    /// Returns the previous event bitmask.
    pub fn set_masked(&mut self, new_events: u32, mask: u32) -> (result: u32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // EV4: set_masked applies mask correctly
            self.events == ((old(self).events & !mask) | (new_events & mask)),
            result == old(self).events,
    {
        let old_events = self.events;
        self.events = (self.events & !mask) | (new_events & mask);
        old_events
    }

    /// Clear specific event bits.
    ///
    /// events.c: event->events &= ~clear_events;
    ///
    /// Returns the resulting event bitmask.
    pub fn clear(&mut self, clear_events: u32) -> (result: u32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // EV3: clear ANDs complement
            self.events == (old(self).events & !clear_events),
            result == self.events,
    {
        self.events = self.events & !clear_events;
        self.events
    }

    /// Check if any of the desired event bits are set.
    ///
    /// events.c: match = (event->events & desired) != 0
    pub fn wait_check_any(&self, desired: u32) -> (result: bool)
        requires self.inv(),
        ensures
            // EV5: wait_any semantics
            result == ((self.events & desired) != 0u32),
    {
        (self.events & desired) != 0
    }

    /// Check if all of the desired event bits are set.
    ///
    /// events.c: match = (event->events & desired) == desired
    pub fn wait_check_all(&self, desired: u32) -> (result: bool)
        requires self.inv(),
        ensures
            // EV6: wait_all semantics
            result == ((self.events & desired) == desired),
    {
        (self.events & desired) == desired
    }

    /// Get the current event bitmask.
    pub fn events_get(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.events,
    {
        self.events
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// EV7: invariant is inductive across all operations.
/// The ensures clauses on all operations already prove this; this lemma
/// documents the property.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv (from init's ensures)
        // post preserves inv (from post's ensures)
        // set preserves inv (from set's ensures)
        // set_masked preserves inv (from set_masked's ensures)
        // clear preserves inv (from clear's ensures)
        true,
{
}

/// EV8: post is monotonic — it never clears bits.
/// After posting, every bit that was set before is still set.
pub proof fn lemma_post_monotonic(events: u32, new_events: u32)
    ensures
        (events | new_events) & events == events,
{
}

/// EV2+EV3: set then clear roundtrip.
/// Setting a value and then clearing those same bits yields 0.
pub proof fn lemma_set_clear_roundtrip(value: u32)
    ensures
        value & !value == 0u32,
{
}

/// EV5+EV6: wait condition correctness.
/// wait_all implies wait_any for non-zero desired.
pub proof fn lemma_wait_conditions_correct(events: u32, desired: u32)
    requires
        desired != 0u32,
        (events & desired) == desired,
    ensures
        (events & desired) != 0u32,
{
}

/// EV1: double-post idempotence.
/// Posting the same bits twice has no additional effect.
pub proof fn lemma_post_idempotent(events: u32, new_events: u32)
    ensures
        (events | new_events) | new_events == events | new_events,
{
}

/// EV4: set_masked with full mask is equivalent to set.
pub proof fn lemma_set_masked_full(events: u32, new_events: u32)
    ensures
        (events & !0xFFFF_FFFFu32) | (new_events & 0xFFFF_FFFFu32) == new_events,
{
}

/// EV4: set_masked with zero mask is identity.
pub proof fn lemma_set_masked_zero(events: u32, new_events: u32)
    ensures
        (events & !0u32) | (new_events & 0u32) == events,
{
}

} // verus!
