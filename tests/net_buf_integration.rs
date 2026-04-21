//! Integration tests for the network buffer model.
//!
//! Tests verified properties NB1-NB6:
//!   NB1: alloc never exceeds pool capacity
//!   NB2: free returns buffer to pool
//!   NB3: ref count tracks owners
//!   NB4: data bounds: head_offset + len <= size
//!   NB5: push/pull preserve bounds
//!   NB6: no double-free

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::net_buf::{
    NetBufPool, NetBuf,
    alloc_decide, free_decide, ref_decide, unref_decide,
    add_decide, remove_decide, push_decide, pull_decide,
};

// =====================================================================
// NetBufPool tests — NB1, NB2
// =====================================================================

#[test]
fn pool_init_valid() {
    let p = NetBufPool::init(16).unwrap();
    assert_eq!(p.allocated_get(), 0);
    assert_eq!(p.free_get(), 16);
    assert_eq!(p.capacity_get(), 16);
    assert!(p.is_empty());
    assert!(!p.is_full());
}

#[test]
fn pool_init_rejects_zero_capacity() {
    assert_eq!(NetBufPool::init(0), Err(EINVAL));
}

#[test]
fn pool_alloc_one() {
    let mut p = NetBufPool::init(8).unwrap();
    assert_eq!(p.alloc(), OK);
    assert_eq!(p.allocated_get(), 1);
    assert_eq!(p.free_get(), 7);
}

/// NB1: alloc never exceeds capacity.
#[test]
fn pool_alloc_up_to_capacity() {
    let mut p = NetBufPool::init(4).unwrap();
    for i in 0..4u16 {
        assert_eq!(p.alloc(), OK);
        assert_eq!(p.allocated_get(), i + 1);
    }
    assert!(p.is_full());
    // NB1: one more alloc must fail
    assert_eq!(p.alloc(), ENOMEM);
    assert_eq!(p.allocated_get(), 4);
}

/// NB1: ENOMEM on exhausted pool.
#[test]
fn pool_alloc_when_full_returns_enomem() {
    let mut p = NetBufPool::init(2).unwrap();
    p.alloc();
    p.alloc();
    assert_eq!(p.alloc(), ENOMEM);
}

/// NB2: free returns buffer to pool.
#[test]
fn pool_free_decrements() {
    let mut p = NetBufPool::init(8).unwrap();
    p.alloc();
    p.alloc();
    assert_eq!(p.free(), OK);
    assert_eq!(p.allocated_get(), 1);
}

#[test]
fn pool_free_when_empty_returns_einval() {
    let mut p = NetBufPool::init(4).unwrap();
    assert_eq!(p.free(), EINVAL);
    assert_eq!(p.allocated_get(), 0);
}

/// NB1 conservation: free + allocated == capacity.
#[test]
fn pool_conservation_invariant() {
    let mut p = NetBufPool::init(10).unwrap();
    for _ in 0..6 {
        p.alloc();
    }
    assert_eq!(p.free_get() + p.allocated_get(), 10);
    for _ in 0..3 {
        p.free();
    }
    assert_eq!(p.free_get() + p.allocated_get(), 10);
}

/// NB2: alloc-free roundtrip returns to original state.
#[test]
fn pool_alloc_free_roundtrip() {
    let mut p = NetBufPool::init(8).unwrap();
    let original = p;
    p.alloc();
    p.free();
    assert_eq!(p, original);
}

#[test]
fn pool_stress_cycles() {
    let mut p = NetBufPool::init(16).unwrap();
    for _ in 0..50 {
        for _ in 0..16 {
            assert_eq!(p.alloc(), OK);
        }
        assert!(p.is_full());
        for _ in 0..16 {
            assert_eq!(p.free(), OK);
        }
        assert!(p.is_empty());
    }
}

// =====================================================================
// NetBuf tests — NB3, NB4, NB5, NB6
// =====================================================================

#[test]
fn buf_init_valid() {
    let b = NetBuf::init(256).unwrap();
    assert_eq!(b.size, 256);
    assert_eq!(b.head_offset, 0);
    assert_eq!(b.len, 0);
    assert_eq!(b.ref_count, 1);
}

#[test]
fn buf_init_rejects_zero_size() {
    assert_eq!(NetBuf::init(0), Err(EINVAL));
}

/// NB4: initial state satisfies head_offset + len <= size.
#[test]
fn buf_init_satisfies_nb4() {
    let b = NetBuf::init(64).unwrap();
    assert!(b.head_offset as u32 + b.len as u32 <= b.size as u32);
}

#[test]
fn buf_headroom_and_tailroom() {
    let b = NetBuf::init(64).unwrap();
    assert_eq!(b.headroom(), 0);
    assert_eq!(b.tailroom(), 64);
    assert_eq!(b.max_len(), 64);
}

