#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use gale::event::Event;

#[derive(Arbitrary, Debug)]
enum Op {
    Post(u32),
    Set(u32),
    SetMasked(u32, u32),
    Clear(u32),
    WaitAny(u32),
    WaitAll(u32),
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    ops: Vec<Op>,
}

fuzz_target!(|input: FuzzInput| {
    let mut ev = Event::init();

    for op in &input.ops {
        match op {
            Op::Post(bits) => {
                let before = ev.events_get();
                ev.post(*bits);
                let after = ev.events_get();
                // Monotonic: old bits preserved
                assert_eq!(before & after, before);
            }
            Op::Set(bits) => {
                ev.set(*bits);
                assert_eq!(ev.events_get(), *bits);
            }
            Op::SetMasked(bits, mask) => {
                let before = ev.events_get();
                ev.set_masked(*bits, *mask);
                let after = ev.events_get();
                // Unmasked bits unchanged
                assert_eq!(after & !mask, before & !mask);
                // Masked bits from new_events
                assert_eq!(after & mask, bits & mask);
            }
            Op::Clear(bits) => {
                let before = ev.events_get();
                ev.clear(*bits);
                let after = ev.events_get();
                assert_eq!(after, before & !bits);
            }
            Op::WaitAny(desired) => {
                let events = ev.events_get();
                assert_eq!(ev.wait_check_any(*desired), (events & desired) != 0);
            }
            Op::WaitAll(desired) => {
                let events = ev.events_get();
                assert_eq!(ev.wait_check_all(*desired), (events & desired) == *desired);
            }
        }
    }
});
