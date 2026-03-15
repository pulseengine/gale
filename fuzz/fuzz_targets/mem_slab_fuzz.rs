#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use gale::error::*;
use gale::mem_slab::MemSlab;

#[derive(Arbitrary, Debug)]
enum Op {
    Alloc,
    Free,
    NumFree,
    NumUsed,
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    block_size: u32,
    num_blocks: u32,
    ops: Vec<Op>,
}

fuzz_target!(|input: FuzzInput| {
    // Bound parameters to avoid trivial rejection.
    let block_size = (input.block_size % 256).max(1);
    let num_blocks = (input.num_blocks % 64).max(1);
    let mut s = match MemSlab::init(block_size, num_blocks) {
        Ok(s) => s,
        Err(_) => return,
    };

    for op in &input.ops {
        match op {
            Op::Alloc => {
                let rc = s.alloc();
                if rc == OK {
                    assert!(s.num_used_get() > 0);
                } else {
                    assert_eq!(rc, ENOMEM);
                    assert!(s.is_full());
                }
            }
            Op::Free => {
                let rc = s.free();
                if rc == OK {
                    assert!(s.num_used_get() < num_blocks);
                } else {
                    assert_eq!(rc, EINVAL);
                    assert!(s.is_empty());
                }
            }
            Op::NumFree => {
                let _ = s.num_free_get();
            }
            Op::NumUsed => {
                let _ = s.num_used_get();
            }
        }
        // Invariant: conservation
        assert_eq!(s.num_free_get() + s.num_used_get(), num_blocks);
    }
});
