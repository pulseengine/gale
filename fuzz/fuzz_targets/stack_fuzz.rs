#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use gale::error::*;
use gale::stack::Stack;

#[derive(Arbitrary, Debug)]
enum Op {
    Push,
    Pop,
    NumFree,
    NumUsed,
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    capacity: u32,
    ops: Vec<Op>,
}

fuzz_target!(|input: FuzzInput| {
    // Bound capacity to avoid trivial rejection.
    let capacity = (input.capacity % 64).max(1);
    let mut s = match Stack::init(capacity) {
        Ok(s) => s,
        Err(_) => return,
    };

    for op in &input.ops {
        match op {
            Op::Push => {
                let rc = s.push();
                if rc == OK {
                    assert!(s.num_used() > 0);
                } else {
                    assert_eq!(rc, ENOMEM);
                    assert!(s.is_full());
                }
            }
            Op::Pop => {
                let rc = s.pop();
                if rc == OK {
                    assert!(s.num_used() < capacity);
                } else {
                    assert_eq!(rc, EBUSY);
                    assert!(s.is_empty());
                }
            }
            Op::NumFree => {
                let _ = s.num_free();
            }
            Op::NumUsed => {
                let _ = s.num_used();
            }
        }
        // Invariant: conservation
        assert_eq!(s.num_free() + s.num_used(), capacity);
    }
});
