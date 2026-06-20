/*
 * Minimal wasm-side host of z_impl_k_sem_take (the dissolved blocking-acquire path).
 *
 * Completes the sem primitive for wasm-cross-LTO: #59 ships k_sem_give (the
 * give/handoff half); this is k_sem_take (the acquire/block half), so the full
 * v0.1.0 sem primitive can dissolve, not just give. Same construction as
 * sem_give_shim.c (kernel APIs as wasm imports, the verified Rust decide rides
 * the bundle so loom inlines the seam). u64 decide → sem-class, NOT
 * --native-pointer-abi, NOT gated on synth#345.
 *
 * Faithful to the fork's z_impl_k_sem_take: count>0 → acquire (count--, unlock);
 * K_NO_WAIT → -EBUSY (unlock); else z_pend_curr(&lock,key,&sem->wait_q,timeout).
 * Unlike give, the acquire/nowait paths use k_spin_unlock (no reschedule); the
 * block path hands off to z_pend_curr (which releases the lock and blocks).
 */

#include <stdint.h>

struct k_thread;
struct k_spinlock { uint8_t lock_internal; };
typedef struct { uint32_t key; } k_spinlock_key_t;
typedef struct { int64_t ticks; } k_timeout_t;   /* K_NO_WAIT == {0} */

/* Faithful Zephyr v4.4.0 k_sem (CONFIG_POLL=n / WAITQ_SCALABLE=n). */
struct k_sem {
    void    *wq_head;
    void    *wq_tail;
    uint32_t count;
    uint32_t limit;
};

extern k_spinlock_key_t k_spin_lock(struct k_spinlock *);
extern void             k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern int              z_pend_curr(struct k_spinlock *, k_spinlock_key_t,
                                    void *wait_q, k_timeout_t timeout);

extern uint64_t gale_k_sem_take_decide(uint32_t count, uint32_t is_no_wait);

/* AAPCS-packed decision: matches #[repr(C)] GaleSemTakeDecision (8 bytes). */
union gale_sem_take_decision_u {
    uint64_t raw;
    struct {
        uint8_t  action;
        uint8_t  pad[3];
        uint32_t new_count;
    } dec;
};

#define GALE_SEM_TAKE_ACQUIRED    0
#define GALE_SEM_TAKE_WOULD_BLOCK 1
#define GALE_SEM_TAKE_PEND        2
#define GALE_EBUSY (-16)

static struct k_spinlock shim_lock_raw;

/* The hot path. THIS IS THE FFI SEAM. */
int z_impl_k_sem_take(struct k_sem *sem, k_timeout_t timeout) {
    k_spinlock_key_t key = k_spin_lock(&shim_lock_raw);

    uint32_t is_no_wait = (timeout.ticks == 0) ? 1U : 0U;

    union gale_sem_take_decision_u du;
    du.raw = gale_k_sem_take_decide(sem->count, is_no_wait);

    int ret;
    switch (du.dec.action) {
    case GALE_SEM_TAKE_ACQUIRED:
        sem->count = du.dec.new_count;
        k_spin_unlock(&shim_lock_raw, key);
        ret = 0;
        break;
    case GALE_SEM_TAKE_WOULD_BLOCK:
        k_spin_unlock(&shim_lock_raw, key);
        ret = GALE_EBUSY;
        break;
    case GALE_SEM_TAKE_PEND:
    default:
        /* z_pend_curr releases the lock and blocks the caller; wait_q is
         * k_sem's first member. */
        ret = z_pend_curr(&shim_lock_raw, key, (void *)sem, timeout);
        break;
    }
    return ret;
}
