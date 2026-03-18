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

#[doc = " IACCVIOL: instruction access violation."]
pub const MMFSR_IACCVIOL: u32 = 1u32 << 0u32;
#[doc = " DACCVIOL: data access violation."]
pub const MMFSR_DACCVIOL: u32 = 1u32 << 1u32;
#[doc = " MUNSTKERR: MemManage fault on unstacking (exception return)."]
pub const MMFSR_MUNSTKERR: u32 = 1u32 << 3u32;
#[doc = " MSTKERR: MemManage fault on stacking (exception entry)."]
pub const MMFSR_MSTKERR: u32 = 1u32 << 4u32;
#[doc = " MLSPERR: MemManage fault during lazy FP state preservation."]
pub const MMFSR_MLSPERR: u32 = 1u32 << 5u32;
#[doc = " MMARVALID: MMFAR holds a valid fault address."]
pub const MMFSR_MMARVALID: u32 = 1u32 << 7u32;
#[doc = " IBUSERR: instruction bus error."]
pub const BFSR_IBUSERR: u32 = 1u32 << 8u32;
#[doc = " PRECISERR: precise data bus error."]
pub const BFSR_PRECISERR: u32 = 1u32 << 9u32;
#[doc = " IMPRECISERR: imprecise data bus error."]
pub const BFSR_IMPRECISERR: u32 = 1u32 << 10u32;
#[doc = " UNSTKERR: bus fault on unstacking."]
pub const BFSR_UNSTKERR: u32 = 1u32 << 11u32;
#[doc = " STKERR: bus fault on stacking."]
pub const BFSR_STKERR: u32 = 1u32 << 12u32;
#[doc = " LSPERR: bus fault during lazy FP state preservation."]
pub const BFSR_LSPERR: u32 = 1u32 << 13u32;
#[doc = " BFARVALID: BFAR holds a valid fault address."]
pub const BFSR_BFARVALID: u32 = 1u32 << 15u32;
#[doc = " UNDEFINSTR: undefined instruction."]
pub const UFSR_UNDEFINSTR: u32 = 1u32 << 16u32;
#[doc = " INVSTATE: invalid state (e.g., ARM mode on Thumb-only core)."]
pub const UFSR_INVSTATE: u32 = 1u32 << 17u32;
#[doc = " INVPC: invalid PC load via EXC_RETURN."]
pub const UFSR_INVPC: u32 = 1u32 << 18u32;
#[doc = " NOCP: no coprocessor (attempted access to unavailable CP)."]
pub const UFSR_NOCP: u32 = 1u32 << 19u32;
#[doc = " STKOF: stack overflow (ARMv8-M only, bit 20)."]
pub const UFSR_STKOF: u32 = 1u32 << 20u32;
#[doc = " UNALIGNED: unaligned memory access."]
pub const UFSR_UNALIGNED: u32 = 1u32 << 24u32;
#[doc = " DIVBYZERO: integer divide by zero."]
pub const UFSR_DIVBYZERO: u32 = 1u32 << 25u32;
#[doc = " VECTTBL: bus fault on vector table read."]
pub const HFSR_VECTTBL: u32 = 1u32 << 1u32;
#[doc = " FORCED: forced HardFault (escalated from configurable fault)."]
pub const HFSR_FORCED: u32 = 1u32 << 30u32;
#[doc = " DEBUGEVT: debug event caused HardFault."]
pub const HFSR_DEBUGEVT: u32 = 1u32 << 31u32;
#[doc = " Mask for MemManage bits (CFSR bits 0-7)."]
pub const MMFSR_MASK: u32 = 0x0000_00FFu32;
#[doc = " Mask for BusFault bits (CFSR bits 8-15)."]
pub const BFSR_MASK: u32 = 0x0000_FF00u32;
#[doc = " Mask for UsageFault bits (CFSR bits 16-31)."]
pub const UFSR_MASK: u32 = 0xFFFF_0000u32;
#[doc = " High-level fault category decoded from CFSR/HFSR."]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultCategory {
    #[doc = " MemManage fault (MPU violation, stack guard hit)."]
    MemManage,
    #[doc = " Bus fault (invalid memory access on bus)."]
    BusFault,
    #[doc = " Usage fault (illegal instruction, alignment, etc.)."]
    UsageFault,
    #[doc = " Hard fault (escalated or vector table fault)."]
    HardFault,
    #[doc = " No fault detected (all status bits clear)."]
    None,
}
#[doc = " ARM Cortex-M fault register snapshot."]
#[doc = ""]
#[doc = " Captures the four fault-related SCB registers at the time of a"]
#[doc = " fault exception. These are read by the fault handler and used to"]
#[doc = " classify and report the fault."]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CortexMFault {
    #[doc = " Configurable Fault Status Register (SCB->CFSR)."]
    #[doc = " Contains MMFSR (bits 0-7), BFSR (bits 8-15), UFSR (bits 16-31)."]
    pub cfsr: u32,
    #[doc = " HardFault Status Register (SCB->HFSR)."]
    pub hfsr: u32,
    #[doc = " MemManage Fault Address Register (SCB->MMFAR)."]
    #[doc = " Valid only when MMFSR.MMARVALID is set."]
    pub mmfar: u32,
    #[doc = " BusFault Address Register (SCB->BFAR)."]
    #[doc = " Valid only when BFSR.BFARVALID is set."]
    pub bfar: u32,
}
impl CortexMFault {
    #[doc = " Create a fault snapshot from raw register values."]
    pub fn new(cfsr: u32, hfsr: u32, mmfar: u32, bfar: u32) -> CortexMFault {
        CortexMFault {
            cfsr,
            hfsr,
            mmfar,
            bfar,
        }
    }
    #[doc = " Extract the MemManage fault status (CFSR bits 0-7)."]
    pub fn mmfsr(&self) -> u32 {
        self.cfsr & 0x0000_00FFu32
    }
    #[doc = " Extract the BusFault status (CFSR bits 8-15)."]
    pub fn bfsr(&self) -> u32 {
        self.cfsr & 0x0000_FF00u32
    }
    #[doc = " Extract the UsageFault status (CFSR bits 16-31)."]
    pub fn ufsr(&self) -> u32 {
        self.cfsr & 0xFFFF_0000u32
    }
    #[doc = " FH2: Check if MMFAR holds a valid fault address."]
    #[doc = ""]
    #[doc = " The MMFAR register is only valid when the MMARVALID bit (bit 7)"]
    #[doc = " is set in the MMFSR portion of CFSR."]
    pub fn is_mmfar_valid(&self) -> bool {
        (self.cfsr & MMFSR_MMARVALID) != 0
    }
    #[doc = " FH2: Check if BFAR holds a valid fault address."]
    #[doc = ""]
    #[doc = " The BFAR register is only valid when the BFARVALID bit (bit 15)"]
    #[doc = " is set in the BFSR portion of CFSR."]
    pub fn is_bfar_valid(&self) -> bool {
        (self.cfsr & BFSR_BFARVALID) != 0
    }
    #[doc = " Get MMFAR value, returning None if not valid."]
    #[doc = ""]
    #[doc = " FH2: Only returns Some when MMARVALID is set."]
    pub fn mmfar_checked(&self) -> Option<u32> {
        if (self.cfsr & MMFSR_MMARVALID) != 0 {
            Some(self.mmfar)
        } else {
            None
        }
    }
    #[doc = " Get BFAR value, returning None if not valid."]
    #[doc = ""]
    #[doc = " FH2: Only returns Some when BFARVALID is set."]
    pub fn bfar_checked(&self) -> Option<u32> {
        if (self.cfsr & BFSR_BFARVALID) != 0 {
            Some(self.bfar)
        } else {
            None
        }
    }
    #[doc = " FH3: Check if HardFault was escalated from a configurable fault."]
    #[doc = ""]
    #[doc = " The FORCED bit (HFSR bit 30) indicates that a configurable fault"]
    #[doc = " (MemManage, BusFault, or UsageFault) was escalated to HardFault"]
    #[doc = " because the configurable fault was disabled or another fault"]
    #[doc = " occurred during fault processing."]
    pub fn is_escalated(&self) -> bool {
        (self.hfsr & HFSR_FORCED) != 0
    }
    #[doc = " Check if a vector table read caused the HardFault."]
    pub fn is_vecttbl_fault(&self) -> bool {
        (self.hfsr & HFSR_VECTTBL) != 0
    }
    #[doc = " FH1: Classify the primary fault category."]
    #[doc = ""]
    #[doc = " Determines the highest-priority fault category from the"]
    #[doc = " CFSR and HFSR registers. Priority order (from ARM architecture):"]
    #[doc = "   1. HardFault (checked via HFSR)"]
    #[doc = "   2. MemManage (CFSR bits 0-7)"]
    #[doc = "   3. BusFault  (CFSR bits 8-15)"]
    #[doc = "   4. UsageFault (CFSR bits 16-31)"]
    #[doc = ""]
    #[doc = " Returns None if no fault bits are set."]
    pub fn classify(&self) -> FaultCategory {
        let hfsr = self.hfsr;
        let cfsr = self.cfsr;
        if (hfsr & HFSR_FORCED) != 0 || (hfsr & HFSR_VECTTBL) != 0 {
            FaultCategory::HardFault
        } else if (cfsr & 0x0000_00FFu32) != 0 {
            FaultCategory::MemManage
        } else if (cfsr & 0x0000_FF00u32) != 0 {
            FaultCategory::BusFault
        } else if (cfsr & 0xFFFF_0000u32) != 0 {
            FaultCategory::UsageFault
        } else {
            FaultCategory::None
        }
    }
    #[doc = " Check for instruction access violation (MPU)."]
    pub fn has_iaccviol(&self) -> bool {
        (self.cfsr & MMFSR_IACCVIOL) != 0
    }
    #[doc = " Check for data access violation (MPU)."]
    pub fn has_daccviol(&self) -> bool {
        (self.cfsr & MMFSR_DACCVIOL) != 0
    }
    #[doc = " Check for instruction bus error."]
    pub fn has_ibuserr(&self) -> bool {
        (self.cfsr & BFSR_IBUSERR) != 0
    }
    #[doc = " Check for precise data bus error."]
    pub fn has_preciserr(&self) -> bool {
        (self.cfsr & BFSR_PRECISERR) != 0
    }
    #[doc = " Check for imprecise data bus error."]
    pub fn has_impreciserr(&self) -> bool {
        (self.cfsr & BFSR_IMPRECISERR) != 0
    }
    #[doc = " Check for undefined instruction."]
    pub fn has_undefinstr(&self) -> bool {
        (self.cfsr & UFSR_UNDEFINSTR) != 0
    }
    #[doc = " Check for invalid state (e.g., ARM mode on Thumb-only)."]
    pub fn has_invstate(&self) -> bool {
        (self.cfsr & UFSR_INVSTATE) != 0
    }
    #[doc = " Check for divide by zero."]
    pub fn has_divbyzero(&self) -> bool {
        (self.cfsr & UFSR_DIVBYZERO) != 0
    }
    #[doc = " Check for unaligned memory access."]
    pub fn has_unaligned(&self) -> bool {
        (self.cfsr & UFSR_UNALIGNED) != 0
    }
    #[doc = " Check for stack overflow (ARMv8-M)."]
    pub fn has_stkof(&self) -> bool {
        (self.cfsr & UFSR_STKOF) != 0
    }
}
