//! Verified ARM Cortex-M fault register decode model for Zephyr RTOS.
//!
//! This is a formally verified model of ARM Cortex-M fault register
//! decoding. All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **CFSR/HFSR/MMFAR/BFAR decode logic** used by
//! arch_system_halt and fault handlers. The actual register reads and
//! exception entry remain in architecture-specific C code.
//!
//! Source mapping:
//!   SCB->CFSR   -> CortexMFault.cfsr     (ARMv7-M Architecture Reference)
//!   SCB->HFSR   -> CortexMFault.hfsr     (ARMv7-M Architecture Reference)
//!   SCB->MMFAR  -> CortexMFault.mmfar    (ARMv7-M Architecture Reference)
//!   SCB->BFAR   -> CortexMFault.bfar     (ARMv7-M Architecture Reference)
//!
//! CFSR bit-field layout (ARMv7-M B3.2.15):
//!   Bits  0-7:  MemManage Fault Status Register (MMFSR)
//!   Bits  8-15: BusFault Status Register (BFSR)
//!   Bits 16-31: UsageFault Status Register (UFSR)
//!
//! Omitted (not safety-relevant):
//!   - AFSR (Auxiliary Fault Status Register) — implementation-defined
//!   - Debug fault registers (DFSR) — debug infrastructure
//!   - SecureFault (ARMv8-M only) — TrustZone extensions
//!   - Actual register read/write — hardware access
//!
//! ASIL-D verified properties:
//!   FH1: CFSR decode is exhaustive (all bit combinations mapped)
//!   FH2: fault address valid only when MMARVALID/BFARVALID set
//!   FH3: HardFault FORCED bit indicates escalated fault
//!
//! Extends the existing fatal.rs module with hardware-specific fault
//! register decoding for ARM Cortex-M targets.
/// IACCVIOL: instruction access violation.
pub const MMFSR_IACCVIOL: u32 = 1u32 << 0u32;
/// DACCVIOL: data access violation.
pub const MMFSR_DACCVIOL: u32 = 1u32 << 1u32;
/// MUNSTKERR: MemManage fault on unstacking (exception return).
pub const MMFSR_MUNSTKERR: u32 = 1u32 << 3u32;
/// MSTKERR: MemManage fault on stacking (exception entry).
pub const MMFSR_MSTKERR: u32 = 1u32 << 4u32;
/// MLSPERR: MemManage fault during lazy FP state preservation.
pub const MMFSR_MLSPERR: u32 = 1u32 << 5u32;
/// MMARVALID: MMFAR holds a valid fault address.
pub const MMFSR_MMARVALID: u32 = 1u32 << 7u32;
/// IBUSERR: instruction bus error.
pub const BFSR_IBUSERR: u32 = 1u32 << 8u32;
/// PRECISERR: precise data bus error.
pub const BFSR_PRECISERR: u32 = 1u32 << 9u32;
/// IMPRECISERR: imprecise data bus error.
pub const BFSR_IMPRECISERR: u32 = 1u32 << 10u32;
/// UNSTKERR: bus fault on unstacking.
pub const BFSR_UNSTKERR: u32 = 1u32 << 11u32;
/// STKERR: bus fault on stacking.
pub const BFSR_STKERR: u32 = 1u32 << 12u32;
/// LSPERR: bus fault during lazy FP state preservation.
pub const BFSR_LSPERR: u32 = 1u32 << 13u32;
/// BFARVALID: BFAR holds a valid fault address.
pub const BFSR_BFARVALID: u32 = 1u32 << 15u32;
/// UNDEFINSTR: undefined instruction.
pub const UFSR_UNDEFINSTR: u32 = 1u32 << 16u32;
/// INVSTATE: invalid state (e.g., ARM mode on Thumb-only core).
pub const UFSR_INVSTATE: u32 = 1u32 << 17u32;
/// INVPC: invalid PC load via EXC_RETURN.
pub const UFSR_INVPC: u32 = 1u32 << 18u32;
/// NOCP: no coprocessor (attempted access to unavailable CP).
pub const UFSR_NOCP: u32 = 1u32 << 19u32;
/// STKOF: stack overflow (ARMv8-M only, bit 20).
pub const UFSR_STKOF: u32 = 1u32 << 20u32;
/// UNALIGNED: unaligned memory access.
pub const UFSR_UNALIGNED: u32 = 1u32 << 24u32;
/// DIVBYZERO: integer divide by zero.
pub const UFSR_DIVBYZERO: u32 = 1u32 << 25u32;
/// VECTTBL: bus fault on vector table read.
pub const HFSR_VECTTBL: u32 = 1u32 << 1u32;
/// FORCED: forced HardFault (escalated from configurable fault).
pub const HFSR_FORCED: u32 = 1u32 << 30u32;
/// DEBUGEVT: debug event caused HardFault.
pub const HFSR_DEBUGEVT: u32 = 1u32 << 31u32;
/// Mask for MemManage bits (CFSR bits 0-7).
pub const MMFSR_MASK: u32 = 0x0000_00FFu32;
/// Mask for BusFault bits (CFSR bits 8-15).
pub const BFSR_MASK: u32 = 0x0000_FF00u32;
/// Mask for UsageFault bits (CFSR bits 16-31).
pub const UFSR_MASK: u32 = 0xFFFF_0000u32;
/// High-level fault category decoded from CFSR/HFSR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultCategory {
    /// MemManage fault (MPU violation, stack guard hit).
    MemManage,
    /// Bus fault (invalid memory access on bus).
    BusFault,
    /// Usage fault (illegal instruction, alignment, etc.).
    UsageFault,
    /// Hard fault (escalated or vector table fault).
    HardFault,
    /// No fault detected (all status bits clear).
    None,
}
/// ARM Cortex-M fault register snapshot.
///
/// Captures the four fault-related SCB registers at the time of a
/// fault exception. These are read by the fault handler and used to
/// classify and report the fault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CortexMFault {
    /// Configurable Fault Status Register (SCB->CFSR).
    /// Contains MMFSR (bits 0-7), BFSR (bits 8-15), UFSR (bits 16-31).
    pub cfsr: u32,
    /// HardFault Status Register (SCB->HFSR).
    pub hfsr: u32,
    /// MemManage Fault Address Register (SCB->MMFAR).
    /// Valid only when MMFSR.MMARVALID is set.
    pub mmfar: u32,
    /// BusFault Address Register (SCB->BFAR).
    /// Valid only when BFSR.BFARVALID is set.
    pub bfar: u32,
}
impl CortexMFault {
    /// Create a fault snapshot from raw register values.
    pub fn new(cfsr: u32, hfsr: u32, mmfar: u32, bfar: u32) -> CortexMFault {
        CortexMFault {
            cfsr,
            hfsr,
            mmfar,
            bfar,
        }
    }
    /// Extract the MemManage fault status (CFSR bits 0-7).
    pub fn mmfsr(&self) -> u32 {
        self.cfsr & 0x0000_00FFu32
    }
    /// Extract the BusFault status (CFSR bits 8-15).
    pub fn bfsr(&self) -> u32 {
        self.cfsr & 0x0000_FF00u32
    }
    /// Extract the UsageFault status (CFSR bits 16-31).
    pub fn ufsr(&self) -> u32 {
        self.cfsr & 0xFFFF_0000u32
    }
    /// FH2: Check if MMFAR holds a valid fault address.
    ///
    /// The MMFAR register is only valid when the MMARVALID bit (bit 7)
    /// is set in the MMFSR portion of CFSR.
    pub fn is_mmfar_valid(&self) -> bool {
        (self.cfsr & MMFSR_MMARVALID) != 0
    }
    /// FH2: Check if BFAR holds a valid fault address.
    ///
    /// The BFAR register is only valid when the BFARVALID bit (bit 15)
    /// is set in the BFSR portion of CFSR.
    pub fn is_bfar_valid(&self) -> bool {
        (self.cfsr & BFSR_BFARVALID) != 0
    }
    /// Get MMFAR value, returning None if not valid.
    ///
    /// FH2: Only returns Some when MMARVALID is set.
    pub fn mmfar_checked(&self) -> Option<u32> {
        if (self.cfsr & MMFSR_MMARVALID) != 0 { Some(self.mmfar) } else { None }
    }
    /// Get BFAR value, returning None if not valid.
    ///
    /// FH2: Only returns Some when BFARVALID is set.
    pub fn bfar_checked(&self) -> Option<u32> {
        if (self.cfsr & BFSR_BFARVALID) != 0 { Some(self.bfar) } else { None }
    }
    /// FH3: Check if HardFault was escalated from a configurable fault.
    ///
    /// The FORCED bit (HFSR bit 30) indicates that a configurable fault
    /// (MemManage, BusFault, or UsageFault) was escalated to HardFault
    /// because the configurable fault was disabled or another fault
    /// occurred during fault processing.
    pub fn is_escalated(&self) -> bool {
        (self.hfsr & HFSR_FORCED) != 0
    }
    /// Check if a vector table read caused the HardFault.
    pub fn is_vecttbl_fault(&self) -> bool {
        (self.hfsr & HFSR_VECTTBL) != 0
    }
    /// FH1: Classify the primary fault category.
    ///
    /// Determines the highest-priority fault category from the
    /// CFSR and HFSR registers. Priority order (from ARM architecture):
    ///   1. HardFault (checked via HFSR)
    ///   2. MemManage (CFSR bits 0-7)
    ///   3. BusFault  (CFSR bits 8-15)
    ///   4. UsageFault (CFSR bits 16-31)
    ///
    /// Returns None if no fault bits are set.
    pub fn classify(&self) -> FaultCategory {
        if (self.hfsr & HFSR_FORCED) != 0 || (self.hfsr & HFSR_VECTTBL) != 0 {
            FaultCategory::HardFault
        } else if (self.cfsr & 0x0000_00FFu32) != 0 {
            FaultCategory::MemManage
        } else if (self.cfsr & 0x0000_FF00u32) != 0 {
            FaultCategory::BusFault
        } else if (self.cfsr & 0xFFFF_0000u32) != 0 {
            FaultCategory::UsageFault
        } else {
            FaultCategory::None
        }
    }
    /// Check for instruction access violation (MPU).
    pub fn has_iaccviol(&self) -> bool {
        (self.cfsr & MMFSR_IACCVIOL) != 0
    }
    /// Check for data access violation (MPU).
    pub fn has_daccviol(&self) -> bool {
        (self.cfsr & MMFSR_DACCVIOL) != 0
    }
    /// Check for instruction bus error.
    pub fn has_ibuserr(&self) -> bool {
        (self.cfsr & BFSR_IBUSERR) != 0
    }
    /// Check for precise data bus error.
    pub fn has_preciserr(&self) -> bool {
        (self.cfsr & BFSR_PRECISERR) != 0
    }
    /// Check for imprecise data bus error.
    pub fn has_impreciserr(&self) -> bool {
        (self.cfsr & BFSR_IMPRECISERR) != 0
    }
    /// Check for undefined instruction.
    pub fn has_undefinstr(&self) -> bool {
        (self.cfsr & UFSR_UNDEFINSTR) != 0
    }
    /// Check for invalid state (e.g., ARM mode on Thumb-only).
    pub fn has_invstate(&self) -> bool {
        (self.cfsr & UFSR_INVSTATE) != 0
    }
    /// Check for divide by zero.
    pub fn has_divbyzero(&self) -> bool {
        (self.cfsr & UFSR_DIVBYZERO) != 0
    }
    /// Check for unaligned memory access.
    pub fn has_unaligned(&self) -> bool {
        (self.cfsr & UFSR_UNALIGNED) != 0
    }
    /// Check for stack overflow (ARMv8-M).
    pub fn has_stkof(&self) -> bool {
        (self.cfsr & UFSR_STKOF) != 0
    }
}