/// NB4: add increases len, tailroom decreases.
#[test]
fn buf_add_increases_len() {
    let mut b = NetBuf::init(64).unwrap();
    assert_eq!(b.add(20), OK);
    assert_eq!(b.len, 20);
    assert_eq!(b.tailroom(), 44);
    assert_eq!(b.headroom(), 0);
}

/// NB4: add fails when tailroom insufficient.
#[test]
fn buf_add_rejects_overflow() {
    let mut b = NetBuf::init(32).unwrap();
    assert_eq!(b.add(16), OK);
    // Only 16 bytes tailroom left
    assert_eq!(b.add(17), ENOMEM);
    assert_eq!(b.len, 16);
}

/// NB4/NB5: add then remove roundtrip.
#[test]
fn buf_add_remove_roundtrip() {
    let mut b = NetBuf::init(64).unwrap();
    let original = b;
    b.add(30);
    b.remove(30);
    assert_eq!(b, original);
}

#[test]
fn buf_remove_when_empty_returns_einval() {
    let mut b = NetBuf::init(64).unwrap();
    assert_eq!(b.remove(1), EINVAL);
}

/// NB5: push moves head_offset back and increases len.
#[test]
fn buf_push_increases_headroom_use() {
    let mut b = NetBuf::init(64).unwrap();
    // Reserve 16 bytes headroom
    b.reset(16);
    assert_eq!(b.headroom(), 16);
    assert_eq!(b.push(8), OK);
    assert_eq!(b.head_offset, 8);
    assert_eq!(b.len, 8);
}

/// NB5: push fails when headroom insufficient.
#[test]
fn buf_push_rejects_no_headroom() {
    let mut b = NetBuf::init(64).unwrap();
    // head_offset = 0, no headroom
    assert_eq!(b.push(1), EINVAL);
    assert_eq!(b.head_offset, 0);
}

/// NB5: push-pull roundtrip.
#[test]
fn buf_push_pull_roundtrip() {
    let mut b = NetBuf::init(64).unwrap();
    b.reset(16);
    let before_head = b.head_offset;
    let before_len = b.len;
    b.push(8);
    b.pull(8);
    assert_eq!(b.head_offset, before_head);
    assert_eq!(b.len, before_len);
}

/// NB5: pull fails when len insufficient.
#[test]
fn buf_pull_rejects_more_than_len() {
    let mut b = NetBuf::init(64).unwrap();
    b.add(10);
    assert_eq!(b.pull(11), EINVAL);
    assert_eq!(b.len, 10);
}

/// NB4: all operations preserve bounds invariant.
#[test]
fn buf_bounds_invariant_preserved() {
    let mut b = NetBuf::init(128).unwrap();
    // Reserve headroom
    b.reset(32);
    // Add 40 bytes at tail
    b.add(40);
    // Verify: head_offset(32) + len(40) = 72 <= size(128)
    assert!(b.head_offset as u32 + b.len as u32 <= b.size as u32);
    // Push 16 bytes at head
    b.push(16);
    assert!(b.head_offset as u32 + b.len as u32 <= b.size as u32);
    // Pull 8 bytes from head
    b.pull(8);
    assert!(b.head_offset as u32 + b.len as u32 <= b.size as u32);
    // Remove 10 bytes from tail
    b.remove(10);
    assert!(b.head_offset as u32 + b.len as u32 <= b.size as u32);
}

/// NB3: ref count starts at 1, increments correctly.
#[test]
fn buf_ref_increments() {
    let mut b = NetBuf::init(64).unwrap();
    assert_eq!(b.ref_count, 1);
    assert_eq!(b.ref_get(), OK);
    assert_eq!(b.ref_count, 2);
    assert_eq!(b.ref_get(), OK);
    assert_eq!(b.ref_count, 3);
}

/// NB3: unref decrements correctly.
#[test]
fn buf_unref_decrements() {
    let mut b = NetBuf::init(64).unwrap();
    b.ref_get();
    b.ref_get();
    // ref_count = 3
    assert!(!b.unref()); // -> 2, not freed
    assert_eq!(b.ref_count, 2);
    assert!(!b.unref()); // -> 1, not freed
    assert_eq!(b.ref_count, 1);
}

/// NB3/NB6: unref to 0 returns should_free=true.
#[test]
fn buf_unref_to_zero_triggers_free() {
    let mut b = NetBuf::init(64).unwrap();
    assert!(b.unref()); // ref_count was 1, now 0, should_free=true
    assert_eq!(b.ref_count, 0);
}

