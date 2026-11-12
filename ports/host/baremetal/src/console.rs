//! Polled transmit on an STM32-class USART, the console the Renode demonstrators
//! report their gate lines through.
//!
//! The register addresses are SoC data and come from the AADL target model
//! (targets/generated/gust_target_<soc>.rs: `USART1_SR`, `USART1_DR`), not from
//! this crate; until 2026-11-12 twelve binaries each hard-coded them. TXE is bit 7
//! of the status register on the F1-class USART (SR) and of the interrupt-and-status
//! register on the G4/L4-class USART (ISR), so only the addresses differ by SoC.

use core::ptr::{read_volatile, write_volatile};

/// Transmit-data-register-empty flag in SR (F1) / ISR (G4, L4).
pub const TXE: u32 = 1 << 7;

/// Write `bytes`, waiting for TXE before each one. `sr` and `dr` are the absolute
/// addresses of the status and data registers.
#[inline]
pub fn tx(sr: u32, dr: u32, bytes: &[u8]) {
    for &b in bytes {
        unsafe {
            while read_volatile(sr as *const u32) & TXE == 0 {}
            write_volatile(dr as *mut u32, (b as u32) & 0xFF);
        }
    }
}
