/*
 * Minimal wasm-side host of z_impl_k_mutex_unlock — the mutex analogue of
 * sem_give_shim.c. Replicates gale-smart-data's gale_mutex.c unlock hot path
 * with kernel APIs as externs (which become wasm imports), so the shim itself
 * compiles to wasm32-unknown-unknown without pulling in Zephyr headers. This
 * puts the C <-> Rust seam (gale_k_mutex_unlock_decide) INSIDE the wasm bundle:
 * wasm-ld merges it with gale-ffi.wasm, loom inlines through it, synth produces
 * ARM with the seam dissolved (no `bl gale_k_mutex_unlock_decide`).
 *
 * First silicon measurement (NUCLEO-G474RE, synth 0.11.41 + loom 1.1.13):
 * k_mutex_unlock = 501 cyc (native gale ref 124). The 0.11.40 silent-miscompile
 * deadlock was the synth#331 spill-slot collision; fixed in v0.11.41 (the
 * no-waiter lock_count store now derives from the mutex pointer, not a clobbered
 * slot). See gale-smart-data NOTES-wasm-cross-lto-spike.md + jess repro/synth-331/.
 */

#include <stdint.h>

/* Opaque k_thread — never deref'd in the shim; the kernel owns it. */
struct k_thread;
struct k_spinlock { uint8_t lock_internal; };
typedef struct { uint32_t key; } k_spinlock_key_t;

/* Faithful mirror of Zephyr v4.4.0 struct k_mutex (CONFIG_POLL=n,
 * CONFIG_WAITQ_SCALABLE=n): _wait_q_t == sys_dlist_t == {head, tail} — TWO
 * pointers, so owner@8 / lock_count@12. A single-pointer wait_q skews owner/count
 * by 4 bytes (owner reads the dlist tail -> always "not current" -> -EPERM,
 * observed on silicon as SELFCHECK rc=-1). Same unfaithful-shim class the sem
 * shim's wait_q fix closed. We touch only owner/lock_count; kernel owns the rest. */
struct k_mutex {
    void            *wq_head;
    void            *wq_tail;
    struct k_thread *owner;
    uint32_t         lock_count;
    int              owner_orig_prio;
};

/* Kernel API externs -> wasm imports -> native bl after synth-emit
 * (renamed to gale_w_* wrappers by build-wasm-dist.sh's objcopy pass). */
extern k_spinlock_key_t  k_spin_lock(struct k_spinlock *);
extern void              k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern struct k_thread * z_unpend_first_thread(void *wait_q);
extern void              z_ready_thread(struct k_thread *);
extern void              arch_thread_return_value_set(struct k_thread *, uint32_t);
extern int               z_reschedule(struct k_spinlock *, k_spinlock_key_t);
extern struct k_thread * gale_w_current(void);   /* _current, out-of-line */

/* The verified Rust decision — same wasm module after merge; loom inlines it. */
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
    /* UNLOCKED: hand off to the highest-priority waiter, if any.
     * wait_q is k_mutex's first member, so &mutex == &mutex->wait_q. */
    struct k_thread *new_owner = z_unpend_first_thread((void *)mutex);
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
