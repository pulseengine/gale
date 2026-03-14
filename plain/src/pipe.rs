//! Plain Rust byte stream pipe for testing and Rocq-of-Rust translation.
//!
//! Identical logic to the Verus-annotated src/pipe.rs.
//! Any divergence between these files is a bug.
//!
//! Source mapping:
//!   k_pipe_init   -> Pipe::init         (pipe.c:67-85)
//!   k_pipe_write  -> Pipe::write_check  (pipe.c:147-218)
//!   k_pipe_read   -> Pipe::read_check   (pipe.c:220-271)
//!   k_pipe_reset  -> Pipe::reset        (pipe.c:273-285)
//!   k_pipe_close  -> Pipe::close        (pipe.c:287-296)

use crate::error::{EAGAIN, ECANCELED, EINVAL, ENOMSG, EPIPE};

/// Pipe flags — matches pipe.c PIPE_FLAG_*.
pub const FLAG_OPEN: u8 = 1;
pub const FLAG_RESET: u8 = 2;

/// Byte stream pipe — state machine + byte count model.
///
/// Models Zephyr's k_pipe state without ring buffer internals.
/// size = ring_buf.size, used = ring_buf_size_get(&buf).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pipe {
    pub size: u32,
    pub used: u32,
    pub flags: u8,
}

impl Pipe {
    /// Initialize a pipe with given buffer size.
    ///
    /// pipe.c:67-85
    pub fn init(size: u32) -> Result<Self, i32> {
        if size == 0 {
            return Err(EINVAL);
        }
        Ok(Pipe { size, used: 0, flags: FLAG_OPEN })
    }

