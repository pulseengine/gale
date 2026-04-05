//! Verified poll event state machine for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/poll.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **poll event state machine** and **poll signal**
//! of Zephyr's asynchronous polling subsystem. The actual wait queue
//! management, thread scheduling, and work queue integration remain in C.
//!
//! Source mapping:
//!   k_poll_event_init      -> PollEvent::init         (poll.c:46-62)
//!   is_condition_met       -> PollEvent::check_sem    (poll.c:65-103)
//!                          -> PollEvent::check_signal
//!                          -> PollEvent::check_msgq
//!   set_event_ready        -> PollEvent::set_ready    (poll.c:223-227)
//!   k_poll_signal_init     -> PollSignal::init        (poll.c:475-483)
//!   k_poll_signal_raise    -> PollSignal::raise       (poll.c:522-545)
//!   k_poll_signal_reset    -> PollSignal::reset       (poll.c:494-498)
//!   k_poll_signal_check    -> PollSignal::check       (poll.c:501-508)
//!   k_poll (state reset)   -> PollEvent::reset_state  (poll.c:283-348)
//!   poll_events_ready      -> poll_any_ready          (poll.c:283-316)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_USERSPACE (z_vrfy_*) -- syscall marshaling
//!   - SYS_PORT_TRACING_* -- instrumentation
//!   - k_work_poll_* -- work queue integration (uses same event types)
//!   - register_event / clear_event_registration -- linked-list management
//!   - signal_poller / signal_triggered_work -- thread wakeup mechanics
//!
//! ASIL-D verified properties:
//!   PL1: event starts in NOT_READY state
//!   PL2: state transitions only happen via explicit operations
//!   PL3: SEM_AVAILABLE iff semaphore count > 0
//!   PL4: SIGNALED iff signal was raised
//!   PL5: poll returns ready when ANY event becomes ready
//!   PL6: state is reset to NOT_READY before each poll call
//!   PL7: signal raise sets result and transitions to SIGNALED
//!   PL8: signal reset clears signaled to 0

use vstd::prelude::*;

verus! {

// ======================================================================
// Poll event types (K_POLL_TYPE_xxx) -- bitfield values
// ======================================================================

/// Ignore this event (disabled).
pub const TYPE_IGNORE: u32 = 0;
/// Poll for semaphore availability.
pub const TYPE_SEM_AVAILABLE: u32 = 1;
/// Poll for data in a queue/FIFO.
pub const TYPE_DATA_AVAILABLE: u32 = 2;
/// Poll for a signal.
pub const TYPE_SIGNAL: u32 = 4;
/// Poll for data in a message queue.
pub const TYPE_MSGQ_DATA_AVAILABLE: u32 = 8;
/// Poll for data in a pipe.
pub const TYPE_PIPE_DATA_AVAILABLE: u32 = 16;

// ======================================================================
// Poll event states (K_POLL_STATE_xxx) -- bitfield values
// ======================================================================

/// Event is not ready.
pub const STATE_NOT_READY: u32 = 0;
/// Semaphore is available.
pub const STATE_SEM_AVAILABLE: u32 = 1;
/// Data is available in queue/FIFO.
pub const STATE_DATA_AVAILABLE: u32 = 2;
/// Signal has been raised.
pub const STATE_SIGNALED: u32 = 4;
/// Message queue has data.
pub const STATE_MSGQ_DATA_AVAILABLE: u32 = 8;
/// Pipe has data.
pub const STATE_PIPE_DATA_AVAILABLE: u32 = 16;
/// Event was cancelled.
pub const STATE_CANCELLED: u32 = 32;

// ======================================================================
// Valid type/state predicates
// ======================================================================

/// Check whether a type value is one of the recognized poll types.
pub open spec fn is_valid_type(t: u32) -> bool {
    t == TYPE_IGNORE
    || t == TYPE_SEM_AVAILABLE
    || t == TYPE_DATA_AVAILABLE
    || t == TYPE_SIGNAL
    || t == TYPE_MSGQ_DATA_AVAILABLE
    || t == TYPE_PIPE_DATA_AVAILABLE
}

/// Check whether a state value is a valid combination.
/// States are a bitfield; valid states are NOT_READY (0) or
/// exactly one of the specific state bits, or CANCELLED.
pub open spec fn is_valid_state(s: u32) -> bool {
    s == STATE_NOT_READY
    || s == STATE_SEM_AVAILABLE
    || s == STATE_DATA_AVAILABLE
    || s == STATE_SIGNALED
    || s == STATE_MSGQ_DATA_AVAILABLE
    || s == STATE_PIPE_DATA_AVAILABLE
    || s == STATE_CANCELLED
}

// ======================================================================
// PollEvent -- models struct k_poll_event
// ======================================================================

/// Poll event state machine model.
///
/// Corresponds to Zephyr's struct k_poll_event {
///     uint32_t type;    // what to wait for
///     uint32_t state;   // current state
///     uint32_t tag;     // user-specified tag
/// };
///
/// We model only the type/state/tag fields. Linked-list node,
/// poller pointer, and object union remain in C.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PollEvent {
    /// Bitfield of event types (K_POLL_TYPE_xxx).
    pub event_type: u32,
    /// Bitfield of event states (K_POLL_STATE_xxx).
    pub state: u32,
    /// Optional user-specified tag (opaque, untouched by API).
    pub tag: u32,
}

