#include <stdint.h>
uint32_t rv_cs_call(uint32_t,uint32_t,int32_t,uint32_t);        /* synth control_step via s11 tramp */
uint32_t control_step_native(uint32_t,uint32_t,int32_t,uint32_t);
#define UART (*(volatile uint8_t*)0x10000000u)
static void pc_(char c){UART=(uint8_t)c;}static void ps_(const char*s){while(*s)pc_(*s++);}
static void pd_(int32_t v){char b[12];int i=0;uint32_t u=v;if(!u)b[i++]='0';while(u){b[i++]='0'+u%10;u/=10;}while(i)pc_(b[--i]);}
static inline uint32_t rc(void){uint32_t c;__asm__ volatile("csrr %0, mcycle":"=r"(c));return c;}
int main(void){
  uint32_t s=rv_cs_call(3000,50,40,0), n=control_step_native(3000,50,40,0);
  ps_("SYNTH-RV32 control_step=");pd_(s);ps_(" native=");pd_(n);ps_(" (exp 2165333)\n");
  uint32_t ovh=~0u;for(int i=0;i<200;i++){uint32_t a=rc(),b=rc();uint32_t d=b-a;if(d<ovh)ovh=d;}
  volatile uint32_t sink=0;uint32_t bs=~0u,bn=~0u;
  for(int i=0;i<200;i++){uint32_t t0=rc();sink+=rv_cs_call(3000,50,40,0);uint32_t t1=rc();uint32_t d=t1-t0;if(d<bs)bs=d;}
  for(int i=0;i<200;i++){uint32_t t0=rc();sink+=control_step_native(3000,50,40,0);uint32_t t1=rc();uint32_t d=t1-t0;if(d<bn)bn=d;}
  ps_("E,control_step,synth_rv32=");pd_((int32_t)(bs-ovh));ps_(" (incl ~6-cyc s11 tramp),native_rv32=");pd_((int32_t)(bn-ovh));ps_("\n=== END ===\n");for(;;){}
}
