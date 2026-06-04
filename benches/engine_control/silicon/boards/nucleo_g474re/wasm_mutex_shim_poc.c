/*
 * Minimal wasm-side host of z_impl_k_mutex_unlock — the mutex analogue of
 * wasm_host_shim_poc.c (sem). Replicates gale_mutex.c's unlock hot path with
 * kernel APIs as externs (wasm imports), so it compiles to wasm32 without
 * Zephyr headers. Puts the C<->Rust seam (gale_k_mutex_unlock_decide) INSIDE
 * the wasm bundle; wasm-ld merges it with gale-ffi.wasm, loom inlines through
 * it, synth emits ARM with the seam dissolved (no `bl gale_k_mutex_unlock_decide`).
 * This is the dissolvability proof for the mutex primitive (2nd after sem 907cyc).
 */
#include <stdint.h>

struct k_thread;
struct k_spinlock { uint8_t lock_internal; };
typedef struct { uint32_t key; } k_spinlock_key_t;

/* k_mutex hot-path fields (layout-compatible prefix; kernel owns the rest). */
struct k_mutex {
    void     *wait_q;            /* _wait_q_t — opaque to the shim */
    struct k_thread *owner;
    uint32_t  lock_count;
    int       owner_orig_prio;
};

/* Kernel API externs -> wasm imports -> native bl after synth-emit. */
extern k_spinlock_key_t  k_spin_lock(struct k_spinlock *);
extern void              k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern struct k_thread * z_unpend_first_thread(void *wait_q);
extern void              z_ready_thread(struct k_thread *);
extern void              arch_thread_return_value_set(struct k_thread *, uint32_t);
extern int               z_reschedule(struct k_spinlock *, k_spinlock_key_t);
extern struct k_thread * gale_w_current(void);   /* _current, out-of-line */

/* The verified Rust decision — same wasm module after merge, loom inlines it. */
struct gale_mutex_unlock_decision { int32_t ret; uint8_t action; uint32_t new_lock_count; };
extern struct gale_mutex_unlock_decision gale_k_mutex_unlock_decide(
    uint32_t lock_count, uint32_t owner_is_null, uint32_t owner_is_current);

#define GALE_MUTEX_UNLOCK_RELEASED 0
#define GALE_MUTEX_UNLOCK_UNLOCKED 1
#define GALE_MUTEX_UNLOCK_ERROR    2

static struct k_spinlock lock;

int z_impl_k_mutex_unlock(struct k_mutex *mutex)
{
    k_spinlock_key_t key = k_spin_lock(&lock);
    struct k_thread *cur = gale_w_current();

    struct gale_mutex_unlock_decision d = gale_k_mutex_unlock_decide(
        mutex->lock_count,
        (mutex->owner == (struct k_thread *)0) ? 1U : 0U,
        (mutex->owner == cur) ? 1U : 0U);

    if (d.action == GALE_MUTEX_UNLOCK_ERROR) {
        k_spin_unlock(&lock, key);
        return d.ret;
    }
    if (d.action == GALE_MUTEX_UNLOCK_RELEASED) {
        mutex->lock_count = d.new_lock_count;   /* reentrant: still held */
        k_spin_unlock(&lock, key);
        return 0;
    }
    /* UNLOCKED: hand off to the highest-priority waiter, if any. */
    struct k_thread *new_owner = z_unpend_first_thread(mutex->wait_q);
    mutex->owner = new_owner;
    if (new_owner != (struct k_thread *)0) {
        mutex->lock_count = 1U;
        arch_thread_return_value_set(new_owner, 0);
        z_ready_thread(new_owner);
        z_reschedule(&lock, key);
    } else {
        mutex->lock_count = 0U;
        k_spin_unlock(&lock, key);
    }
    return 0;
}
