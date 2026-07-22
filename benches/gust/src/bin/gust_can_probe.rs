//! gust-can-probe — the LOCAL qemu-semihosting probe of the DISSOLVED can-thin .o,
//! run BEFORE the Renode content-gate to catch table/linmem no-op bugs fast.
//!
//! Points the dissolved bxCAN driver at a plain `[u32; N]` RAM window (real mapped
//! SRAM on lm3s6965evb) and checks the exact register effects — the INRQ init
//! request, the BTR bit-timing write, the TX-mailbox load + TXRQ, the RX-FIFO
//! RFOM0 release — plus the mode FSM via semihosting, so a dissolved primitive that
//! silently no-ops (e.g. a `.rodata` linmem lookup that reads 0 under
//! `--relocatable`) fails HERE, on `cargo run`, not three CI minutes later in
//! Renode. Also DEMONSTRATES the driver's distinctive safety property,
//! config-only-in-init: a `can_configure` from Sleep or from Normal is rejected
//! (CAN_FAULT) and leaves BTR untouched — the bit-timing register is written ONLY
//! inside the INRQ/INAK init window, so a wrong bit rate can never reach the bus.
#![no_std]
#![no_main]
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio seam the dissolved driver imports — here it reads/writes the RAM window.
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

extern "C" {
    fn can_enter_init(base: u32, state: u32) -> u32;
    fn can_configure(base: u32, state: u32, brp: u32, ts1: u32, ts2: u32, sjw: u32) -> u32;
    fn can_leave_init(base: u32, state: u32) -> u32;
    fn can_tx_request(base: u32, state: u32, id: u32, dlc: u32, dlo: u32, dhi: u32) -> u32;
    fn can_rx_release(base: u32, state: u32) -> u32;
    fn can_mode(state: u32) -> u32;
}

// Fake bxCAN register window in RAM: word i = byte offset i*4. Highest offset is
// TDH0R at 0x18C (word 99) → a 120-word window covers the whole map with slack.
static mut REG: [u32; 120] = [0; 120];

const MCR: u32 = 0x000;
const MSR: u32 = 0x004;
const TSR: u32 = 0x008;
const RF0R: u32 = 0x00C;
const BTR: u32 = 0x01C;
const TI0R: u32 = 0x180;
const TDT0R: u32 = 0x184;
const TDL0R: u32 = 0x188;
const TDH0R: u32 = 0x18C;

const MCR_INRQ: u32 = 1 << 0;
const MSR_INAK: u32 = 1 << 0;
const TSR_TME0: u32 = 1 << 26;
const RF0R_RFOM0: u32 = 1 << 5;
const CAN_FAULT: u32 = 0xFFFF_FFFF;

const PH_INIT: u32 = 1;
const PH_NORMAL: u32 = 2;

// Bit-timing inputs and the exact BTR value the driver must compute (pure bit math,
// masked to each field: BRP[9:0] | TS1<<16 | TS2<<20 | SJW<<24).
const BRP: u32 = 0x0C;
const TS1: u32 = 0xD;
const TS2: u32 = 0x5;
const SJW: u32 = 0x2;
const BTR_EXPECT: u32 = 0x0C | (0xD << 16) | (0x5 << 20) | (0x2 << 24); // 0x025D_000C

// TX frame inputs and the exact TI0R the driver must set (STID<<21 | TXRQ).
const TX_ID: u32 = 0x123;
const TX_DLC: u32 = 8;
const TX_DLO: u32 = 0xDEAD_BEEF;
const TX_DHI: u32 = 0xCAFE_F00D;
const TI0R_EXPECT: u32 = (0x123 << 21) | 1; // 0x2460_0001

#[inline]
fn rd(base: u32, off: u32) -> u32 {
    unsafe { read_volatile((base + off) as *const u32) }
}
#[inline]
fn wr(base: u32, off: u32, v: u32) {
    unsafe { write_volatile((base + off) as *mut u32, v) }
}

