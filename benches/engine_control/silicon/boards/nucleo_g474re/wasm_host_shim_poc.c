/*
 * Minimal wasm-side host of z_impl_k_sem_give.
 *
 * Replicates the hot-path structure of gale-smart-data's gale_sem.c
 * but with kernel APIs as externs (which become wasm imports), so the
 * shim itself compiles to wasm32-unknown-unknown without pulling in
 * Zephyr headers. This puts the C ↔ Rust seam INSIDE the wasm bundle —
 * meld/wasm-ld merges it with gale-ffi.wasm, loom inlines through it,
 * synth produces ARM with the seam dissolved.
 *
 * The published silicon-LTO recovery for handoff cycles was 471 (-10.8%
 * vs baseline 528) at ADC=n, or 558 (vs baseline 506) at ADC=y. If the
 * loom→synth output matches LLVM-LTO's inlined-into-z_impl_k_sem_give
 * shape (no `bl gale_k_sem_give_decide`, decision logic merged into
 * z_impl_k_sem_give's body), we have evidence that the verified-Rust
 * route is at parity with LTO at the codegen level.
 */

#include <stdint.h>

/* Opaque k_thread — we never deref this in the shim, kernel does. */
struct k_thread;
/* k_spinlock has a non-zero size in Zephyr; give it one byte so the
 * static instance below is a complete type. The kernel's k_spin_lock
 * etc. take a pointer; they don't care about our internal size. */
struct k_spinlock { uint8_t lock_internal; };

/* Match gale-smart-data layout — count + limit are the first two u32s
 * in a k_sem. We don't touch the rest. */
struct k_sem {
    uint32_t count;
    uint32_t limit;
};

typedef struct { uint32_t key; } k_spinlock_key_t;

/* Kernel API externs — wasm imports at link time, native bl at synth-emit. */
extern k_spinlock_key_t  k_spin_lock(struct k_spinlock *);
extern void              k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern struct k_thread * z_unpend_first_thread(void *wait_q);
extern void              z_ready_thread(struct k_thread *);
extern void              arch_thread_return_value_set(struct k_thread *, uint32_t);
extern int               z_reschedule(struct k_spinlock *, k_spinlock_key_t);

/* The verified Rust function — same wasm module after wasm-ld merge,
 * so loom can inline. */
extern uint64_t gale_k_sem_give_decide(uint32_t count, uint32_t limit,
                                       uint32_t has_waiter);

/* AAPCS-packed decision struct — matches gale-smart-data's layout. */
union gale_sem_give_decision_u {
    uint64_t raw;
    struct {
        uint8_t  action;
        uint8_t  pad[3];
        uint32_t new_count;
    } dec;
};

#define GALE_SEM_ACTION_WAKE 1

/* The shim spinlock — kept simple, kernel handles real one. */
static struct k_spinlock shim_lock_raw;

/* The hot path. THIS IS THE FFI SEAM. */
void z_impl_k_sem_give(struct k_sem *sem) {
    k_spinlock_key_t key = k_spin_lock(&shim_lock_raw);

    /* Extract: try to unpend first waiter (kernel side effect). */
    struct k_thread *thread = z_unpend_first_thread((void *)0);

    /* Decide: Rust determines action via 8-byte u64-packed decision. */
    union gale_sem_give_decision_u du;
    du.raw = gale_k_sem_give_decide(sem->count, sem->limit,
                                    thread != (struct k_thread *)0 ? 1U : 0U);

    /* Apply: execute the decision. */
    if (du.dec.action == GALE_SEM_ACTION_WAKE) {
        arch_thread_return_value_set(thread, 0U);
        z_ready_thread(thread);
    } else {
        sem->count = du.dec.new_count;
    }

    z_reschedule(&shim_lock_raw, key);
}
