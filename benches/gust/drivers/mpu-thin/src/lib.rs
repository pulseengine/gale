//! SPIKE — gust:mpu thin-seam I-ISO region-programming core.
//! Bodies lifted VERBATIM from plain/src/mpu_switch.rs (Verus+Kani verified).
//! The single native atom `mpu_write` arrives as a WIT-typed component import.
#![no_std]
#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }

use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "mpu-thin", path: "wit", generate_all });

mod mpu {
pub const MIN_REGION_SIZE: u32 = 32;
pub fn is_power_of_two(n: u32) -> bool {
    let result = n > 0 && (n & (n - 1)) == 0;
    result
}

pub fn validate_region(base: u32, size: u32) -> bool {
    if size == 0 {
        return false;
    }
    let power_of_two = (size & (size - 1)) == 0;
    let min_size = size >= MIN_REGION_SIZE;
    let aligned = (base & (size - 1)) == 0;
    let no_overflow = base.checked_add(size).is_some();
    power_of_two && min_size && aligned && no_overflow
}
}

mod iso {
    use crate::mpu::MIN_REGION_SIZE;
    // the seam: what was `unsafe extern "C" fn mpu_write` is now a typed import
    pub fn mpu_write(rnr: u32, rbar: u32, rasr: u32) {
        crate::gust::mpu::regs::mpu_write(rnr, rbar, rasr);
    }
    pub const MAX_PARTITIONS: usize = 4;
    
    pub const MAX_REGIONS: usize = 8;
    
    pub const TABLE_SLOTS: usize = 32;
    
    pub const SEQ_LEN: usize = 10;
    
    pub const REQUIRED_DREGION: u32 = 8;
    
    pub const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;
    
    pub const MPU_CTRL_ENABLE: u32 = 1;
    
    pub const MPU_CTRL_DISABLE: u32 = 0;
    
    #[derive(Clone, Copy)]
    pub struct MpuWrite {
        /// Region number (RNR), or `MPU_CTRL_ID` for the trailing enable.
        pub rnr: u32,
        /// Region base address register value (RBAR).
        pub rbar: u32,
        /// Region attribute and size register value (RASR), or the MPU_CTRL
        /// value for the trailing enable write.
        pub rasr: u32,
    }
    
    #[derive(Clone, Copy)]
    pub struct ProgramSeq {
        pub w: [MpuWrite; SEQ_LEN],
    }
    
    pub struct RegionTable {
        pub base: [u32; TABLE_SLOTS],
        pub size: [u32; TABLE_SLOTS],
        pub enabled: [bool; TABLE_SLOTS],
        pub writable: [bool; TABLE_SLOTS],
    }
    
    pub fn size_field(size: u32) -> u32 {
        if size == 32 {
            4
        } else if size == 64 {
            5
        } else if size == 128 {
            6
        } else if size == 256 {
            7
        } else if size == 512 {
            8
        } else if size == 1024 {
            9
        } else if size == 2048 {
            10
        } else if size == 4096 {
            11
        } else if size == 8192 {
            12
        } else if size == 16384 {
            13
        } else if size == 32768 {
            14
        } else if size == 65536 {
            15
        } else if size == 131072 {
            16
        } else if size == 262144 {
            17
        } else if size == 524288 {
            18
        } else if size == 1048576 {
            19
        } else if size == 2097152 {
            20
        } else if size == 4194304 {
            21
        } else if size == 8388608 {
            22
        } else if size == 16777216 {
            23
        } else if size == 33554432 {
            24
        } else if size == 67108864 {
            25
        } else if size == 134217728 {
            26
        } else if size == 268435456 {
            27
        } else if size == 536870912 {
            28
        } else if size == 1073741824 {
            29
        } else if size == 2147483648 {
            30
        } else {
            0
        }
    }
    
    pub fn rasr_for(size: u32, writable: bool) -> u32 {
        let f = size_field(size);
        let ap: u32 = if writable { 3 } else { 6 };
        1u32 + 2u32 * f + 0x0100_0000u32 * ap
    }
    
    fn emit_write(w: &MpuWrite) {
        unsafe { mpu_write(w.rnr, w.rbar, w.rasr) };
    }
    
    pub fn apply_program(seq: &ProgramSeq) {
        let mut i: usize = 0;
        while i < SEQ_LEN {
            emit_write(&seq.w[i]);
            i += 1;
        }
    }
    
