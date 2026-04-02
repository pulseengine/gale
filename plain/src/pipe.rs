//! Verified byte stream pipe for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/pipe.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **state machine and byte count tracking** of
//! Zephyr's pipe.  The actual ring buffer (head/tail/base indices) and
//! data transfer (memcpy) remain in C via Zephyr's ring_buf subsystem.
//!
//! Source mapping:
//!   k_pipe_init   -> Pipe::init         (pipe.c:67-85)
//!   k_pipe_write  -> Pipe::write_check  (pipe.c:147-218, state/count validation)
//!   k_pipe_read   -> Pipe::read_check   (pipe.c:220-271, state/count validation)
//!   k_pipe_reset  -> Pipe::reset        (pipe.c:273-285)
//!   k_pipe_close  -> Pipe::close        (pipe.c:287-296)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_POLL (poll_events) — application convenience
//!   - CONFIG_OBJ_CORE_PIPE — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - CONFIG_KERNEL_COHERENCE — cache coherency optimization
//!   - copy_to_pending_readers — direct-copy optimization
//!
//! ASIL-D verified properties:
//!   PP1: 0 <= used <= size (capacity invariant)
//!   PP2: size > 0 (always after init)
//!   PP3: write_check on closed pipe: returns EPIPE
//!   PP4: write_check on resetting pipe: returns ECANCELED
//!   PP5: write_check computes correct byte count (min of request and free)
//!   PP6: read_check computes correct byte count (min of request and used)
//!   PP7: reset sets used to 0
//!   PP8: close clears open flag
//!   PP9: conservation: used + free == size
//!   PP10: no arithmetic overflow in any operation
use crate::error::*;
/// Pipe flags — matches pipe.c PIPE_FLAG_*.
pub const FLAG_OPEN: u8 = 1;
pub const FLAG_RESET: u8 = 2;
/// Pipe state machine + byte count model.
///
/// Corresponds to Zephyr's struct k_pipe {
///     size_t waiting;
///     struct ring_buf buf;   // we model as size + used
///     uint8_t flags;
/// };
///
/// The ring buffer internals (head/tail/base indices) stay in C.
/// We model only the byte-level state: total size and bytes used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pipe {
    /// Total buffer capacity in bytes (immutable after init).
    pub size: u32,
    /// Bytes currently in the buffer.
    pub used: u32,
    /// Pipe state flags (OPEN, RESET).
    pub flags: u8,
}
impl Pipe {
    /// Initialize a pipe with given buffer size.
    ///
    /// pipe.c:67-85
    pub fn init(size: u32) -> Result<Pipe, i32> {
        if size == 0 {
            Err(EINVAL)
        } else {
            Ok(Pipe {
                size,
                used: 0,
                flags: FLAG_OPEN,
            })
        }
    }
    /// Validate a write and compute how many bytes can be written.
    ///
    /// pipe.c:147-218 (state check + ring_buf_put result)
    ///
    /// Returns:
    ///   Ok(n)  — n bytes can be written (0 < n <= requested)
    ///   Err(EPIPE) — pipe closed
    ///   Err(ECANCELED) — pipe resetting
    ///   Err(EAGAIN) — no space (pipe full)
    ///   Err(ENOMSG) — zero-length write request
    pub fn write_check(&mut self, request_len: u32) -> Result<u32, i32> {
        if (self.flags & FLAG_RESET) != 0 {
            self.flags = self.flags;
            return Err(ECANCELED);
        }
        if (self.flags & FLAG_OPEN) == 0 {
            return Err(EPIPE);
        }
        if request_len == 0 {
            return Err(ENOMSG);
        }
        #[allow(clippy::arithmetic_side_effects)]
        let free = self.size - self.used;
        if free == 0 {
            return Err(EAGAIN);
        }
        let n = if request_len <= free { request_len } else { free };
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.used = self.used + n;
        }
        Ok(n)
    }
    /// Validate a read and compute how many bytes can be read.
    ///
    /// pipe.c:220-271 (state check + ring_buf_get result)
    ///
    /// Returns:
    ///   Ok(n)  — n bytes can be read (0 < n <= requested)
    ///   Err(EPIPE) — pipe closed and empty
    ///   Err(ECANCELED) — pipe resetting
    ///   Err(EAGAIN) — no data (pipe empty)
    ///   Err(ENOMSG) — zero-length read request
    pub fn read_check(&mut self, request_len: u32) -> Result<u32, i32> {
        if (self.flags & FLAG_RESET) != 0 {
            return Err(ECANCELED);
        }
        if request_len == 0 {
            return Err(ENOMSG);
        }
        if self.used == 0 {
            if (self.flags & FLAG_OPEN) == 0 {
                return Err(EPIPE);
            }
            return Err(EAGAIN);
        }
        let n = if request_len <= self.used { request_len } else { self.used };
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.used = self.used - n;
        }
        Ok(n)
    }
    /// Reset the pipe (discard all data).
    ///
    /// pipe.c:273-285:
    ///   ring_buf_reset(&pipe->buf);
    ///   pipe->flags |= PIPE_FLAG_RESET;
    pub fn reset(&mut self) {
        self.used = 0;
        self.flags = self.flags | FLAG_RESET;
    }
    /// Close the pipe.
    ///
    /// pipe.c:287-296:
    ///   pipe->flags = 0;
    pub fn close(&mut self) {
        self.flags = 0;
    }
    /// Get free space in buffer.
    pub fn space_get(&self) -> u32 {
        #[allow(clippy::arithmetic_side_effects)]
        let r = self.size - self.used;
        r
    }
    /// Get bytes available for reading.
    pub fn data_get(&self) -> u32 {
        self.used
    }
    /// Check if pipe buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.used == 0
    }
    /// Check if pipe is full.
    pub fn is_full(&self) -> bool {
        self.used == self.size
    }
    /// Check if pipe is open.
    pub fn is_open(&self) -> bool {
        (self.flags & FLAG_OPEN) != 0
    }
    /// Check if pipe is resetting.
    pub fn is_resetting(&self) -> bool {
        (self.flags & FLAG_RESET) != 0
    }
    /// Get pipe buffer size.
    pub fn size(&self) -> u32 {
        self.size
    }
    /// Clear the reset flag after reset is complete.
    pub fn clear_reset(&mut self) {
        self.flags = self.flags & !FLAG_RESET;
    }
}
/// Lightweight write decision — no WaitQueue allocation.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum WriteDecision {
    /// Write OK: n bytes can be written to ring buffer.
    WriteOk = 0,
    /// Wake a blocked reader (buffer was empty, reader is waiting).
    WakeReader = 1,
    /// Buffer full or zero-size pipe: pend current thread.
    WritePend = 2,
    /// Error: pipe closed or resetting.
    WriteError = 3,
}
/// Result of a write decision with byte counts.
#[derive(Debug)]
pub struct WriteDecideResult {
    pub decision: WriteDecision,
    pub ret: i32,
    pub actual_bytes: u32,
    pub new_used: u32,
}
/// Lightweight read decision — no WaitQueue allocation.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReadDecision {
    /// Read OK: n bytes can be read from ring buffer.
    ReadOk = 0,
    /// Wake a blocked writer (buffer was full, writer is waiting).
    WakeWriter = 1,
    /// Buffer empty: pend current thread.
    ReadPend = 2,
    /// Error: pipe closed or resetting.
    ReadError = 3,
}
/// Result of a read decision with byte counts.
#[derive(Debug)]
pub struct ReadDecideResult {
    pub decision: ReadDecision,
    pub ret: i32,
    pub actual_bytes: u32,
    pub new_used: u32,
}
/// Lightweight write decision — takes scalars, no WaitQueue allocation.
///
/// Verified properties (PP3, PP4, PP5, PP9, PP10):
/// - resetting ==> WriteError(ECANCELED)
/// - !open ==> WriteError(EPIPE)
/// - empty && has_reader ==> WakeReader
/// - full || size==0 ==> WritePend
/// - otherwise ==> WriteOk with min(request, free) bytes
pub fn write_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_reader: bool,
) -> WriteDecideResult {
    if (flags & FLAG_RESET) != 0 {
        return WriteDecideResult {
            decision: WriteDecision::WriteError,
            ret: ECANCELED,
            actual_bytes: 0,
            new_used: used,
        };
    }
    if (flags & FLAG_OPEN) == 0 {
        return WriteDecideResult {
            decision: WriteDecision::WriteError,
            ret: EPIPE,
            actual_bytes: 0,
            new_used: used,
        };
    }
    if used == 0 && has_reader {
        let actual = if size > 0 && request_len > 0 {
            if request_len <= size { request_len } else { size }
        } else {
            0u32
        };
        return WriteDecideResult {
            decision: WriteDecision::WakeReader,
            ret: OK,
            actual_bytes: actual,
            new_used: 0,
        };
    }
    if size == 0 || used >= size {
        return WriteDecideResult {
            decision: WriteDecision::WritePend,
            ret: OK,
            actual_bytes: 0,
            new_used: used,
        };
    }
    let free = size - used;
    let n = if request_len <= free { request_len } else { free };
    let nu = used + n;
    WriteDecideResult {
        decision: WriteDecision::WriteOk,
        ret: OK,
        actual_bytes: n,
        new_used: nu,
    }
}
/// Lightweight read decision — takes scalars, no WaitQueue allocation.
///
/// Verified properties (PP3, PP4, PP6, PP9, PP10):
/// - resetting ==> ReadError(ECANCELED)
/// - full && has_writer ==> WakeWriter
/// - data available ==> ReadOk with min(request, used) bytes
/// - empty && closed ==> ReadError(EPIPE)
/// - empty && open ==> ReadPend
pub fn read_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_writer: bool,
) -> ReadDecideResult {
    if (flags & FLAG_RESET) != 0 {
        return ReadDecideResult {
            decision: ReadDecision::ReadError,
            ret: ECANCELED,
            actual_bytes: 0,
            new_used: used,
        };
    }
    if used >= size && has_writer {
        let n = if request_len <= used { request_len } else { used };
        let nu = used - n;
        return ReadDecideResult {
            decision: ReadDecision::WakeWriter,
            ret: OK,
            actual_bytes: n,
            new_used: nu,
        };
    }
    if used > 0 {
        let n = if request_len <= used { request_len } else { used };
        let nu = used - n;
        return ReadDecideResult {
            decision: ReadDecision::ReadOk,
            ret: OK,
            actual_bytes: n,
            new_used: nu,
        };
    }
    if (flags & FLAG_OPEN) == 0 {
        return ReadDecideResult {
            decision: ReadDecision::ReadError,
            ret: EPIPE,
            actual_bytes: 0,
            new_used: 0,
        };
    }
    ReadDecideResult {
        decision: ReadDecision::ReadPend,
        ret: OK,
        actual_bytes: 0,
        new_used: 0,
    }
}
