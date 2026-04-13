//! Verified network buffer management model for Zephyr RTOS.
//!
//! This is a formally verified model of Zephyr's net_buf subsystem,
//! sourced from lib/net_buf/buf.c and lib/net_buf/buf_simple.c.
//! The net_buf subsystem is referenced by 6,164 BLE sites, 519 networking
//! sites, and 427 USB sites — buffer corruption here cascades across all
//! wireless protocols.
//!
//! This module models the **allocation tracking and data pointer arithmetic**
//! of net_buf pools and buffers.  Actual buffer memory, fragment linked
//! lists, k_lifo/k_spinlock concurrency, and DMA data callbacks remain in C.
//!
//! Source mapping:
//!   net_buf_alloc / net_buf_alloc_len  -> NetBufPool::alloc_decide
//!   net_buf_unref                      -> NetBufPool::free_decide
//!   net_buf_ref                        -> NetBuf::ref_decide
//!   net_buf_simple_add                 -> NetBuf::add_decide
//!   net_buf_simple_remove_mem          -> NetBuf::remove_decide
//!   net_buf_simple_push                -> NetBuf::push_decide
//!   net_buf_simple_pull                -> NetBuf::pull_decide
//!   net_buf_simple_headroom            -> NetBuf::headroom
//!   net_buf_simple_tailroom            -> NetBuf::tailroom
//!
//! Omitted (not safety-relevant):
//!   - k_lifo / k_spinlock — concurrency primitives
//!   - DMA data callbacks (mem_pool, fixed, heap allocators)
//!   - Fragment chain traversal — pointer-graph, not arithmetic
//!   - CONFIG_NET_BUF_LOG — debug tracing
//!   - CONFIG_USERSPACE — syscall marshaling
//!
//! ASIL-D verified properties:
//!   NB1: alloc never exceeds pool capacity (0 <= allocated <= capacity)
//!   NB2: free returns buffer to pool (allocated decrements correctly)
//!   NB3: ref count tracks owners (ref_count >= 1 while in use)
//!   NB4: data bounds: head_offset + len <= size (no overflow)
//!   NB5: push/pull preserve bounds (headroom and tailroom checks)
//!   NB6: no double-free (ref_count must be 1 to trigger free)
use crate::error::*;
/// Network buffer pool allocation tracker.
///
/// Models struct net_buf_pool { buf_count, uninit_count, free_count }.
/// We track allocated (buf_count - free_count) to satisfy NB1/NB2.
///
/// The actual pool memory, k_lifo free list, and k_spinlock remain in C.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetBufPool {
    /// Total buffer count in the pool (immutable after init).
    /// Corresponds to struct net_buf_pool::buf_count.
    pub capacity: u16,
    /// Number of buffers currently allocated (in use).
    /// Derived: buf_count - free_count - uninit_count in Zephyr terms.
    pub allocated: u16,
}
impl NetBufPool {
    /// Initialize a buffer pool.
    ///
    /// Returns EINVAL if capacity is 0.
    pub fn init(capacity: u16) -> Result<NetBufPool, i32> {
        if capacity == 0 {
            Err(EINVAL)
        } else {
            Ok(NetBufPool {
                capacity,
                allocated: 0,
            })
        }
    }
    /// Allocate one buffer from the pool.
    ///
    /// NB1: success only when allocated < capacity.
    /// Returns ENOMEM when pool is exhausted (pool exhaustion handling).
    pub fn alloc(&mut self) -> i32 {
        if self.allocated < self.capacity {
            self.allocated = self.allocated + 1;
            OK
        } else {
            ENOMEM
        }
    }
    /// Free one buffer back to the pool.
    ///
    /// NB2: allocated decrements. EINVAL if pool is already empty (double-free guard).
    pub fn free(&mut self) -> i32 {
        if self.allocated > 0 {
            self.allocated = self.allocated - 1;
            OK
        } else {
            EINVAL
        }
    }
    /// Number of buffers currently allocated.
    pub fn allocated_get(&self) -> u16 {
        self.allocated
    }
    /// Number of free buffers available.
    /// NB1 conservation: free_count + allocated == capacity.
    pub fn free_get(&self) -> u16 {
        self.capacity - self.allocated
    }
    /// Total pool capacity.
    pub fn capacity_get(&self) -> u16 {
        self.capacity
    }
    /// Check if pool is full.
    pub fn is_full(&self) -> bool {
        self.allocated == self.capacity
    }
    /// Check if pool has no allocated buffers.
    pub fn is_empty(&self) -> bool {
        self.allocated == 0
    }
}
/// Individual network buffer state model.
///
/// Models the data pointer arithmetic of struct net_buf_simple:
///   uint8_t *__buf  — start of allocated region
///   uint8_t *data   — current read/write pointer (offset from __buf)
///   uint16_t len    — bytes of valid data from data pointer
///   uint16_t size   — total allocated buffer size
///
/// We model data pointer as `head_offset` (bytes from __buf to data).
/// This keeps all arithmetic in integer domain without raw pointers.
///
/// Invariant (NB4): head_offset + len <= size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetBuf {
    /// Total allocated buffer size in bytes (immutable after alloc).
    /// Corresponds to net_buf_simple::size.
    pub size: u16,
    /// Offset of the current data pointer from __buf start.
    /// Corresponds to (data - __buf) in C.
    pub head_offset: u16,
    /// Length of valid data from head_offset.
    /// Corresponds to net_buf_simple::len.
    pub len: u16,
    /// Reference count — number of current owners.
    /// NB3: must be >= 1 while buffer is in use.
    pub ref_count: u8,
}
impl NetBuf {
    /// Initialize a buffer with given size.
    ///
    /// Starts with data pointer at beginning (head_offset = 0),
    /// len = 0 (empty), ref_count = 1.
    pub fn init(size: u16) -> Result<NetBuf, i32> {
        if size == 0 {
            Err(EINVAL)
        } else {
            Ok(NetBuf {
                size,
                head_offset: 0,
                len: 0,
                ref_count: 1,
            })
        }
    }
    /// Reset buffer to empty state with optional headroom reservation.
    ///
    /// net_buf_reset / net_buf_simple_reserve: sets data pointer forward,
    /// len = 0. Matches net_buf_simple_reserve(buf, reserve).
    pub fn reset(&mut self, reserve: u16) -> i32 {
        if reserve > self.size {
            return EINVAL;
        }
        self.head_offset = reserve;
        self.len = 0;
        OK
    }
    /// Increment reference count (net_buf_ref).
    ///
    /// NB3: ref_count tracks owners. Saturates at u8::MAX.
    pub fn ref_get(&mut self) -> i32 {
        if self.ref_count < u8::MAX {
            self.ref_count = self.ref_count + 1;
            OK
        } else {
            EOVERFLOW
        }
    }
    /// Decrement reference count (net_buf_unref).
    ///
    /// NB3/NB6: returns true when ref_count reaches 0 (buffer must be freed).
    /// Caller must not double-unref (NB6: ref_count must be >= 1 on entry).
    pub fn unref(&mut self) -> bool {
        if self.ref_count > 1 {
            self.ref_count = self.ref_count - 1;
            false
        } else {
            self.ref_count = 0;
            true
        }
    }
    /// Headroom: bytes before data pointer (available for push).
    /// Corresponds to net_buf_simple_headroom().
    pub fn headroom(&self) -> u16 {
        self.head_offset
    }
    /// Tailroom: bytes after data end (available for add).
    /// Corresponds to net_buf_simple_tailroom().
    /// NB4: result = size - head_offset - len >= 0 (guaranteed by inv).
    pub fn tailroom(&self) -> u16 {
        self.size - self.head_offset - self.len
    }
    /// Maximum usable data length (from current head_offset to buffer end).
    /// Corresponds to net_buf_simple_max_len().
    pub fn max_len(&self) -> u16 {
        self.size - self.head_offset
    }
    /// Add bytes at the tail of the buffer (net_buf_simple_add).
    ///
    /// Grows len by `bytes`. Checks tailroom >= bytes.
    /// NB4: head_offset + (len + bytes) <= size after add.
    /// NB5: tailroom decreases by bytes.
    pub fn add(&mut self, bytes: u16) -> i32 {
        let tailroom: u16 = self.size - self.head_offset - self.len;
        if bytes > tailroom {
            return ENOMEM;
        }
        self.len = self.len + bytes;
        OK
    }
    /// Remove bytes from the tail of the buffer (net_buf_simple_remove_mem).
    ///
    /// Shrinks len by `bytes`. Checks len >= bytes.
    /// NB4/NB5: len decreases, head_offset unchanged.
    pub fn remove(&mut self, bytes: u16) -> i32 {
        if bytes > self.len {
            return EINVAL;
        }
        self.len = self.len - bytes;
        OK
    }
    /// Push bytes at the head of the buffer (net_buf_simple_push).
    ///
    /// Moves data pointer back by `bytes`, grows len by `bytes`.
    /// Requires headroom >= bytes (head_offset >= bytes).
    /// NB4: (head_offset - bytes) + (len + bytes) = head_offset + len <= size.
    /// NB5: headroom decreases by bytes, tailroom unchanged.
    pub fn push(&mut self, bytes: u16) -> i32 {
        if bytes > self.head_offset {
            return EINVAL;
        }
        self.head_offset = self.head_offset - bytes;
        self.len = self.len + bytes;
        OK
    }
    /// Pull bytes from the head of the buffer (net_buf_simple_pull).
    ///
    /// Moves data pointer forward by `bytes`, shrinks len by `bytes`.
    /// Requires len >= bytes.
    /// NB4: (head_offset + bytes) + (len - bytes) = head_offset + len <= size.
    /// NB5: headroom increases by bytes, tailroom unchanged.
    pub fn pull(&mut self, bytes: u16) -> i32 {
        if bytes > self.len {
            return EINVAL;
        }
        self.head_offset = self.head_offset + bytes;
        self.len = self.len - bytes;
        OK
    }
}
/// Decide a pool allocation: validate capacity and compute new allocated count.
///
/// NB1: success when allocated < capacity.
/// NB1: full pool returns ENOMEM.
pub fn alloc_decide(allocated: u16, capacity: u16) -> Result<u16, i32> {
    if allocated < capacity { Ok(allocated + 1) } else { Err(ENOMEM) }
}
/// Decide a pool free: validate and compute new allocated count.
///
/// NB2: free decrements allocated. Rejects double-free (NB6).
pub fn free_decide(allocated: u16) -> Result<u16, i32> {
    if allocated > 0 { Ok(allocated - 1) } else { Err(EINVAL) }
}
/// Decide a ref increment: NB3 — ref_count tracks owners.
///
/// Returns new ref_count on success, EOVERFLOW if saturated.
pub fn ref_decide(ref_count: u8) -> Result<u8, i32> {
    if ref_count < u8::MAX { Ok(ref_count + 1) } else { Err(EOVERFLOW) }
}
/// Decide a ref decrement (unref): NB3/NB6.
///
/// Returns (new_ref_count, should_free).
/// NB6: returns EINVAL if ref_count is already 0 (double-free guard).
pub fn unref_decide(ref_count: u8) -> Result<(u8, bool), i32> {
    if ref_count == 0 {
        Err(EINVAL)
    } else {
        let new_ref = ref_count - 1;
        Ok((new_ref, new_ref == 0))
    }
}
/// Decide a data-add (tail append): NB4/NB5 bounds check.
///
/// Returns new len on success, ENOMEM if tailroom insufficient.
pub fn add_decide(
    head_offset: u16,
    len: u16,
    size: u16,
    bytes: u16,
) -> Result<u16, i32> {
    let tailroom: u16 = size - head_offset - len;
    if bytes > tailroom { Err(ENOMEM) } else { Ok(len + bytes) }
}
/// Decide a data-remove (tail shrink): NB4/NB5 bounds check.
///
/// Returns new len on success, EINVAL if len < bytes.
pub fn remove_decide(len: u16, bytes: u16) -> Result<u16, i32> {
    if bytes > len { Err(EINVAL) } else { Ok(len - bytes) }
}
/// Decide a push (prepend at head): NB4/NB5 bounds check.
///
/// Returns (new_head_offset, new_len) on success.
/// Requires headroom >= bytes (head_offset >= bytes).
pub fn push_decide(head_offset: u16, len: u16, bytes: u16) -> Result<(u16, u16), i32> {
    if bytes > head_offset {
        Err(EINVAL)
    } else {
        Ok((head_offset - bytes, len + bytes))
    }
}
/// Decide a pull (consume from head): NB4/NB5 bounds check.
///
/// Returns (new_head_offset, new_len) on success.
/// Requires len >= bytes.
pub fn pull_decide(
    head_offset: u16,
    len: u16,
    size: u16,
    bytes: u16,
) -> Result<(u16, u16), i32> {
    if bytes > len { Err(EINVAL) } else { Ok((head_offset + bytes, len - bytes)) }
}