    impl RegionTable {
        /// An all-disabled table (the deny-everything baseline). Real
        /// deployments construct their static per-partition configuration as
        /// a constant and discharge `table_inv` at build time.
        pub fn new() -> RegionTable {
            RegionTable {
                base: [0u32; TABLE_SLOTS],
                size: [0u32; TABLE_SLOTS],
                enabled: [false; TABLE_SLOTS],
                writable: [false; TABLE_SLOTS],
            }
        }
        /// Compute the exact register-write sequence for switching the MPU to
        /// partition `part` — the verified heart of I-ISO. See the module
        /// header for P1–P4.
        pub fn program_partition(&self, part: u32) -> ProgramSeq {
            let mut out = ProgramSeq {
                w: [MpuWrite {
                    rnr: 0,
                    rbar: 0,
                    rasr: 0,
                }; SEQ_LEN],
            };
            out.w[0] = MpuWrite {
                rnr: MPU_CTRL_ID,
                rbar: 0,
                rasr: MPU_CTRL_DISABLE,
            };
            let mut r: usize = 0;
            while r < MAX_REGIONS {
                let i = (part as usize) * MAX_REGIONS + r;
                if self.enabled[i] {
                    let rasr = rasr_for(self.size[i], self.writable[i]);
                    out.w[r + 1] = MpuWrite {
                        rnr: r as u32,
                        rbar: self.base[i],
                        rasr,
                    };
                } else {
                    out.w[r + 1] = MpuWrite {
                        rnr: r as u32,
                        rbar: 0,
                        rasr: 0,
                    };
                }
                r += 1;
            }
            out.w[MAX_REGIONS + 1] = MpuWrite {
                rnr: MPU_CTRL_ID,
                rbar: 0,
                rasr: MPU_CTRL_ENABLE,
            };
            out
        }
        /// Program the MPU for partition `part`: compute the verified
        /// sequence, then emit it through the trusted seam — the one call a
        /// partition switch makes. The computation and the emission loop are
        /// verified; only the single register store is trusted.
        pub fn switch_to_partition(&self, part: u32) {
            let seq = self.program_partition(part);
            apply_program(&seq);
        }
        /// Verified table builder: add region request (`base`, `size`,
        /// `writable`) to partition `part`'s FIRST free slot.
        ///
        /// Rejects (returns `false`, table proven unchanged — B2) when:
        ///   * `part` is out of range (defensive: keeps the stripped exec
        ///     builder total — no panic on any input),
        ///   * `size` is not a power of two >= `MIN_REGION_SIZE` (32) — the
        ///     same characterisation `crate::mpu::validate_region` enforces,
        ///     reusing the verified `crate::mpu::is_power_of_two`,
        ///   * `base` is not `size`-aligned,
        ///   * `base + size` wraps the address space (the U-6 bound),
        ///   * the request OVERLAPS an enabled region already granted to
        ///     `part` (THE isolation-bearing check), or
        ///   * all of `part`'s region slots are occupied.
        ///
        /// On acceptance the resulting table is proven to still satisfy
        /// `table_inv` (B1) — in particular the new region is well-formed and
        /// disjoint from every other enabled region of `part` — so a caller
        /// building exclusively through `new()` + `try_add_region` cannot
        /// construct an isolation-violating table, and `program_partition`'s
        /// precondition holds on the result by construction.
        pub fn try_add_region(
            &mut self,
            part: u32,
            base: u32,
            size: u32,
            writable: bool,
        ) -> bool {
            if part >= MAX_PARTITIONS as u32 {
                return false;
            }
            if !crate::mpu::is_power_of_two(size) {
                return false;
            }
            if size < MIN_REGION_SIZE {
                return false;
            }
            if base % size != 0 {
                return false;
            }
            if base.checked_add(size).is_none() {
                return false;
            }
            let mut r: usize = 0;
            while r < MAX_REGIONS {
                let i = (part as usize) * MAX_REGIONS + r;
                if self.enabled[i] {
                    if !(base + size <= self.base[i] || self.base[i] + self.size[i] <= base)
                    {
                        return false;
                    }
                }
                r += 1;
            }
            let mut f: usize = 0;
            while f < MAX_REGIONS {
                let i = (part as usize) * MAX_REGIONS + f;
                if !self.enabled[i] {
                    self.base[i] = base;
                    self.size[i] = size;
                    self.writable[i] = writable;
                    self.enabled[i] = true;
                    return true;
                }
                f += 1;
            }
            false
        }
        /// Exec mirror of `covers`, proven equivalent: does some enabled
        /// region of partition `part` contain `addr`? Post-strip this is the
        /// plain runtime query for what the builder granted (and the
        /// Kani-checkable form of `covers`).
        pub fn covers_addr(&self, part: u32, addr: u32) -> bool {
            let mut r: usize = 0;
            while r < MAX_REGIONS {
                let i = (part as usize) * MAX_REGIONS + r;
                if self.enabled[i] && self.base[i] <= addr
                    && addr - self.base[i] < self.size[i]
                {
                    return true;
                }
                r += 1;
            }
            false
        }
    }}

// One instance owns the table — the exec-provider construction.
static mut TABLE: Option<iso::RegionTable> = None;
fn table() -> &'static mut iso::RegionTable {
    unsafe {
        if TABLE.is_none() { TABLE = Some(iso::RegionTable::new()); }
        TABLE.as_mut().unwrap()
    }
}

struct P;
impl exports::gust::mpu::iso::Guest for P {
    fn size_field(size: u32) -> u32 { iso::size_field(size) }
    fn rasr_for(size: u32, writable: bool) -> u32 { iso::rasr_for(size, writable) }
    fn try_add_region(part: u32, base: u32, size: u32, writable: bool) -> bool {
        table().try_add_region(part, base, size, writable)
    }
    fn covers_addr(part: u32, addr: u32) -> bool { table().covers_addr(part, addr) }
    fn switch_to_partition(part: u32) { table().switch_to_partition(part) }
}
export!(P);