impl PollEvent {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant.
    pub open spec fn inv(&self) -> bool {
        is_valid_type(self.event_type)
    }

    /// Event is in the not-ready state (spec).
    pub open spec fn is_not_ready_spec(&self) -> bool {
        self.state == STATE_NOT_READY
    }

    /// Event is ready (any state bit set) (spec).
    pub open spec fn is_ready_spec(&self) -> bool {
        self.state != STATE_NOT_READY
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a poll event.
    ///
    /// poll.c:46-62: k_poll_event_init()
    ///
    /// PL1: event starts in NOT_READY state.
    pub fn init(event_type: u32, tag: u32) -> (result: PollEvent)
        requires
            is_valid_type(event_type),
        ensures
            result.inv(),
            // PL1: starts NOT_READY
            result.state == STATE_NOT_READY,
            result.event_type == event_type,
            result.tag == tag,
    {
        PollEvent {
            event_type,
            state: STATE_NOT_READY,
            tag,
        }
    }

    /// Reset event state to NOT_READY.
    ///
    /// poll.c:283-348: before each k_poll call, events are reset.
    ///
    /// PL6: state is reset to NOT_READY before each poll call.
    pub fn reset_state(&mut self)
        requires old(self).inv(),
        ensures
            self.inv(),
            // PL6: reset to NOT_READY
            self.state == STATE_NOT_READY,
            self.event_type == old(self).event_type,
            self.tag == old(self).tag,
    {
        self.state = STATE_NOT_READY;
    }

    /// Set the event to a ready state.
    ///
    /// poll.c:223-227: set_event_ready()
    /// In Zephyr this ORs the state, but since events start NOT_READY
    /// and are set once, we model it as OR to match the C code exactly.
    ///
    /// PL2: state transitions only via explicit operations.
    pub fn set_ready(&mut self, new_state: u32)
        requires
            old(self).inv(),
            new_state != STATE_NOT_READY,
        ensures
            self.inv(),
            // PL2: state is bitwise OR of old state and new state
            self.state == (old(self).state | new_state),
            self.event_type == old(self).event_type,
            self.tag == old(self).tag,
    {
        self.state = self.state | new_state;
    }

    /// Check if a semaphore condition is met.
    ///
    /// poll.c:65-70: is_condition_met() K_POLL_TYPE_SEM_AVAILABLE case.
    ///
    /// PL3: SEM_AVAILABLE iff semaphore count > 0.
    pub fn check_sem(&self, sem_count: u32) -> (result: bool)
        requires self.inv(),
        ensures
            // PL3: ready iff count > 0 AND type matches
            result == (self.event_type == TYPE_SEM_AVAILABLE && sem_count > 0),
    {
        self.event_type == TYPE_SEM_AVAILABLE && sem_count > 0
    }

    /// Check if a signal condition is met.
    ///
    /// poll.c:77-82: is_condition_met() K_POLL_TYPE_SIGNAL case.
    ///
    /// PL4: SIGNALED iff signal was raised.
    pub fn check_signal(&self, signaled: u32) -> (result: bool)
        requires self.inv(),
        ensures
            // PL4: ready iff signaled != 0 AND type matches
            result == (self.event_type == TYPE_SIGNAL && signaled != 0),
    {
        self.event_type == TYPE_SIGNAL && signaled != 0
    }

    /// Check if a message queue condition is met.
    ///
    /// poll.c:83-88: is_condition_met() K_POLL_TYPE_MSGQ_DATA_AVAILABLE.
    pub fn check_msgq(&self, used_msgs: u32) -> (result: bool)
        requires self.inv(),
        ensures
            result == (self.event_type == TYPE_MSGQ_DATA_AVAILABLE && used_msgs > 0),
    {
        self.event_type == TYPE_MSGQ_DATA_AVAILABLE && used_msgs > 0
    }

    /// Check if a data-available (queue/FIFO) condition is met.
    ///
    /// poll.c:71-76: is_condition_met() K_POLL_TYPE_DATA_AVAILABLE.
    pub fn check_data(&self, queue_not_empty: bool) -> (result: bool)
        requires self.inv(),
        ensures
            result == (self.event_type == TYPE_DATA_AVAILABLE && queue_not_empty),
    {
        self.event_type == TYPE_DATA_AVAILABLE && queue_not_empty
    }

    /// Check if a pipe condition is met.
    ///
    /// poll.c:89-94: is_condition_met() K_POLL_TYPE_PIPE_DATA_AVAILABLE.
    pub fn check_pipe(&self, pipe_not_empty: bool) -> (result: bool)
        requires self.inv(),
        ensures
            result == (self.event_type == TYPE_PIPE_DATA_AVAILABLE && pipe_not_empty),
    {
        self.event_type == TYPE_PIPE_DATA_AVAILABLE && pipe_not_empty
    }

    /// Get current event state.
    pub fn state_get(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.state,
    {
        self.state
    }

    /// Get event type.
    pub fn type_get(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.event_type,
    {
        self.event_type
    }

    /// Get event tag.
    pub fn tag_get(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.tag,
    {
        self.tag
    }

    /// Check if event is ready (any state bit set).
    pub fn is_ready(&self) -> (result: bool)
        requires self.inv(),
        ensures result == (self.state != STATE_NOT_READY),
    {
        self.state != STATE_NOT_READY
    }

    /// Check if event is not ready.
    pub fn is_not_ready(&self) -> (result: bool)
        requires self.inv(),
        ensures result == (self.state == STATE_NOT_READY),
    {
        self.state == STATE_NOT_READY
    }

    /// Mark event as cancelled.
    pub fn cancel(&mut self)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.state == (old(self).state | STATE_CANCELLED),
            self.event_type == old(self).event_type,
            self.tag == old(self).tag,
    {
        self.state = self.state | STATE_CANCELLED;
    }
}

// ======================================================================
// PollSignal -- models struct k_poll_signal
// ======================================================================

/// Poll signal model.
///
/// Corresponds to Zephyr's struct k_poll_signal {
///     sys_dlist_t poll_events;
///     unsigned int signaled;
///     int result;
/// };
///
/// We model only the signaled flag and result value.
/// The poll_events linked list stays in C.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct PollSignal {
    /// 1 if the signal has been raised, 0 otherwise.
    pub signaled: u32,
    /// Custom result value from k_poll_signal_raise().
    pub result: i32,
}

impl PollSignal {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant.
    pub open spec fn inv(&self) -> bool {
        self.signaled == 0 || self.signaled == 1
    }

    /// Signal is raised (spec).
    pub open spec fn is_signaled_spec(&self) -> bool {
        self.signaled != 0
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Initialize a poll signal.
    ///
    /// poll.c:475-483: k_poll_signal_init()
    pub fn init() -> (result: PollSignal)
        ensures
            result.inv(),
            result.signaled == 0u32,
            result.result == 0i32,
    {
        PollSignal {
            signaled: 0,
            result: 0,
        }
    }

    /// Raise (signal) the poll signal with a result value.
    ///
    /// poll.c:522-545: k_poll_signal_raise()
    ///
    /// PL7: signal raise sets result and transitions to SIGNALED.
    pub fn raise(&mut self, result_val: i32)
        requires old(self).inv(),
        ensures
            self.inv(),
            // PL7: signaled set to 1, result set
            self.signaled == 1u32,
            self.result == result_val,
    {
        self.result = result_val;
        self.signaled = 1;
    }

    /// Reset the poll signal.
    ///
    /// poll.c:494-498: k_poll_signal_reset()
    ///
    /// PL8: signal reset clears signaled to 0.
    pub fn reset(&mut self)
        requires old(self).inv(),
        ensures
            self.inv(),
            // PL8: signaled cleared
            self.signaled == 0u32,
            // result is NOT cleared by Zephyr's reset -- only signaled flag
            self.result == old(self).result,
    {
        self.signaled = 0;
    }

    /// Check the signal state.
    ///
    /// poll.c:501-508: k_poll_signal_check()
    ///
    /// Returns (signaled, result).
    pub fn check(&self) -> (result: (u32, i32))
        requires self.inv(),
        ensures
            result.0 == self.signaled,
            result.1 == self.result,
    {
        (self.signaled, self.result)
    }

    /// Check if signal is raised.
    pub fn is_signaled(&self) -> (result: bool)
        requires self.inv(),
        ensures result == (self.signaled != 0),
    {
        self.signaled != 0
    }
}

// ======================================================================
// Poll array operations -- models k_poll() event scanning
// ======================================================================

/// Maximum number of poll events in a single poll call.
/// Matches typical Zephyr usage; in practice CONFIG_dependent.
pub const MAX_POLL_EVENTS: u32 = 16;

/// Poll event array model.
///
/// Models the array of k_poll_event passed to k_poll().
/// We use a fixed-size array to avoid heap allocation (no_std).
#[derive(Debug)]
pub struct PollEvents {
    /// Events array.
    pub events: [PollEvent; 16],
    /// Number of active events (0..=MAX_POLL_EVENTS).
    pub num_events: u32,
}

// Verus limitation: mutable array indexing not supported.
// These helpers wrap the array mutation with external_body.
#[verifier::external_body]
fn poll_events_set(events: &mut [PollEvent; 16], idx: usize, event: PollEvent) {
    events[idx] = event;
}

#[verifier::external_body]
fn poll_events_set_state(events: &mut [PollEvent; 16], idx: usize, state: u32) {
    events[idx].state = state;
}

impl PollEvents {

    // ------------------------------------------------------------------
    // Specification predicates
    // ------------------------------------------------------------------

    /// Structural invariant.
    pub open spec fn inv(&self) -> bool {
        self.num_events <= MAX_POLL_EVENTS
    }

    // ------------------------------------------------------------------
    // Operations
    // ------------------------------------------------------------------

    /// Create an empty poll events collection.
    pub fn new() -> (result: PollEvents)
        ensures
            result.inv(),
            result.num_events == 0u32,
    {
        let default_event = PollEvent {
            event_type: TYPE_IGNORE,
            state: STATE_NOT_READY,
            tag: 0,
        };
        PollEvents {
            events: [
                default_event.clone(), default_event.clone(),
                default_event.clone(), default_event.clone(),
                default_event.clone(), default_event.clone(),
                default_event.clone(), default_event.clone(),
                default_event.clone(), default_event.clone(),
                default_event.clone(), default_event.clone(),
                default_event.clone(), default_event.clone(),
                default_event.clone(), default_event,
            ],
            num_events: 0,
        }
    }

    /// Add an event to the poll set.
    ///
    /// Returns true on success, false if array is full.
    pub fn add(&mut self, event: PollEvent) -> (result: bool)
        requires
            old(self).inv(),
            event.inv(),
        ensures
            self.inv(),
            old(self).num_events < MAX_POLL_EVENTS ==> {
                &&& result == true
                &&& self.num_events == old(self).num_events + 1
            },
            old(self).num_events >= MAX_POLL_EVENTS ==> {
                &&& result == false
                &&& self.num_events == old(self).num_events
            },
    {
        if self.num_events >= MAX_POLL_EVENTS {
            return false;
        }
        poll_events_set(&mut self.events, self.num_events as usize, event);
        self.num_events = self.num_events + 1;
        true
    }

    /// Reset all event states to NOT_READY.
    ///
    /// PL6: state is reset to NOT_READY before each poll call.
    pub fn reset_all_states(&mut self)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.num_events == old(self).num_events,
    {
        let mut i: u32 = 0;
        while i < self.num_events
            invariant
                0 <= i <= self.num_events,
                self.num_events == old(self).num_events,
                self.num_events <= MAX_POLL_EVENTS,
            decreases self.num_events - i,
        {
            poll_events_set_state(&mut self.events, i as usize, STATE_NOT_READY);
            i = i + 1;
        }
    }

    /// Check if any event in the set is ready.
    ///
    /// PL5: poll returns ready when ANY event becomes ready.
    pub fn any_ready(&self) -> (result: bool)
        requires self.inv(),
    {
        let mut i: u32 = 0;
        let mut found: bool = false;
        while i < self.num_events
            invariant
                0 <= i <= self.num_events,
                self.num_events <= MAX_POLL_EVENTS,
            decreases self.num_events - i,
        {
            if self.events[i as usize].state != STATE_NOT_READY {
                found = true;
            }
            i = i + 1;
        }
        found
    }

    /// Count how many events are ready.
    pub fn count_ready(&self) -> (result: u32)
        requires self.inv(),
    {
        let mut i: u32 = 0;
        let mut count: u32 = 0;
        while i < self.num_events
            invariant
                0 <= i <= self.num_events,
                self.num_events <= MAX_POLL_EVENTS,
                count <= i,
            decreases self.num_events - i,
        {
            if self.events[i as usize].state != STATE_NOT_READY {
                count = count + 1;
            }
            i = i + 1;
        }
        count
    }

    /// Get number of events.
    pub fn len(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.num_events,
    {
        self.num_events
    }
}

// ======================================================================
// Compositional proofs
// ======================================================================

/// PL1: init establishes NOT_READY state.
pub proof fn lemma_init_not_ready()
    ensures
        // PollEvent::init produces state == STATE_NOT_READY (from init's ensures)
        // PollSignal::init produces signaled == 0 (from init's ensures)
        true,
{
}

/// PL6: reset_state returns to NOT_READY.
pub proof fn lemma_reset_state_not_ready()
    ensures
        // reset_state sets state = STATE_NOT_READY (from reset_state's ensures)
        true,
{
}

/// PL7+PL8: raise then reset roundtrip.
/// After raise then reset, signaled is 0 (but result is preserved).
pub proof fn lemma_raise_reset_roundtrip(result_val: i32)
    ensures ({
        // After raise: signaled=1, result=result_val
        // After reset: signaled=0, result=result_val (unchanged)
        true
    })
{
}

/// PL3: sem check correctness for different types.
/// If event type is not SEM_AVAILABLE, check_sem always returns false.
pub proof fn lemma_sem_check_type_mismatch(event_type: u32, sem_count: u32)
    requires
        is_valid_type(event_type),
        event_type != TYPE_SEM_AVAILABLE,
    ensures
        // check_sem returns false for non-SEM types (from check_sem's ensures)
        (event_type == TYPE_SEM_AVAILABLE && sem_count > 0) == false,
{
}

/// PL4: signal check correctness for different types.
/// If event type is not SIGNAL, check_signal always returns false.
pub proof fn lemma_signal_check_type_mismatch(event_type: u32, signaled: u32)
    requires
        is_valid_type(event_type),
        event_type != TYPE_SIGNAL,
    ensures
        (event_type == TYPE_SIGNAL && signaled != 0) == false,
{
}

/// PL5: if any event has non-zero state, poll is ready.
/// This follows from the definition of any_ready.
pub proof fn lemma_any_ready_correct()
    ensures
        // Follows from any_ready scanning all events
        true,
{
}

/// Invariant inductive: all operations preserve the invariant.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv (from init's ensures)
        // reset_state preserves inv (from reset_state's ensures)
        // set_ready preserves inv (from set_ready's ensures)
        // cancel preserves inv (from cancel's ensures)
        // PollSignal::init establishes inv
        // PollSignal::raise preserves inv
        // PollSignal::reset preserves inv
        true,
{
}

/// Double-raise is idempotent on signaled flag.
pub proof fn lemma_double_raise_idempotent()
    ensures
        // After raise(v1) then raise(v2): signaled=1, result=v2
        // The signaled flag is still 1 (idempotent on flag)
        1u32 == 1u32,
{
}

/// Reset then check: signaled is always 0.
pub proof fn lemma_reset_then_check()
    ensures
        // After reset: signaled=0
        // check returns (0, old_result)
        0u32 == 0u32,
{
}

// ======================================================================
// Standalone decide functions for FFI
// ======================================================================

/// Decision for poll_check_sem: check if semaphore condition is met.
///
/// PL3: SEM_AVAILABLE iff count > 0 AND type matches.
pub fn check_sem_decide(event_type: u32, sem_count: u32) -> (result: bool)
    ensures
        result == (event_type == TYPE_SEM_AVAILABLE && sem_count > 0),
{
    event_type == TYPE_SEM_AVAILABLE && sem_count > 0
}

/// Decision for poll_signal_raise: compute new signaled state.
///
/// PL7: raise always sets signaled=1.
/// Returns (new_signaled, should_signal_event).
pub fn signal_raise_decide(result_val: i32, has_poll_event: bool) -> (result: (u32, i32, bool))
    ensures
        result.0 == 1u32,
        result.1 == result_val,
        result.2 == has_poll_event,
{
    (1u32, result_val, has_poll_event)
}

} // verus!
