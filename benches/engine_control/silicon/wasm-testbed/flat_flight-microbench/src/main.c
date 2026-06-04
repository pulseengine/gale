/* flat_flight (composed flight algo) ARM-silicon microbench — the Track-A frozen target.
 * synth flat_flight takes 2 wasm linmem ptrs (st,s) + uses a linmem stack; called via
 * ff_tramp.S which sets r11=&wasm_linmem (fp=linmem-base model, 0 statics so this suffices).
 * native = same logic on real pointers. DWT min/200, overhead-subtracted. */
#include <zephyr/kernel.h>
#include <stdint.h>
struct imu_sample { int16_t gyro_x,gyro_y,gyro_z,accel_x,accel_y,accel_z; uint32_t algo_cycles; uint16_t seq; uint8_t step,_pad; };
struct flight_state { int32_t pitch_mdeg,roll_mdeg,yaw_mdeg,pitch_rate,roll_rate,yaw_rate; uint32_t updates; };
#define DEMCR (*(volatile uint32_t*)0xE000EDFCu)
#define DWT_CTRL (*(volatile uint32_t*)0xE0001000u)
#define DWT_CYCCNT (*(volatile uint32_t*)0xE0001004u)

/* the wasm linear memory (64KB; stack lives at top 65536, structs placed low). r11 base. */
uint8_t wasm_linmem[65536] __attribute__((aligned(8)));
extern uint32_t synth_flat_flight_buf(uint32_t st_off, uint32_t s_off);  /* tramp -> synth body */

__attribute__((noinline))
uint32_t n_flat_flight(struct flight_state *st, const struct imu_sample *s){
	int32_t ap=s->accel_x, ar=s->accel_y;
	int32_t gp=st->pitch_mdeg+s->gyro_y, gr=st->roll_mdeg+s->gyro_x, gy=st->yaw_mdeg+s->gyro_z;
	st->pitch_mdeg=(gp*980+ap*20)/1000; st->roll_mdeg=(gr*980+ar*20)/1000; st->yaw_mdeg=gy;
	st->pitch_rate=s->gyro_y; st->roll_rate=s->gyro_x; st->yaw_rate=s->gyro_z;
	int32_t ail=-(st->roll_mdeg>>6)-(st->roll_rate>>7), ele=-(st->pitch_mdeg>>6)-(st->pitch_rate>>7), rud=-(st->yaw_mdeg>>6)-(st->yaw_rate>>7);
	if(ail>127)ail=127; if(ail<-127)ail=-127; if(ele>127)ele=127; if(ele<-127)ele=-127; if(rud>127)rud=127; if(rud<-127)rud=-127;
	return ((uint32_t)(uint8_t)(int8_t)ail)|((uint32_t)(uint8_t)(int8_t)ele<<8)|((uint32_t)(uint8_t)(int8_t)rud<<16)|((uint32_t)(st->updates&0xFF)<<24);
}
static inline void dwt_init(void){ DEMCR|=(1u<<24); DWT_CYCCNT=0; DWT_CTRL|=1u; }
static void init_st(struct flight_state*st){ st->pitch_mdeg=1000;st->roll_mdeg=-500;st->yaw_mdeg=200;st->pitch_rate=0;st->roll_rate=0;st->yaw_rate=0;st->updates=7; }
static void init_s(struct imu_sample*s){ *s=(struct imu_sample){0}; s->gyro_x=100;s->gyro_y=-50;s->gyro_z=30;s->accel_x=300;s->accel_y=-200; }

int main(void){
	dwt_init();
	/* place st at offset 0, s at offset 32 in linmem; native uses stack copies */
	struct flight_state *lst=(struct flight_state*)&wasm_linmem[0];
	struct imu_sample   *ls =(struct imu_sample*)&wasm_linmem[32];
	struct flight_state nst; struct imu_sample ns;
	init_st(lst); init_s(ls); init_st(&nst); init_s(&ns);
	uint32_t vs=synth_flat_flight_buf(0,32), vn=n_flat_flight(&nst,&ns);
	printk("SELFCHECK synth=0x%08x native=0x%08x exp=0x07fdf307 %s\n", vs, vn, (vs==vn&&vs==0x07fdf307u)?"OK":"BAD");
	uint32_t ovh=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ uint32_t a=DWT_CYCCNT,b=DWT_CYCCNT; uint32_t d=b-a; if(d<ovh)ovh=d; }
	volatile uint32_t sink=0;
	uint32_t bs=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ init_st(lst); uint32_t t0=DWT_CYCCNT; sink+=synth_flat_flight_buf(0,32); uint32_t t1=DWT_CYCCNT; uint32_t d=t1-t0; if(d<bs)bs=d; }
	uint32_t bn=0xFFFFFFFFu;
	for(int i=0;i<200;i++){ init_st(&nst); uint32_t t0=DWT_CYCCNT; sink+=n_flat_flight(&nst,&ns); uint32_t t1=DWT_CYCCNT; uint32_t d=t1-t0; if(d<bn)bn=d; }
	printk("E,flat_flight,synth=%u,native=%u (ovh=%u)\n", bs-ovh, bn-ovh, ovh);
	printk("SINK=%u\n",sink);
	printk("=== END ===\n");
	return 0;
}
