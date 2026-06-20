#include <zephyr/kernel.h>
#include <stdint.h>
#include <string.h>
#define DEMCR (*(volatile uint32_t*)0xE000EDFCu)
#define DWT_CTRL (*(volatile uint32_t*)0xE0001000u)
#define DWT_CYCCNT (*(volatile uint32_t*)0xE0001004u)
typedef uint32_t (*c4)(uint32_t,uint32_t,int32_t,uint32_t);
extern uint32_t control_step_decide(uint32_t,uint32_t,int32_t,uint32_t);  /* tramp->synth */
extern uint32_t n_control_step(uint32_t,uint32_t,int32_t,uint32_t);       /* native */
/* wasm linmem: tables live at offset 65536 (spark 400B) + 65936 (fuel 800B) */
uint8_t wasm_linmem[0x10A00] __attribute__((aligned(8)));
extern const int8_t  spark_advance_table[20][20];
extern const uint16_t fuel_duration_table[20][20];
static inline void dwt(void){DEMCR|=(1u<<24);DWT_CYCCNT=0;DWT_CTRL|=1u;}
static uint32_t m(c4 fn,volatile uint32_t*s){uint32_t b=~0u;for(int i=0;i<200;i++){uint32_t t0=DWT_CYCCNT;uint32_t r=fn(3000,50,40,0);uint32_t t1=DWT_CYCCNT;*s+=r;uint32_t d=t1-t0;if(d<b)b=d;}return b;}
int main(void){
	dwt();
	memcpy(&wasm_linmem[65536], spark_advance_table, 400);
	memcpy(&wasm_linmem[65936], fuel_duration_table, 800);
	volatile c4 sc=(c4)control_step_decide, nc=(c4)n_control_step;
	uint32_t vs=sc(3000,50,40,0), vn=nc(3000,50,40,0);
	printk("SELFCHECK synth=%u native=%u exp=2165333 %s\n", vs, vn, (vs==vn&&vs==2165333u)?"OK":"BAD");
	uint32_t ov=~0u;for(int i=0;i<200;i++){uint32_t a=DWT_CYCCNT,b=DWT_CYCCNT;uint32_t d=b-a;if(d<ov)ov=d;}
	volatile uint32_t sink=0;
	printk("E,control_step,synth=%u,native=%u\n", m(sc,&sink)-ov, m(nc,&sink)-ov);
	printk("=== END ===\n"); return 0;
}
