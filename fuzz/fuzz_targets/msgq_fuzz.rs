#![no_main]

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

use gale::msgq::MsgQ;

#[derive(Arbitrary, Debug)]
enum Op {
    Put,
    Get,
    PutFront,
    PeekAt(u32),
    Purge,
    NumFree,
    NumUsed,
}

#[derive(Arbitrary, Debug)]
struct FuzzInput {
    msg_size: u32,
    max_msgs: u32,
    ops: Vec<Op>,
}

fuzz_target!(|input: FuzzInput| {
    // Constrain to valid init params.
    let msg_size = (input.msg_size % 256).max(1);
    let max_msgs = (input.max_msgs % 64).max(1);

    // Avoid overflow in buffer size.
    if msg_size.checked_mul(max_msgs).is_none() {
        return;
    }

    let mut mq = match MsgQ::init(msg_size, max_msgs) {
        Ok(mq) => mq,
        Err(_) => return,
    };

    for op in &input.ops {
        match op {
            Op::Put => {
                let _ = mq.put();
            }
            Op::Get => {
                let _ = mq.get();
            }
            Op::PutFront => {
                let _ = mq.put_front();
            }
            Op::PeekAt(idx) => {
                let _ = mq.peek_at(*idx);
            }
            Op::Purge => {
                mq.purge();
            }
            Op::NumFree => {
                let _ = mq.num_free_get();
            }
            Op::NumUsed => {
                let _ = mq.num_used_get();
            }
        }

        // Check invariants after every operation.
        assert!(mq.num_used_get() <= mq.max_msgs_get());
        assert_eq!(
            mq.num_free_get() + mq.num_used_get(),
            mq.max_msgs_get()
        );
        let expected_write =
            (mq.read_idx_get() + mq.num_used_get()) % mq.max_msgs_get();
        assert_eq!(mq.write_idx_get(), expected_write);
    }
});