#[entry]
fn main() -> ! {
    let base = addr_of_mut!(REG) as u32;
    let mut ok = true;

    // 0) config-only-in-init (write-protection): a config attempted straight from
    //    Sleep (state=0) MUST be rejected — no BTR write. A dissolve that no-ops the
    //    phase gate would let this through and corrupt the bus bit rate.
    let cfg_sleep = unsafe { can_configure(base, 0, BRP, TS1, TS2, SJW) };
    let btr0 = rd(base, BTR);
    if cfg_sleep != CAN_FAULT || btr0 != 0 {
        hprintln!("can-protect FAIL: cfg={:#x} BTR={:#x}", cfg_sleep, btr0);
        ok = false;
    } else {
        hprintln!("can-protect ok: config-from-Sleep faulted, BTR untouched");
    }

    // 1) enter_init: driver sets MCR.INRQ and spins until MSR.INAK confirms. Pre-seed
    //    INAK so the handshake completes; assert MCR was actually written and the FSM
    //    is now Init.
    wr(base, MSR, MSR_INAK);
    let s1 = unsafe { can_enter_init(base, 0) };
    let mcr1 = rd(base, MCR);
    if s1 == CAN_FAULT || unsafe { can_mode(s1) } != PH_INIT || mcr1 != MCR_INRQ {
        hprintln!("can-init FAIL: s1={:#x} mode={} MCR={:#x}", s1, unsafe { can_mode(s1) }, mcr1);
        ok = false;
    } else {
        hprintln!("can-init ok: MCR.INRQ set, phase=Init");
    }

    // 2) configure in Init: the driver computes and writes BTR = the four bit-timing
    //    fields, each masked into its own slot. A no-op leaves it 0.
    let s2 = unsafe { can_configure(base, s1, BRP, TS1, TS2, SJW) };
    let btr2 = rd(base, BTR);
    if s2 == CAN_FAULT || unsafe { can_mode(s2) } != PH_INIT || btr2 != BTR_EXPECT {
        hprintln!("can-config FAIL: s2={:#x} BTR={:#x} want {:#x}", s2, btr2, BTR_EXPECT);
        ok = false;
    } else {
        hprintln!("can-config ok: BTR={:#x}", btr2);
    }

    // 3) leave_init: clear MCR.INRQ and spin until MSR.INAK clears — Init -> Normal.
    //    Clear INAK first so the exit handshake completes.
    wr(base, MSR, 0);
    let s3 = unsafe { can_leave_init(base, s2) };
    let mcr3 = rd(base, MCR);
    if s3 == CAN_FAULT || unsafe { can_mode(s3) } != PH_NORMAL || mcr3 != 0 {
        hprintln!("can-normal FAIL: s3={:#x} mode={} MCR={:#x}", s3, unsafe { can_mode(s3) }, mcr3);
        ok = false;
    } else {
        hprintln!("can-normal ok: MCR.INRQ cleared, phase=Normal");
    }

    // 4) config-only-in-init, the DISTINCTIVE property: a config now that the bus is
    //    live (Normal) is rejected AND leaves BTR exactly as configured in Init — it
    //    is NOT overwritten with the (zero) stale bit rate. This is the non-vacuous
    //    heart of the gate: a dissolve that dropped the phase gate would clobber BTR.
    let cfg_normal = unsafe { can_configure(base, s3, 0, 0, 0, 0) };
    let btr4 = rd(base, BTR);
    if cfg_normal != CAN_FAULT || btr4 != BTR_EXPECT {
        hprintln!("can-config-gated FAIL: cfg={:#x} BTR={:#x} want unchanged {:#x}", cfg_normal, btr4, BTR_EXPECT);
        ok = false;
    } else {
        hprintln!("can-config-gated ok: config-in-Normal faulted, BTR stays {:#x}", btr4);
    }

    // 5) TX gating: rejected off-Normal AND when the mailbox is busy (TME0 clear); on
    //    an empty mailbox it loads ID/DLC/data exactly and sets TXRQ.
    let tx_sleep = unsafe { can_tx_request(base, 0, TX_ID, TX_DLC, TX_DLO, TX_DHI) };
    wr(base, TSR, 0); // mailbox busy
    let tx_busy = unsafe { can_tx_request(base, s3, TX_ID, TX_DLC, TX_DLO, TX_DHI) };
    let ti_busy = rd(base, TI0R);
    wr(base, TSR, TSR_TME0); // mailbox empty
    let s_tx = unsafe { can_tx_request(base, s3, TX_ID, TX_DLC, TX_DLO, TX_DHI) };
    let ti = rd(base, TI0R);
    let tdt = rd(base, TDT0R);
    let tdl = rd(base, TDL0R);
    let tdh = rd(base, TDH0R);
    if tx_sleep != CAN_FAULT
        || tx_busy != CAN_FAULT
        || ti_busy != 0
        || s_tx == CAN_FAULT
        || unsafe { can_mode(s_tx) } != PH_NORMAL
        || ti != TI0R_EXPECT
        || tdt != (TX_DLC & 0xF)
        || tdl != TX_DLO
        || tdh != TX_DHI
    {
        hprintln!(
            "can-tx FAIL: sleep={:#x} busy={:#x} ti_busy={:#x} s_tx={:#x} TI0R={:#x} TDT={:#x} TDL={:#x} TDH={:#x}",
            tx_sleep, tx_busy, ti_busy, s_tx, ti, tdt, tdl, tdh
        );
        ok = false;
    } else {
        hprintln!("can-tx ok: off-Normal + busy faulted, empty-mailbox load TI0R={:#x} DLC={:#x}", ti, tdt);
    }

    // 6) RX gating: rejected off-Normal AND when nothing is pending (FMP0==0); with a
    //    pending message it releases FIFO 0 by setting exactly RFOM0.
    let rx_sleep = unsafe { can_rx_release(base, 0) };
    wr(base, RF0R, 0); // nothing pending
    let rx_empty = unsafe { can_rx_release(base, s3) };
    let rf_empty = rd(base, RF0R);
    wr(base, RF0R, 1); // FMP0 = 1 pending
    let s_rx = unsafe { can_rx_release(base, s3) };
    let rf = rd(base, RF0R);
    if rx_sleep != CAN_FAULT
        || rx_empty != CAN_FAULT
        || rf_empty != 0
        || s_rx == CAN_FAULT
        || unsafe { can_mode(s_rx) } != PH_NORMAL
        || rf != RF0R_RFOM0
    {
        hprintln!(
            "can-rx FAIL: sleep={:#x} empty={:#x} rf_empty={:#x} s_rx={:#x} RF0R={:#x}",
            rx_sleep, rx_empty, rf_empty, s_rx, rf
        );
        ok = false;
    } else {
        hprintln!("can-rx ok: off-Normal + empty faulted, pending release RF0R={:#x}", rf);
    }

    if ok {
        hprintln!("can-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}