    /// Validate a write and compute how many bytes can be written.
    ///
    /// Returns: Ok(n) bytes to write, or error code.
    pub fn write_check(&mut self, request_len: u32) -> Result<u32, i32> {
        // Resetting check
        if (self.flags & FLAG_RESET) != 0 {
            return Err(ECANCELED);
        }
        // Closed check
        if (self.flags & FLAG_OPEN) == 0 {
            return Err(EPIPE);
        }
        // Zero-length request
        if request_len == 0 {
            return Err(ENOMSG);
        }
        // Compute available space
        // Safe: used <= size (invariant)
        #[allow(clippy::arithmetic_side_effects)]
        let free = self.size - self.used;
        if free == 0 {
            return Err(EAGAIN);
        }
        let n = if request_len <= free { request_len } else { free };
        // Safe: used + n <= size (n <= free = size - used)
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.used += n;
        }
        Ok(n)
    }

    /// Validate a read and compute how many bytes can be read.
    ///
    /// Returns: Ok(n) bytes to read, or error code.
    pub fn read_check(&mut self, request_len: u32) -> Result<u32, i32> {
        // Resetting check
        if (self.flags & FLAG_RESET) != 0 {
            return Err(ECANCELED);
        }
        // Zero-length request
        if request_len == 0 {
            return Err(ENOMSG);
        }
        // Empty check
        if self.used == 0 {
            if (self.flags & FLAG_OPEN) == 0 {
                return Err(EPIPE);
            }
            return Err(EAGAIN);
        }
        let n = if request_len <= self.used { request_len } else { self.used };
        // Safe: used - n >= 0 (n <= used)
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.used -= n;
        }
        Ok(n)
    }

    /// Reset the pipe.
    ///
    /// pipe.c:273-285
    pub fn reset(&mut self) {
        self.used = 0;
        self.flags |= FLAG_RESET;
    }

    /// Close the pipe.
    ///
    /// pipe.c:287-296
    pub fn close(&mut self) {
        self.flags = 0;
    }

    /// Clear reset flag (called after all waiters have been woken).
    pub fn clear_reset(&mut self) {
        self.flags &= !FLAG_RESET;
    }

    /// Get free space in buffer.
    pub fn space_get(&self) -> u32 {
        // Safe: used <= size (invariant)
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

    /// Check if pipe buffer is full.
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

    /// Buffer size accessor.
    pub fn size(&self) -> u32 {
        self.size
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;

    #[test]
    fn test_init_valid() {
        let p = Pipe::init(64).unwrap();
        assert_eq!(p.data_get(), 0);
        assert_eq!(p.space_get(), 64);
        assert!(p.is_empty());
        assert!(!p.is_full());
        assert!(p.is_open());
        assert!(!p.is_resetting());
    }

    #[test]
    fn test_init_zero_size() {
        assert_eq!(Pipe::init(0), Err(EINVAL));
    }

    #[test]
    fn test_write_read_basic() {
        let mut p = Pipe::init(64).unwrap();
        let written = p.write_check(10).unwrap();
        assert_eq!(written, 10);
        assert_eq!(p.data_get(), 10);
        assert_eq!(p.space_get(), 54);

        let read = p.read_check(5).unwrap();
        assert_eq!(read, 5);
        assert_eq!(p.data_get(), 5);
    }

    #[test]
    fn test_write_clamped() {
        let mut p = Pipe::init(8).unwrap();
        let n = p.write_check(20).unwrap();
        assert_eq!(n, 8);
        assert!(p.is_full());
    }

    #[test]
    fn test_read_clamped() {
        let mut p = Pipe::init(8).unwrap();
        p.write_check(5).unwrap();
        let n = p.read_check(20).unwrap();
        assert_eq!(n, 5);
        assert!(p.is_empty());
    }

    #[test]
    fn test_write_full_returns_eagain() {
        let mut p = Pipe::init(4).unwrap();
        p.write_check(4).unwrap();
        assert_eq!(p.write_check(1), Err(EAGAIN));
    }

    #[test]
    fn test_read_empty_returns_eagain() {
        let mut p = Pipe::init(4).unwrap();
        assert_eq!(p.read_check(1), Err(EAGAIN));
    }

    #[test]
    fn test_write_closed_returns_epipe() {
        let mut p = Pipe::init(4).unwrap();
        p.close();
        assert_eq!(p.write_check(1), Err(EPIPE));
    }

    #[test]
    fn test_read_closed_empty_returns_epipe() {
        let mut p = Pipe::init(4).unwrap();
        p.close();
        assert_eq!(p.read_check(1), Err(EPIPE));
    }

    #[test]
    fn test_write_resetting_returns_ecanceled() {
        let mut p = Pipe::init(4).unwrap();
        p.reset();
        assert_eq!(p.write_check(1), Err(ECANCELED));
    }

    #[test]
    fn test_read_resetting_returns_ecanceled() {
        let mut p = Pipe::init(4).unwrap();
        p.write_check(2).unwrap();
        p.reset();
        assert_eq!(p.read_check(1), Err(ECANCELED));
    }

    #[test]
    fn test_reset_empties() {
        let mut p = Pipe::init(8).unwrap();
        p.write_check(5).unwrap();
        assert_eq!(p.data_get(), 5);
        p.reset();
        assert!(p.is_empty());
        assert!(p.is_resetting());
    }

    #[test]
    fn test_close_clears_flags() {
        let mut p = Pipe::init(4).unwrap();
        p.close();
        assert!(!p.is_open());
        assert!(!p.is_resetting());
        assert_eq!(p.flags, 0);
    }

    #[test]
    fn test_conservation() {
        let mut p = Pipe::init(16).unwrap();
        for len in [3, 5, 2, 6] {
            p.write_check(len).unwrap();
            assert_eq!(p.space_get() + p.data_get(), 16);
        }
        for len in [2, 4, 3] {
            p.read_check(len).unwrap();
            assert_eq!(p.space_get() + p.data_get(), 16);
        }
    }

    #[test]
    fn test_write_zero_returns_enomsg() {
        let mut p = Pipe::init(4).unwrap();
        assert_eq!(p.write_check(0), Err(ENOMSG));
    }

    #[test]
    fn test_read_zero_returns_enomsg() {
        let mut p = Pipe::init(4).unwrap();
        p.write_check(2).unwrap();
        assert_eq!(p.read_check(0), Err(ENOMSG));
    }

    #[test]
    fn test_clear_reset() {
        let mut p = Pipe::init(4).unwrap();
        p.reset();
        assert!(p.is_resetting());
        p.clear_reset();
        assert!(!p.is_resetting());
        assert!(p.is_open());
    }
}