/// NB3: ref overflow protection.
#[test]
fn buf_ref_overflow_protection() {
    let mut b = NetBuf::init(64).unwrap();
    b.ref_count = u8::MAX;
    assert_eq!(b.ref_get(), EOVERFLOW);
    assert_eq!(b.ref_count, u8::MAX);
}

/// NB6: double-free detection via unref_decide.
#[test]
fn unref_decide_rejects_zero_ref_count() {
    let result = unref_decide(0);
    assert_eq!(result, Err(EINVAL));
}

#[test]
fn buf_reset_with_headroom() {
    let mut b = NetBuf::init(64).unwrap();
    b.add(20);
    b.reset(8);
    assert_eq!(b.head_offset, 8);
    assert_eq!(b.len, 0);
    assert!(b.head_offset as u32 + b.len as u32 <= b.size as u32);
}

#[test]
fn buf_reset_rejects_too_large_reserve() {
    let mut b = NetBuf::init(64).unwrap();
    assert_eq!(b.reset(65), EINVAL);
}

// =====================================================================
// Standalone decide function tests
// =====================================================================

#[test]
fn alloc_decide_success() {
    assert_eq!(alloc_decide(3, 8), Ok(4));
}

#[test]
fn alloc_decide_full() {
    assert_eq!(alloc_decide(8, 8), Err(ENOMEM));
}

#[test]
fn free_decide_success() {
    assert_eq!(free_decide(5), Ok(4));
}

#[test]
fn free_decide_empty() {
    assert_eq!(free_decide(0), Err(EINVAL));
}

#[test]
fn ref_decide_increments() {
    assert_eq!(ref_decide(3), Ok(4));
}

#[test]
fn ref_decide_overflow() {
    assert_eq!(ref_decide(u8::MAX), Err(EOVERFLOW));
}

#[test]
fn unref_decide_decrements() {
    assert_eq!(unref_decide(3), Ok((2, false)));
}

#[test]
fn unref_decide_last_ref() {
    assert_eq!(unref_decide(1), Ok((0, true)));
}

#[test]
fn unref_decide_double_free() {
    assert_eq!(unref_decide(0), Err(EINVAL));
}

#[test]
fn add_decide_within_tailroom() {
    assert_eq!(add_decide(0, 10, 64, 20), Ok(30));
}

#[test]
fn add_decide_exact_tailroom() {
    // head_offset=0, len=54, size=64 -> tailroom=10; add 10
    assert_eq!(add_decide(0, 54, 64, 10), Ok(64));
}

#[test]
fn add_decide_exceeds_tailroom() {
    assert_eq!(add_decide(0, 54, 64, 11), Err(ENOMEM));
}

#[test]
fn remove_decide_success() {
    assert_eq!(remove_decide(20, 10), Ok(10));
}

#[test]
fn remove_decide_underflow() {
    assert_eq!(remove_decide(5, 10), Err(EINVAL));
}

#[test]
fn push_decide_success() {
    // head_offset=16, push 8 -> new_head=8, new_len=len+8
    assert_eq!(push_decide(16, 10, 8), Ok((8, 18)));
}

#[test]
fn push_decide_no_headroom() {
    assert_eq!(push_decide(0, 10, 8), Err(EINVAL));
}

#[test]
fn pull_decide_success() {
    // head_offset=8, len=20, size=64, pull 8
    assert_eq!(pull_decide(8, 20, 64, 8), Ok((16, 12)));
}

#[test]
fn pull_decide_more_than_len() {
    assert_eq!(pull_decide(0, 5, 64, 10), Err(EINVAL));
}

/// NB4: add_decide preserves head_offset + new_len <= size.
#[test]
fn add_decide_nb4_invariant() {
    let head_offset: u16 = 8;
    let len: u16 = 20;
    let size: u16 = 64;
    let bytes: u16 = 10;
    let new_len = add_decide(head_offset, len, size, bytes).unwrap();
    assert!(head_offset as u32 + new_len as u32 <= size as u32);
}

/// NB5: push_decide preserves bounds.
#[test]
fn push_decide_nb5_invariant() {
    let head_offset: u16 = 16;
    let len: u16 = 20;
    let size: u32 = 64;
    let bytes: u16 = 8;
    let (new_head, new_len) = push_decide(head_offset, len, bytes).unwrap();
    assert!(u32::from(new_head) + u32::from(new_len) <= size);
}

/// NB5: pull_decide preserves bounds.
#[test]
fn pull_decide_nb5_invariant() {
    let head_offset: u16 = 8;
    let len: u16 = 20;
    let size: u16 = 64;
    let bytes: u16 = 8;
    let (new_head, new_len) = pull_decide(head_offset, len, size, bytes).unwrap();
    assert!(new_head as u32 + new_len as u32 <= size as u32);
}
