# Phase 1.5: Timer, Event, Mem_Slab — Design Spec

**Date:** 2026-03-15
**Goal:** Add formally verified Rust models for k_timer, k_event, k_mem_slab.

## Scope per primitive

| Primitive | Gale verifies | Stays in C |
|-----------|--------------|------------|
| k_timer | Status counter (increment/reset/sync), period validation | Timeout subsystem, ISR callbacks, scheduling |
| k_event | Bitmask ops (post OR, set replace, clear AND-NOT, wait ANY/ALL matching) | Wait queue, spinlock, ISR waking |
| k_mem_slab | Block count (alloc/free bounds, num_used/num_free conservation) | Free list pointers, buffer, pointer validation |

## ASIL-D Properties

### Timer (TM1-TM8)
- TM1: 0 <= status (non-negative expiry counter)
- TM2: status_get atomically reads and resets to 0
- TM3: start resets status to 0
- TM4: stop resets status to 0
- TM5: expiry increments status by exactly 1
- TM6: period == 0 means one-shot (no repeat)
- TM7: period > 0 means periodic (auto-restart)
- TM8: no arithmetic overflow on status increment

### Event (EV1-EV8)
- EV1: post ORs bits (events |= new_events)
- EV2: set replaces bits (events = new_events)
- EV3: clear ANDs complement (events &= ~clear_bits)
- EV4: set_masked applies mask (events = (events & ~mask) | (new & mask))
- EV5: wait_any succeeds when (events & desired) != 0
- EV6: wait_all succeeds when (events & desired) == desired
- EV7: 0 <= events <= u32::MAX (always valid bitmask)
- EV8: no bits lost during concurrent post (OR is monotonic)

### Mem_Slab (MS1-MS8)
- MS1: 0 <= num_used <= num_blocks (bounds invariant)
- MS2: num_blocks > 0 after init
- MS3: block_size > 0 after init
- MS4: alloc when not full: num_used += 1
- MS5: alloc when full: returns ENOMEM, state unchanged
- MS6: free: num_used -= 1
- MS7: num_free + num_used == num_blocks (conservation)
- MS8: no arithmetic overflow

## Provenance
- timer.c: SHA256=0318fa68..., 434 lines
- events.c: SHA256=85e77a43..., 448 lines
- mem_slab.c: SHA256=c60846d0..., 353 lines

## STPA Safety Analysis

### Timer Losses
- L-TM1: Timer fires after being stopped (stale callback execution)
- L-TM2: Status counter overflow causes missed expiry detection
- L-TM3: Periodic timer fails to restart

### Timer Hazards
- H-TM1: status incremented when timer is in STOPPED state
- H-TM2: status overflows u32::MAX
- H-TM3: period field corrupted between expiry and restart

### Timer Unsafe Control Actions
- UCA-TM1: expiry_fn called after k_timer_stop returns (race)
- UCA-TM2: status_get returns stale value (not atomically reset)
- UCA-TM3: start called with period=0 but timer auto-repeats

### Event Losses
- L-EV1: Event notification lost (thread never wakes)
- L-EV2: Spurious wake (thread wakes without matching events)
- L-EV3: Event bits corrupted by concurrent access

### Event Hazards
- H-EV1: wait_all returns when only partial bits set
- H-EV2: clear removes bits that a waiter needs
- H-EV3: post OR overwrites bits set by set operation

### Event Unsafe Control Actions
- UCA-EV1: wait_any returns 0 (no events matched)
- UCA-EV2: set_masked clears bits outside the mask
- UCA-EV3: clear called between post and waiter check

### Mem_Slab Losses
- L-MS1: Memory leak (block allocated but never freed)
- L-MS2: Double-free (block freed twice, corrupts free list)
- L-MS3: Use-after-free (block used after being freed)

### Mem_Slab Hazards
- H-MS1: num_used exceeds num_blocks
- H-MS2: num_used goes negative (underflow on free)
- H-MS3: alloc succeeds when no blocks available

### Mem_Slab Unsafe Control Actions
- UCA-MS1: alloc returns success but num_used not incremented
- UCA-MS2: free decrements num_used below 0
- UCA-MS3: free called on invalid pointer (not from this slab)
