//! ARMv7-M port layer for gust (rework step 3, docs/architecture/three-modes-two-axes.md).
//!
//! The `gust:hal/mmio` bridge. A dissolved driver object leaves its MMIO accesses as
//! undefined symbols named after the interface FIELDS, `read32` and `write32` (not the
//! older `mmio_*` extern names), and the binary that links the object supplies them.
//! Until this crate existed, 19 gust binaries each carried their own copy of the same
//! two functions; they now link this one.
//!
//! A binary that links a dissolved driver must name this crate (`use gust_armv7m as _;`)
//! so rustc links it: an unreferenced dependency is not linked, and the object's
//! `read32`/`write32` would then be undefined at link time.
#![no_std]

use core::ptr::{read_volatile, write_volatile};

/// `gust:hal/mmio.read32`: one volatile 32-bit load.
#[export_name = "read32"]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}

/// `gust:hal/mmio.write32`: one volatile 32-bit store.
#[export_name = "write32"]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}
