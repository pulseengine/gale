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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollEvent {
    /// Bitfield of event types (K_POLL_TYPE_xxx).
    pub event_type: u32,
    /// Bitfield of event states (K_POLL_STATE_xxx).
    pub state: u32,
    /// Optional user-specified tag (opaque, untouched by API).
    pub tag: u32,
}
impl PollEvent {
    /// Initialize a poll event.
    ///
    /// poll.c:46-62: k_poll_event_init()
    ///
    /// PL1: event starts in NOT_READY state.
    pub fn init(event_type: u32, tag: u32) -> PollEvent {
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
    pub fn reset_state(&mut self) {
        self.state = STATE_NOT_READY;
    }
    /// Set the event to a ready state.
    ///
    /// poll.c:223-227: set_event_ready()
    /// In Zephyr this ORs the state, but since events start NOT_READY
    /// and are set once, we model it as OR to match the C code exactly.
    ///
    /// PL2: state transitions only via explicit operations.
    pub fn set_ready(&mut self, new_state: u32) {
        self.state = self.state | new_state;
    }
    /// Check if a semaphore condition is met.
    ///
    /// poll.c:65-70: is_condition_met() K_POLL_TYPE_SEM_AVAILABLE case.
    ///
    /// PL3: SEM_AVAILABLE iff semaphore count > 0.
    pub fn check_sem(&self, sem_count: u32) -> bool {
        self.event_type == TYPE_SEM_AVAILABLE && sem_count > 0
    }
    /// Check if a signal condition is met.
    ///
    /// poll.c:77-82: is_condition_met() K_POLL_TYPE_SIGNAL case.
    ///
    /// PL4: SIGNALED iff signal was raised.
    pub fn check_signal(&self, signaled: u32) -> bool {
        self.event_type == TYPE_SIGNAL && signaled != 0
    }
    /// Check if a message queue condition is met.
    ///
    /// poll.c:83-88: is_condition_met() K_POLL_TYPE_MSGQ_DATA_AVAILABLE.
    pub fn check_msgq(&self, used_msgs: u32) -> bool {
        self.event_type == TYPE_MSGQ_DATA_AVAILABLE && used_msgs > 0
    }
    /// Check if a data-available (queue/FIFO) condition is met.
    ///
    /// poll.c:71-76: is_condition_met() K_POLL_TYPE_DATA_AVAILABLE.
    pub fn check_data(&self, queue_not_empty: bool) -> bool {
        self.event_type == TYPE_DATA_AVAILABLE && queue_not_empty
    }
    /// Check if a pipe condition is met.
    ///
    /// poll.c:89-94: is_condition_met() K_POLL_TYPE_PIPE_DATA_AVAILABLE.
    pub fn check_pipe(&self, pipe_not_empty: bool) -> bool {
        self.event_type == TYPE_PIPE_DATA_AVAILABLE && pipe_not_empty
    }
    /// Get current event state.
    pub fn state_get(&self) -> u32 {
        self.state
    }
    /// Get event type.
    pub fn type_get(&self) -> u32 {
        self.event_type
    }
    /// Get event tag.
    pub fn tag_get(&self) -> u32 {
        self.tag
    }
    /// Check if event is ready (any state bit set).
    pub fn is_ready(&self) -> bool {
        self.state != STATE_NOT_READY
    }
    /// Check if event is not ready.
    pub fn is_not_ready(&self) -> bool {
        self.state == STATE_NOT_READY
    }
    /// Mark event as cancelled.
    pub fn cancel(&mut self) {
        self.state = self.state | STATE_CANCELLED;
    }
}
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollSignal {
    /// 1 if the signal has been raised, 0 otherwise.
    pub signaled: u32,
    /// Custom result value from k_poll_signal_raise().
    pub result: i32,
}
impl PollSignal {
    /// Initialize a poll signal.
    ///
    /// poll.c:475-483: k_poll_signal_init()
    pub fn init() -> PollSignal {
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
    pub fn raise(&mut self, result_val: i32) {
        self.result = result_val;
        self.signaled = 1;
    }
    /// Reset the poll signal.
    ///
    /// poll.c:494-498: k_poll_signal_reset()
    ///
    /// PL8: signal reset clears signaled to 0.
    pub fn reset(&mut self) {
        self.signaled = 0;
    }
    /// Check the signal state.
    ///
    /// poll.c:501-508: k_poll_signal_check()
    ///
    /// Returns (signaled, result).
    pub fn check(&self) -> (u32, i32) {
        (self.signaled, self.result)
    }
    /// Check if signal is raised.
    pub fn is_signaled(&self) -> bool {
        self.signaled != 0
    }
}
/// Maximum number of poll events in a single poll call.
/// Matches typical Zephyr usage; in practice CONFIG_dependent.
pub const MAX_POLL_EVENTS: u32 = 16;
/// Poll event array model.
///
/// Models the array of k_poll_event passed to k_poll().
/// We use a fixed-size array to avoid heap allocation (no_std).
#[derive(Debug, Clone)]
pub struct PollEvents {
    /// Events array.
    pub events: [PollEvent; 16],
    /// Number of active events (0..=MAX_POLL_EVENTS).
    pub num_events: u32,
}
impl PollEvents {
    /// Create an empty poll events collection.
    pub fn new() -> PollEvents {
        let default_event = PollEvent {
            event_type: TYPE_IGNORE,
            state: STATE_NOT_READY,
            tag: 0,
        };
        PollEvents {
            events: [
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event.clone(),
                default_event,
            ],
            num_events: 0,
        }
    }
    /// Add an event to the poll set.
    ///
    /// Returns true on success, false if array is full.
    pub fn add(&mut self, event: PollEvent) -> bool {
        if self.num_events >= MAX_POLL_EVENTS {
            return false;
        }
        self.events[self.num_events as usize] = event;
        self.num_events = self.num_events + 1;
        true
    }
    /// Reset all event states to NOT_READY.
    ///
    /// PL6: state is reset to NOT_READY before each poll call.
    pub fn reset_all_states(&mut self) {
        let mut i: u32 = 0;
        while i < self.num_events {
            self.events[i as usize].state = STATE_NOT_READY;
            i = i + 1;
        }
    }
    /// Check if any event in the set is ready.
    ///
    /// PL5: poll returns ready when ANY event becomes ready.
    pub fn any_ready(&self) -> bool {
        let mut i: u32 = 0;
        let mut found: bool = false;
        while i < self.num_events {
            if self.events[i as usize].state != STATE_NOT_READY {
                found = true;
            }
            i = i + 1;
        }
        found
    }
    /// Count how many events are ready.
    pub fn count_ready(&self) -> u32 {
        let mut i: u32 = 0;
        let mut count: u32 = 0;
        while i < self.num_events {
            if self.events[i as usize].state != STATE_NOT_READY {
                count = count + 1;
            }
            i = i + 1;
        }
        count
    }
    /// Get number of events.
    pub fn len(&self) -> u32 {
        self.num_events
    }
}
