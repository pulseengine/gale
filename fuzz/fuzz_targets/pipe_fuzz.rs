#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use gale::error::*;
use gale::pipe::Pipe;

#[derive(Arbitrary, Debug)]
enum Op {
    Write(u32),
    Read(u32),
    Reset,
    Close,
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    size: u32,
    ops: Vec<Op>,
}

fuzz_target!(|input: FuzzInput| {
    let size = (input.size % 128).max(1);
    let mut p = match Pipe::init(size) {
        Ok(p) => p,
        Err(_) => return,
    };

    for op in &input.ops {
        match op {
            Op::Write(len) => {
                let len = (*len % 64).max(1);
                match p.write_check(len) {
                    Ok(n) => {
                        assert!(n > 0 && n <= len);
                    }
                    Err(EAGAIN) => assert!(p.is_full()),
                    Err(EPIPE) => assert!(!p.is_open()),
                    Err(ECANCELED) => assert!(p.is_resetting()),
                    Err(_) => {}
                }
            }
            Op::Read(len) => {
                let len = (*len % 64).max(1);
                match p.read_check(len) {
                    Ok(n) => {
                        assert!(n > 0 && n <= len);
                    }
                    Err(EAGAIN) => assert!(p.is_empty()),
                    Err(EPIPE) => assert!(!p.is_open()),
                    Err(ECANCELED) => assert!(p.is_resetting()),
                    Err(_) => {}
                }
            }
            Op::Reset => p.reset(),
            Op::Close => p.close(),
        }
        // Conservation check (only valid when not reset/closed)
        assert_eq!(p.space_get() + p.data_get(), size);
    }
});
