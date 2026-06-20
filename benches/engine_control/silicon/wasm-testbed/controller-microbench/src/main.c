/* controller_step (7-arg value fn) wasm-cross-LTO vs native on G474RE silicon.
 * synth passes args in r0-r7; ctl_tramp.S shuffles AAPCS->r0-r7. DWT min/200, overhead-subtracted. */
#include <zephyr/kernel.h>
#include <stdint.h>
#define DEMCR (*(volatile uint32_t*)0xE000EDFCu)
#define DWT_CTRL (*(volatile uint32_t*)0xE0001000u)
#define DWT_CYCCNT (*(volatile uint32_t*)0xE0001004u)
typedef uint32_t (*c7_t)(int32_t,int32_t,int32_t,int32_t,int32_t,int32_t,uint32_t);
extern uint32_t controller_step_decide(int32_t,int32_t,int32_t,int32_t,int32_t,int32_t,uint32_t); /* tramp->synth */

__attribute__((noinline))
uint32_t n_controller(int32_t rm,int32_t rr,int32_t pm,int32_t pr,int32_t ym,int32_t yr,uint32_t u){
	int32_t a=-(rm>>6)-(rr>>7), e=-(pm>>6)-(pr>>7), r=-(ym>>6)-(yr>>7);
	if(a>127)a=127; if(a<-127)a=-127; if(e>127)e=127; if(e<-127)e=-127; if(r>127)r=127; if(r<-127)r=-127;
	return ((uint32_t)(uint8_t)(int8_t)a)|((uint32_t)(uint8_t)(int8_t)e<<8)|((uint32_t)(uint8_t)(int8_t)r<<16)|((uint32_t)(u&0xFFu)<<24);
}
static inline void dwt_init(void){ DEMCR|=(1u<<24); DWT_CYCCNT=0; DWT_CTRL|=1u; }
static uint32_t m7(c7_t fn,volatile uint32_t*s){
	uint32_t best=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ uint32_t t0=DWT_CYCCNT; uint32_t r=fn(8000,256,-4000,128,2000,-256,5); uint32_t t1=DWT_CYCCNT; *s+=r; uint32_t d=t1-t0; if(d<best)best=d; }
	return best;
}
int main(void){
	dwt_init();
	volatile c7_t sc=(c7_t)controller_step_decide, nc=(c7_t)n_controller;
	uint32_t vs=sc(8000,256,-4000,128,2000,-256,5), vn=nc(8000,256,-4000,128,2000,-256,5);
	printk("SELFCHECK synth=0x%08x native=0x%08x %s\n", vs, vn, vs==vn?"MATCH":"MISMATCH");
	uint32_t ovh=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ uint32_t a0=DWT_CYCCNT; uint32_t a1=DWT_CYCCNT; uint32_t d=a1-a0; if(d<ovh)ovh=d; }
	volatile uint32_t sink=0;
	printk("E,controller_step,synth=%u,native=%u\n", m7(sc,&sink)-ovh, m7(nc,&sink)-ovh);
	printk("SINK=%u ovh=%u\n",sink,ovh);
	printk("=== END ===\n");
	return 0;
}
