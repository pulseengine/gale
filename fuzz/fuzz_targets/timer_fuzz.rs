#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use gale::error::*;
use gale::timer::Timer;

#[derive(Arbitrary, Debug)]
enum Op {
    Start,
    Stop,
    Expire,
    StatusGet,
    StatusPeek,
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    period: u32,
    ops: Vec<Op>,
}

fuzz_target!(|input: FuzzInput| {
    let mut t = Timer::init(input.period);

    for op in &input.ops {
        match op {
            Op::Start => {
                t.start();
                assert!(t.is_running());
                assert_eq!(t.status_peek(), 0);
            }
            Op::Stop => {
                t.stop();
                assert!(!t.is_running());
                assert_eq!(t.status_peek(), 0);
            }
            Op::Expire => {
                let old = t.status_peek();
                match t.expire() {
                    Ok(n) => {
                        assert_eq!(n, old + 1);
                        assert_eq!(t.status_peek(), old + 1);
                    }
                    Err(e) => {
                        assert_eq!(e, EOVERFLOW);
                        assert_eq!(old, u32::MAX);
                        assert_eq!(t.status_peek(), u32::MAX);
                    }
                }
            }
            Op::StatusGet => {
                let old = t.status_peek();
                let got = t.status_get();
                assert_eq!(got, old);
                assert_eq!(t.status_peek(), 0);
            }
            Op::StatusPeek => {
                let _ = t.status_peek();
            }
        }
        // Invariant: period never changes
        assert_eq!(t.period_get(), input.period);
    }
});
