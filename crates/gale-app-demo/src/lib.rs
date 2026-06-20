//! gale-app-demo — a gale application component that IMPORTS gale:kernel.
//! It contains no kernel logic; composed with gale-kiln the imports resolve to
//! the verified gale::* decisions. Proves the no-C-FFI component loop.
#![allow(warnings)]
wit_bindgen::generate!({ world: "demo", path: "wit", generate_all });

use gale::kernel::{sem, msgq};

struct Component;

impl Guest for Component {
    fn run_demo() -> u32 {
        // take from an empty sem, no-wait -> WOULD_BLOCK (=1)
        let t = sem::take(0, true) as u32;
        // give below limit, no waiter -> INCREMENT (=1)
        let g = sem::give(0, 3, false) as u32;
        // put into a full queue, no-wait -> FULL (=3)
        let p = msgq::put(0, 4, 4, false, true) as u32;
        (t & 0x3) | ((g & 0x3) << 2) | ((p & 0x3) << 4)
    }
}

export!(Component);
