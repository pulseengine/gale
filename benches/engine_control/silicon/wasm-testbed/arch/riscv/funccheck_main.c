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
  /* Multi-vector coverage: each function exercised across the input edges that
   * per-axis const-CSE dedup is most likely to break (saturation ±127, signed-div
   * negatives, table cells). Expecteds are run_testbed.sh's verified ground truth
   * (native == wasmtime == on-silicon). A const-CSE miscompile that reuses a
   * clobbered constant register typically only shows on a subset of inputs, so
   * single-vector coverage would miss it — hence the spread. */

  /* filter_axis: signed mul + signed div by 1000. Zero, positive, negative. */
  chk("filter_axis(0,0,0)",         (uint32_t)filter_axis_decide(0,0,0),          0u);
  chk("filter_axis(1000,100,500)",  (uint32_t)filter_axis_decide(1000,100,500),   1088u);
  chk("filter_axis(-2000,50,-300)", (uint32_t)filter_axis_decide(-2000,50,-300),  (uint32_t)(int32_t)-1917);

  /* controller_step: SAR + ±127 saturation + 8-bit pack. Zero, the packed
   * mid-range vector, and an over-range vector that drives all 3 axes past the
   * clamp (each axis re-materializes ±127 — the prime const-CSE dedup site). */
  chk("controller_step(0..0)",      controller_step_decide(0,0,0,0,0,0,0),                 0u);
  chk("controller_step(6400..5)",   controller_step_decide(6400,0,-12800,0,3200,0,5),      97419164u);
  chk("controller_step(satclamp)",  controller_step_decide(99999,99999,-99999,-99999,99999,-99999,255),
      /* a=-127 e=+127 r=-127, updates=255 -> 0xFF817F81 (wasmtime-verified) */ 0xFF817F81u);

  /* control_step: 2 unsigned-const divides + 2 tables (via s11 tramp). 4 cells. */
  chk("control_step(3000,50,90,0)", rv_cs_call(3000,50,90,0), 2164988u);
  chk("control_step(3000,50,40,0)", rv_cs_call(3000,50,40,0), 2165333u);
  chk("control_step(3000,50,0,0)",  rv_cs_call(3000,50,0,0),  2165678u);
  chk("control_step(6000,80,40,3)", rv_cs_call(6000,80,40,3), 2230501u);
  ps_("=== END ===\n");for(;;){}
}
