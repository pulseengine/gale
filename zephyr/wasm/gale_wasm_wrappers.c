/*
 * Out-of-line wrappers for the wasm-cross-LTO sem variant.
 *
 * The wasm shim (compiled clang->wasm-ld->loom->synth) calls the Zephyr
 * kernel APIs as out-of-line functions. Several of those — k_spin_lock,
 * k_spin_unlock, arch_thread_return_value_set — are `static inline` in
 * Zephyr headers, so they have no linkable symbol. These thin wrappers
 * (built WITH the Zephyr headers) provide out-of-line entry points; the
 * synth object's imports are objcopy-renamed to call gale_w_* instead.
 *
 * The spinlock wrappers ignore the pointer the wasm shim passes (it is a
 * wasm-linear-memory address, not a real lock) and use a real kernel-RAM
 * spinlock instead — correct and avoids touching the wasm linmem region.
 *
 * Only compiled when GALE_WASM_LTO_OVERRIDE_SEM_GIVE is defined.
 */
#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <string.h>
#include <wait_q.h>
#include <ksched.h>

static struct k_spinlock gale_wasm_lock;

k_spinlock_key_t gale_w_spin_lock(struct k_spinlock *ignored)
{
	ARG_UNUSED(ignored);
	return k_spin_lock(&gale_wasm_lock);
}

void gale_w_spin_unlock(struct k_spinlock *ignored, k_spinlock_key_t key)
{
	ARG_UNUSED(ignored);
	k_spin_unlock(&gale_wasm_lock, key);
}

int gale_w_reschedule(struct k_spinlock *ignored, k_spinlock_key_t key)
{
	ARG_UNUSED(ignored);
	z_reschedule(&gale_wasm_lock, key);   /* z_reschedule is void here */
	return 0;                             /* wasm shim expects an i32 (dropped) */
}

struct k_thread *gale_w_unpend_first_thread(_wait_q_t *wait_q)
{
	return z_unpend_first_thread(wait_q);
}

void gale_w_ready_thread(struct k_thread *thread)
{
	z_ready_thread(thread);
}

void gale_w_arch_thread_return_value_set(struct k_thread *thread, unsigned int value)
{
	arch_thread_return_value_set(thread, value);
}

/* Out-of-line _current accessor for the wasm-cross-LTO MUTEX shim
 * (wasm_mutex_shim_poc.c): _current is a macro (z_smp_current_get() /
 * _kernel.cpus[].current) with no linkable symbol, so the dissolved
 * z_impl_k_mutex_unlock imports gale_w_current instead. The mutex shim
 * reuses every other gale_w_* wrapper above (spinlock, unpend, ready,
 * reschedule, return_value_set) unchanged — only this one is new. */
struct k_thread *gale_w_current(void)
{
	return _current;
}

/* Mutex priority-inheritance restoration (gale#62): the dissolved unlock shim
 * treats k_thread as opaque, so it can't read base.prio or call z_thread_prio_set
 * (what the real z_impl_k_mutex_unlock reaches via the static adjust_owner_prio).
 * These two thin wrappers expose exactly that:
 *   - gale_w_adjust_thread_prio: restore `thread` to `new_prio` (undo the
 *     inherited boost on unlock) — mirrors adjust_owner_prio's body.
 *   - gale_w_thread_prio: read base.prio (so the shim can stash the new owner's
 *     original priority into mutex->owner_orig_prio on handoff). */
int gale_w_adjust_thread_prio(struct k_thread *thread, int new_prio)
{
	if (thread->base.prio != new_prio) {
		return z_thread_prio_set(thread, new_prio) ? 1 : 0;
	}
	return 0;
}

int gale_w_thread_prio(struct k_thread *thread)
{
	return thread->base.prio;
}

/* msgq put shim (msgq_put_shim.c) wrappers. k_thread is opaque in the shim and
 * k_msgq_put can copy arbitrary-size messages and block, none of which the
 * dissolved object can do against an opaque thread / without Zephyr headers:
 *   - gale_w_thread_swap_data: read base.swap_data — the waiting reader's
 *     destination buffer (set by k_msgq_get before it pended); the put copies
 *     the message into it on WAKE_READER.
 *   - gale_w_memcpy: out-of-line memcpy (the wasm shim has no libc; this keeps
 *     the byte copy a clean native call rather than a wasm bulk-memory op).
 *   - gale_w_msgq_pend: the PUT_PEND blocking path. wait_q / scheduling stay
 *     native (docs/wasm-module-distribution.md). The shim passes &msgq (==
 *     &msgq->wait_q, first member) and the 8-byte timeout as int64 ticks; we
 *     stash the message pointer in swap_data and z_pend_curr on gale_wasm_lock
 *     — the same lock the shim's spin ops use, so the put-pend / get-wake
 *     handshake shares one critical section (valid for the !SMP 0-byte-spinlock
 *     config these modules target). */
void *gale_w_thread_swap_data(struct k_thread *thread)
{
	return thread->base.swap_data;
}

void gale_w_memcpy(void *dst, const void *src, uint32_t n)
{
	(void)memcpy(dst, src, (size_t)n);
}

int gale_w_msgq_pend(void *wait_q, k_spinlock_key_t key,
		     const void *data, int64_t timeout_ticks)
{
	k_timeout_t timeout = { .ticks = (k_ticks_t)timeout_ticks };

	_current->base.swap_data = (void *)data;
	return z_pend_curr(&gale_wasm_lock, key, (_wait_q_t *)wait_q, timeout);
}
