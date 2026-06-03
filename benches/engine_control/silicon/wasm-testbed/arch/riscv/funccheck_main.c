/* RV32 functional regression harness: runs synth's RISC-V output for each
 * dissolved leaf and asserts the verified-correct value. Catches RV32
 * miscompiles that the wasmtime/ARM-only testbed lanes miss (e.g. synth
 * #232: i32.div_s overflow guard clobbered the dividend on v0.11.26).
 * control_step reaches its tables via the s11 trampoline (rv_cs_call);
 * filter/controller are pure-register (0 memory ops) so called directly. */
#include <stdint.h>
uint32_t rv_cs_call(uint32_t,uint32_t,int32_t,uint32_t);          /* synth control_step via s11 tramp */
int32_t  filter_axis_decide(int32_t,int32_t,int32_t);             /* synth, direct */
uint32_t controller_step_decide(int32_t,int32_t,int32_t,int32_t,int32_t,int32_t,uint32_t);
#define UART (*(volatile uint8_t*)0x10000000u)
static void pc_(char c){UART=(uint8_t)c;}
static void ps_(const char*s){while(*s)pc_(*s++);}
static void pd_(uint32_t u){char b[12];int i=0;if(!u)b[i++]='0';while(u){b[i++]='0'+u%10;u/=10;}while(i)pc_(b[--i]);}
static void chk(const char*nm,uint32_t got,uint32_t exp){
  ps_(got==exp?"PASS ":"FAIL ");ps_(nm);ps_(" got=");pd_(got);ps_(" exp=");pd_(exp);pc_('\n');
}
int main(void){
  chk("filter_axis(1000,100,500)",   (uint32_t)filter_axis_decide(1000,100,500), 1088u);
  chk("controller_step(6400..5)",    controller_step_decide(6400,0,-12800,0,3200,0,5), 97419164u);
  chk("control_step(3000,50,40,0)",  rv_cs_call(3000,50,40,0), 2165333u);
  ps_("=== END ===\n");for(;;){}
}
