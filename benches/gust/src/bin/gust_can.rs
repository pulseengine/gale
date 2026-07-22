//! gust-can — the thin-seam CAN (bxCAN) driver driven bare-metal on gust, with a
//! self-checking Renode content-gate (gust-OS driver breadth close-out).
//!
//! Links ONLY the dissolved can-thin driver (mmio bridge, 0 new TCB atoms); the raw
//! USART1 poke is trusted plumbing to report results. Asserts, on a RAM-mapped bxCAN
//! window (real CAN1 address, so no dependence on Renode's CAN peripheral model):
//! (a) config-only-in-init write-protection — a `can_configure` before Init is
//! rejected and touches no register, (b) the INRQ init request + the exact BTR
//! bit-timing write inside Init, (c) the INRQ-clear Init->Normal handshake, (d) the
//! DISTINCTIVE property: a config once Normal is rejected and leaves BTR unchanged —
//! the bit rate can never be clobbered live, (e) TX-mailbox load gated on TME0, and
//! (f) RX-FIFO RFOM0 release gated on FMP0. Emits can-*-ok on USART1 iff correct.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
use panic_halt as _;

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

const CAN1: u32 = 0x4000_6400; // RAM-mapped bxCAN window in the gate .repl
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

const BRP: u32 = 0x0C;
const TS1: u32 = 0xD;
const TS2: u32 = 0x5;
const SJW: u32 = 0x2;
const BTR_EXPECT: u32 = 0x0C | (0xD << 16) | (0x5 << 20) | (0x2 << 24); // 0x025D_000C

const TX_ID: u32 = 0x123;
const TX_DLC: u32 = 8;
const TX_DLO: u32 = 0xDEAD_BEEF;
const TX_DHI: u32 = 0xCAFE_F00D;
const TI0R_EXPECT: u32 = (0x123 << 21) | 1; // 0x2460_0001

const USART1: u32 = 0x4001_3800;
const USART_SR: u32 = 0x00;
const USART_DR: u32 = 0x04;
const USART_BRR: u32 = 0x08;
const USART_CR1: u32 = 0x0C;
const TXE: u32 = 1 << 7;

fn tx(s: &[u8]) {
    for &b in s {
        unsafe {
            while read_volatile((USART1 + USART_SR) as *const u32) & TXE == 0 {}
            write_volatile((USART1 + USART_DR) as *mut u32, (b as u32) & 0xFF);
        }
    }
}

#[inline]
fn rd(off: u32) -> u32 {
    unsafe { read_volatile((CAN1 + off) as *const u32) }
}
#[inline]
fn wr(off: u32, v: u32) {
    unsafe { write_volatile((CAN1 + off) as *mut u32, v) }
}

#[entry]
fn main() -> ! {
    unsafe {
        // enable GPIOA(PA9 TX), AFIO, USART1; PA9 → AF push-pull; USART1 8MHz/115200.
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));
    }

    tx(b"can-gate begin\n");

    // 0) config-only-in-init write-protection: config from Sleep (state=0) rejected,
    //    no BTR write.
    let cfg_sleep = unsafe { can_configure(CAN1, 0, BRP, TS1, TS2, SJW) };
    let btr0 = rd(BTR);
    tx(if cfg_sleep == CAN_FAULT && btr0 == 0 {
        b"can-protect-ok\n"
    } else {
        b"can-protect-bad\n"
    });

    // 1) enter_init: driver sets MCR.INRQ, spins on MSR.INAK (pre-seeded), FSM->Init.
    wr(MSR, MSR_INAK);
    let s1 = unsafe { can_enter_init(CAN1, 0) };
    let mcr1 = rd(MCR);
    tx(if s1 != CAN_FAULT && unsafe { can_mode(s1) } == PH_INIT && mcr1 == MCR_INRQ {
        b"can-init-ok\n"
    } else {
        b"can-init-bad\n"
    });

    // 2) configure in Init: BTR carries the four masked bit-timing fields exactly.
    let s2 = unsafe { can_configure(CAN1, s1, BRP, TS1, TS2, SJW) };
    let btr2 = rd(BTR);
    tx(if s2 != CAN_FAULT && unsafe { can_mode(s2) } == PH_INIT && btr2 == BTR_EXPECT {
        b"can-config-ok\n"
    } else {
        b"can-config-bad\n"
    });

    // 3) leave_init: clear MCR.INRQ, spin until INAK clears (pre-cleared), Init->Normal.
    wr(MSR, 0);
    let s3 = unsafe { can_leave_init(CAN1, s2) };
    let mcr3 = rd(MCR);
    let normal_ok = s3 != CAN_FAULT && unsafe { can_mode(s3) } == PH_NORMAL && mcr3 == 0;

    // 4) config-only-in-init DISTINCTIVE property: config once Normal is rejected AND
    //    leaves BTR exactly as configured — never clobbered with a stale (zero) rate.
    let cfg_normal = unsafe { can_configure(CAN1, s3, 0, 0, 0, 0) };
    let btr4 = rd(BTR);
    tx(if normal_ok && cfg_normal == CAN_FAULT && btr4 == BTR_EXPECT {
        b"can-normal-ok\n"
    } else {
        b"can-normal-bad\n"
    });

    // 5) TX gating: off-Normal + busy-mailbox rejected; empty mailbox loads ID/DLC/data.
    let tx_sleep = unsafe { can_tx_request(CAN1, 0, TX_ID, TX_DLC, TX_DLO, TX_DHI) };
    wr(TSR, 0);
    let tx_busy = unsafe { can_tx_request(CAN1, s3, TX_ID, TX_DLC, TX_DLO, TX_DHI) };
    let ti_busy = rd(TI0R);
    wr(TSR, TSR_TME0);
    let s_tx = unsafe { can_tx_request(CAN1, s3, TX_ID, TX_DLC, TX_DLO, TX_DHI) };
    let ti = rd(TI0R);
    let tdt = rd(TDT0R);
    let tdl = rd(TDL0R);
    let tdh = rd(TDH0R);
    tx(
        if tx_sleep == CAN_FAULT
            && tx_busy == CAN_FAULT
            && ti_busy == 0
            && s_tx != CAN_FAULT
            && unsafe { can_mode(s_tx) } == PH_NORMAL
            && ti == TI0R_EXPECT
            && tdt == (TX_DLC & 0xF)
            && tdl == TX_DLO
            && tdh == TX_DHI
        {
            b"can-tx-ok\n"
        } else {
            b"can-tx-bad\n"
        },
    );

    // 6) RX gating: off-Normal + nothing-pending rejected; a pending message releases
    //    FIFO 0 by setting exactly RFOM0.
    let rx_sleep = unsafe { can_rx_release(CAN1, 0) };
    wr(RF0R, 0);
    let rx_empty = unsafe { can_rx_release(CAN1, s3) };
    let rf_empty = rd(RF0R);
    wr(RF0R, 1);
    let s_rx = unsafe { can_rx_release(CAN1, s3) };
    let rf = rd(RF0R);
    tx(
        if rx_sleep == CAN_FAULT
            && rx_empty == CAN_FAULT
            && rf_empty == 0
            && s_rx != CAN_FAULT
            && unsafe { can_mode(s_rx) } == PH_NORMAL
            && rf == RF0R_RFOM0
        {
            b"can-rx-ok\n"
        } else {
            b"can-rx-bad\n"
        },
    );

    tx(b"can-gate done\n");
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}
