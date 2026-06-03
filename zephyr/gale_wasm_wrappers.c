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
