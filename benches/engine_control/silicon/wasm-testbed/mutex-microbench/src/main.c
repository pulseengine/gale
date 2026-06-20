/* k_mutex_unlock cycle microbench — the 2nd primitive after k_sem_give (907).
 * Times the uncontended k_mutex_unlock hot path on real silicon: DWT CYCCNT
 * @170 MHz, min over 200 iters, overhead floor (two back-to-back reads)
 * subtracted. Which z_impl_k_mutex_unlock is measured is selected at BUILD
 * time: native gale (rustc-direct) vs wasm-cross-LTO (GALE_WASM_LTO_MUTEX_LIB).
 * Same methodology as silicon-microbench (control_step) + the sem 907 result. */
#include <zephyr/kernel.h>
#include <stdint.h>

#define DEMCR      (*(volatile uint32_t *)0xE000EDFCu)
#define DWT_CTRL   (*(volatile uint32_t *)0xE0001000u)
#define DWT_CYCCNT (*(volatile uint32_t *)0xE0001004u)
static inline void dwt_init(void){ DEMCR |= (1u<<24); DWT_CYCCNT = 0; DWT_CTRL |= 1u; }

static struct k_mutex m;

int main(void)
{
	dwt_init();
	k_mutex_init(&m);

	/* overhead floor: cost of two adjacent DWT reads */
	uint32_t ovh = 0xFFFFFFFFu;
	for (int i = 0; i < 200; i++) {
		uint32_t a0 = DWT_CYCCNT; uint32_t a1 = DWT_CYCCNT;
		uint32_t d = a1 - a0; if (d < ovh) ovh = d;
	}

	/* selfcheck: lock+unlock works (owner round-trips) */
	k_mutex_lock(&m, K_FOREVER);
	int rc = k_mutex_unlock(&m);
	printk("SELFCHECK k_mutex_unlock rc=%d owner=%p (exp 0 / NULL)\n",
	       rc, (void *)m.owner);

	/* measure uncontended k_mutex_unlock: lock (outside window) then time unlock */
	uint32_t best = 0xFFFFFFFFu;
	for (int i = 0; i < 200; i++) {
		k_mutex_lock(&m, K_FOREVER);
		uint32_t t0 = DWT_CYCCNT;
		k_mutex_unlock(&m);
		uint32_t t1 = DWT_CYCCNT;
		uint32_t d = t1 - t0; if (d < best) best = d;
	}
	printk("E,k_mutex_unlock,cyc=%u (ovh=%u, "
#ifdef GALE_WASM_LTO_OVERRIDE_MUTEX_UNLOCK
	       "wasm-cross-LTO"
#else
	       "gale-native"
#endif
	       ")\n", best - ovh, ovh);
	printk("=== END ===\n");
	return 0;
}
