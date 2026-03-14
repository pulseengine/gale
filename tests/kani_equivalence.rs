//! Kani bounded model checking: C↔Rust semantic equivalence.
//!
//! Models the C behavior as Rust functions and proves the Gale Rust
//! implementation produces identical results for all bounded inputs.
//!
//! This implements REQ-TRACTOR-001 Level 2.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

#[cfg(kani)]
mod equivalence {
    use gale::error::*;
    use gale::stack::Stack;

    // ── C model: stack ──────────────────────────────────────────────────
    //
    // Models the logic of gale_stack_push_validate / gale_stack_pop_validate
    // as called from kernel/stack.c. These are the exact checks the C shim
    // delegates to Rust — we re-derive them from the C source to verify
    // the Rust implementation matches.

    /// C model: k_stack_init validation (stack.c:37-42)
    /// Returns 0 on success, -EINVAL if num_entries == 0.
    fn c_stack_init_validate(num_entries: u32) -> i32 {
        if num_entries == 0 {
            EINVAL
        } else {
            OK
        }
    }

    /// C model: gale_stack_push_validate (stack.c:120-128)
    /// count = next - base, capacity = top - base.
    /// Returns (return_code, new_count).
    fn c_stack_push_validate(count: u32, capacity: u32) -> (i32, u32) {
        if count >= capacity {
            (ENOMEM, count)
        } else {
            (OK, count + 1)
        }
    }

    /// C model: gale_stack_pop_validate (stack.c:165-177)
    /// Returns (return_code, new_count).
    fn c_stack_pop_validate(count: u32) -> (i32, u32) {
        if count == 0 {
            (EBUSY, 0)
        } else {
            (OK, count - 1)
        }
    }

    // ── Equivalence harnesses ───────────────────────────────────────────

    /// Prove: Gale Stack::init produces same result as C model for all inputs.
    #[kani::proof]
    fn stack_init_equivalence() {
        let capacity: u32 = kani::any();
        kani::assume(capacity <= 256); // bound search space

        let c_result = c_stack_init_validate(capacity);
        let rust_result = Stack::init(capacity);

        match rust_result {
            Ok(s) => {
                assert!(c_result == OK, "C rejected but Rust accepted");
                assert!(s.num_used() == 0, "Rust init count != 0");
            }
            Err(e) => {
                assert!(c_result == EINVAL, "C accepted but Rust rejected");
                assert!(e == EINVAL, "Rust error code mismatch");
            }
        }
    }

    /// Prove: Gale Stack::push produces same result as C model for all states.
    #[kani::proof]
    fn stack_push_equivalence() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 64);
        let mut stack = Stack::init(capacity).unwrap();

        // Push to some arbitrary count
        let initial_count: u32 = kani::any();
        kani::assume(initial_count <= capacity);
        // Simulate state by pushing initial_count times
        for _ in 0..initial_count {
            stack.push();
        }

        let count_before = stack.num_used();
        let (c_rc, c_new_count) = c_stack_push_validate(count_before, capacity);
        let rust_rc = stack.push();
        let count_after = stack.num_used();

        assert!(rust_rc == c_rc, "push return code mismatch");
        assert!(count_after == c_new_count, "push count mismatch");
    }

    /// Prove: Gale Stack::pop produces same result as C model for all states.
    #[kani::proof]
    fn stack_pop_equivalence() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 64);
        let mut stack = Stack::init(capacity).unwrap();

        let initial_count: u32 = kani::any();
        kani::assume(initial_count <= capacity);
        for _ in 0..initial_count {
            stack.push();
        }

        let count_before = stack.num_used();
        let (c_rc, c_new_count) = c_stack_pop_validate(count_before);
        let rust_rc = stack.pop();
        let count_after = stack.num_used();

        assert!(rust_rc == c_rc, "pop return code mismatch");
        assert!(count_after == c_new_count, "pop count mismatch");
    }

    /// Prove: arbitrary sequence of push/pop operations produces same
    /// results in both C model and Gale implementation.
    #[kani::proof]
    #[kani::unwind(5)]
    fn stack_operation_sequence_equivalence() {
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 8);
        let mut stack = Stack::init(capacity).unwrap();
        let mut c_count: u32 = 0;

        // 4 arbitrary operations
        for _ in 0..4 {
            let do_push: bool = kani::any();
            if do_push {
                let (c_rc, c_new) = c_stack_push_validate(c_count, capacity);
                let rust_rc = stack.push();
                assert!(rust_rc == c_rc);
                c_count = c_new;
            } else {
                let (c_rc, c_new) = c_stack_pop_validate(c_count);
                let rust_rc = stack.pop();
                assert!(rust_rc == c_rc);
                c_count = c_new;
            }
            assert!(stack.num_used() == c_count);
        }
    }
}
