//! Integration tests for the byte stream pipe.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::pipe::Pipe;

#[test]
fn init_valid() {
    let p = Pipe::init(64).unwrap();
    assert_eq!(p.data_get(), 0);
    assert_eq!(p.space_get(), 64);
    assert!(p.is_empty());
    assert!(p.is_open());
}

#[test]
fn init_rejects_zero() {
    assert_eq!(Pipe::init(0), Err(EINVAL));
}

#[test]
fn write_read_basic() {
    let mut p = Pipe::init(32).unwrap();
    assert_eq!(p.write_check(10).unwrap(), 10);
    assert_eq!(p.data_get(), 10);
    assert_eq!(p.read_check(5).unwrap(), 5);
    assert_eq!(p.data_get(), 5);
}

#[test]
fn write_clamped_to_free_space() {
    let mut p = Pipe::init(8).unwrap();
    assert_eq!(p.write_check(20).unwrap(), 8);
    assert!(p.is_full());
}

#[test]
fn read_clamped_to_available() {
    let mut p = Pipe::init(8).unwrap();
    p.write_check(5).unwrap();
    assert_eq!(p.read_check(20).unwrap(), 5);
    assert!(p.is_empty());
}

#[test]
fn write_full_returns_eagain() {
    let mut p = Pipe::init(4).unwrap();
    p.write_check(4).unwrap();
    assert_eq!(p.write_check(1), Err(EAGAIN));
}

#[test]
fn read_empty_returns_eagain() {
    let mut p = Pipe::init(4).unwrap();
    assert_eq!(p.read_check(1), Err(EAGAIN));
}

#[test]
fn write_closed_returns_epipe() {
    let mut p = Pipe::init(4).unwrap();
    p.close();
    assert_eq!(p.write_check(1), Err(EPIPE));
}

#[test]
fn read_closed_empty_returns_epipe() {
    let mut p = Pipe::init(4).unwrap();
    p.close();
    assert_eq!(p.read_check(1), Err(EPIPE));
}

#[test]
fn write_resetting_returns_ecanceled() {
    let mut p = Pipe::init(4).unwrap();
    p.reset();
    assert_eq!(p.write_check(1), Err(ECANCELED));
}

#[test]
fn read_resetting_returns_ecanceled() {
    let mut p = Pipe::init(4).unwrap();
    p.write_check(2).unwrap();
    p.reset();
    assert_eq!(p.read_check(1), Err(ECANCELED));
}

#[test]
fn reset_empties_pipe() {
    let mut p = Pipe::init(8).unwrap();
    p.write_check(5).unwrap();
    p.reset();
    assert!(p.is_empty());
    assert!(p.is_resetting());
}

#[test]
fn close_clears_flags() {
    let mut p = Pipe::init(4).unwrap();
    p.close();
    assert!(!p.is_open());
    assert!(!p.is_resetting());
}

#[test]
fn conservation() {
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
fn write_zero_returns_enomsg() {
    let mut p = Pipe::init(4).unwrap();
    assert_eq!(p.write_check(0), Err(ENOMSG));
}

#[test]
fn read_zero_returns_enomsg() {
    let mut p = Pipe::init(4).unwrap();
    p.write_check(2).unwrap();
    assert_eq!(p.read_check(0), Err(ENOMSG));
}

#[test]
fn streaming_write_read() {
    let mut p = Pipe::init(10).unwrap();
    // Write 7 bytes in chunks
    assert_eq!(p.write_check(3).unwrap(), 3);
    assert_eq!(p.write_check(4).unwrap(), 4);
    assert_eq!(p.data_get(), 7);
    // Read 5 bytes
    assert_eq!(p.read_check(5).unwrap(), 5);
    assert_eq!(p.data_get(), 2);
    // Write 8 more (clamped to 8)
    assert_eq!(p.write_check(8).unwrap(), 8);
    assert!(p.is_full());
}

#[test]
fn stress_write_read_cycles() {
    let mut p = Pipe::init(32).unwrap();
    for _ in 0..100 {
        let n = p.write_check(15).unwrap();
        assert!(n > 0 && n <= 15);
        let m = p.read_check(10).unwrap();
        assert!(m > 0 && m <= 10);
        assert_eq!(p.space_get() + p.data_get(), 32);
    }
}
