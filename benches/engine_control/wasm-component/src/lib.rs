//! engine-control as a WebAssembly **Component**, built on the forked
//! pulseengine/wit-bindgen (amortized per-item stream allocation).
//!
//! Two exports, one component:
//!  - `control.step` — synchronous, runs on any component host today.
//!  - `crank-stream.process` — async: consumes a `stream<sensors>` (the real
//!    engine_control crank-sample shape). Each sample is read with
//!    `StreamReader::next`, which in the fork reuses one cached buffer across
//!    samples (first read allocs, the rest don't) — the embedded heap-bounding
//!    property the relay/embedded teams need.
//!
//! Algorithm is a faithful port of ../src/control.c (table lookups + integer
//! corrections, no float, no alloc), so the component is functionally identical
//! to the C the bench dissolves to native.
#![allow(warnings)]

wit_bindgen::generate!({
    world: "engine-control",
    path: "wit",
    // async-ness comes from the WIT itself: `crank-stream.process` is `async
    // func` (the streaming export), `control.step` is a plain sync `func`.
});

use exports::gale::engine_control::control::{Actuators, Guest as ControlGuest, Sensors};
use exports::gale::engine_control::crank_stream::Guest as CrankStreamGuest;
use wit_bindgen::StreamReader;

const RPM_BINS: usize = 20;
const LOAD_BINS: usize = 20;
include!("tables.rs");

#[inline]
fn rpm_bin(rpm: u32) -> usize {
    let b = (rpm / 500) as usize;
    if b >= RPM_BINS { RPM_BINS - 1 } else { b }
}
#[inline]
fn load_bin(load_pct: u32) -> usize {
    let b = (load_pct / 5) as usize;
    if b >= LOAD_BINS { LOAD_BINS - 1 } else { b }
}
#[inline]
fn coolant_enrichment_permille(coolant_c: i32) -> u32 {
    if coolant_c >= 80 {
        0
    } else if coolant_c <= 0 {
        300
    } else {
        (((80 - coolant_c) * 300) / 80) as u32
    }
}

/// The pure control step — shared by the sync and the streaming exports.
fn control_step(input: &Sensors) -> Actuators {
    let rb = rpm_bin(input.rpm);
    let lb = load_bin(input.load_pct);

    let mut advance = SPARK_ADVANCE_TABLE[rb][lb] as i32 - input.knock_retard as i32;
    if advance < 0 {
        advance = 0;
    }

    let base_fuel = FUEL_DURATION_TABLE[rb][lb] as u32;
    let enrich = coolant_enrichment_permille(input.coolant_c);
    let mut corrected = base_fuel + (base_fuel * enrich / 1000);
    if corrected > u16::MAX as u32 {
        corrected = u16::MAX as u32;
    }

    Actuators {
        spark_advance_deg: advance,
        fuel_duration_us: corrected,
    }
}

struct Component;

impl ControlGuest for Component {
    fn step(input: Sensors) -> Actuators {
        control_step(&input)
    }
}

impl CrankStreamGuest for Component {
    /// Drain the crank-sample stream, computing the control step per sample.
    /// `samples.next().await` reuses the fork's cached buffer — no per-sample
    /// heap allocation after the first. Returns the count processed.
    async fn process(mut samples: StreamReader<Sensors>) -> u32 {
        let mut n: u32 = 0;
        while let Some(s) = samples.next().await {
            let _ = control_step(&s);
            n = n.wrapping_add(1);
        }
        n
    }
}

export!(Component);
