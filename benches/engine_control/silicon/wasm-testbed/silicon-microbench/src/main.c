/* Decompose the wasm-cross-LTO vs native gap on filter_axis, on real silicon.
 * Three synth functions (v_full / v_nodiv / v_div) vs native gcc equivalents,
 * all called via volatile fn-ptr (thumb bit set; synth#170 = no $t symbol) so
 * call overhead is identical. DWT CYCCNT @170 MHz, min over 200 iters. */
#include <zephyr/kernel.h>
#include <stdint.h>

#define DEMCR      (*(volatile uint32_t *)0xE000EDFCu)
#define DWT_CTRL   (*(volatile uint32_t *)0xE0001000u)
#define DWT_CYCCNT (*(volatile uint32_t *)0xE0001004u)

extern char v_full, v_nodiv, v_div;   /* synth symbols in fv_algo.o */
typedef int32_t (*f3_t)(int32_t, int32_t, int32_t);
typedef int32_t (*f1_t)(int32_t);

__attribute__((noinline)) int32_t n_full(int32_t p,int32_t g,int32_t a){ int32_t t=p+g; return (t*980+a*20)/1000; }
__attribute__((noinline)) int32_t n_nodiv(int32_t p,int32_t g,int32_t a){ int32_t t=p+g; return t*980+a*20; }
__attribute__((noinline)) int32_t n_div(int32_t x){ return x/1000; }

static inline void dwt_init(void){ DEMCR |= (1u<<24); DWT_CYCCNT = 0; DWT_CTRL |= 1u; }

static uint32_t m3(f3_t fn,int32_t p,int32_t g,int32_t a,volatile int32_t*s){
	uint32_t best=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ uint32_t t0=DWT_CYCCNT; int32_t r=fn(p,g,a); uint32_t t1=DWT_CYCCNT; *s+=r; uint32_t d=t1-t0; if(d<best)best=d; }
	return best;
}
static uint32_t m1(f1_t fn,int32_t x,volatile int32_t*s){
	uint32_t best=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ uint32_t t0=DWT_CYCCNT; int32_t r=fn(x); uint32_t t1=DWT_CYCCNT; *s+=r; uint32_t d=t1-t0; if(d<best)best=d; }
	return best;
}

int main(void){
	dwt_init();
	f3_t s_full =(f3_t)((uintptr_t)&v_full |1u);
	f3_t s_nodiv=(f3_t)((uintptr_t)&v_nodiv|1u);
	f1_t s_div  =(f1_t)((uintptr_t)&v_div  |1u);
	volatile f3_t nf=n_full, nn=n_nodiv; volatile f1_t nd=n_div;

	/* selfcheck */
	printk("SELFCHECK full s=%d n=%d (exp 1088)  div s=%d n=%d (exp -1917)\n",
	       s_full(1000,100,500), nf(1000,100,500), s_div(-1917000), nd(-1917000));

	uint32_t ovh=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ uint32_t a0=DWT_CYCCNT; uint32_t a1=DWT_CYCCNT; uint32_t d=a1-a0; if(d<ovh)ovh=d; }
	volatile int32_t sink=0;
	int32_t p=1000,g=100,a=500;
	printk("E,full ,synth=%u,native=%u\n",  m3(s_full ,p,g,a,&sink)-ovh, m3(nf,p,g,a,&sink)-ovh);
	printk("E,nodiv,synth=%u,native=%u\n",  m3(s_nodiv,p,g,a,&sink)-ovh, m3(nn,p,g,a,&sink)-ovh);
	printk("E,div  ,synth=%u,native=%u\n",  m1(s_div ,1234567,&sink)-ovh, m1(nd,1234567,&sink)-ovh);
	printk("SINK=%d ovh=%u\n",(int)sink,ovh);
	printk("=== END ===\n");
	return 0;
}
