#include <stdint.h>
#include "control.h"
int32_t  filter_axis_decide(int32_t,int32_t,int32_t);
uint32_t control_step_decide(uint32_t,uint32_t,int32_t,uint32_t);
uint32_t flat_flight(struct flight_state*, const struct imu_sample*);
#define UART (*(volatile uint8_t*)0x10000000u)
static void pc_(char c){UART=(uint8_t)c;}
static void ps_(const char*s){while(*s)pc_(*s++);}
static void pd_(int32_t v){char b[12];int i=0,neg=0;uint32_t u;if(v<0){neg=1;u=-v;}else u=v;if(!u)b[i++]='0';while(u){b[i++]='0'+u%10;u/=10;}if(neg)pc_('-');while(i)pc_(b[--i]);}
static inline uint32_t rc(void){uint32_t c;__asm__ volatile("csrr %0, mcycle":"=r"(c));return c;}
static uint32_t ovh; static volatile int32_t sink;
static uint32_t meas_f(int32_t(*f)(int32_t,int32_t,int32_t),int32_t a,int32_t b,int32_t c){uint32_t best=~0u;for(int i=0;i<200;i++){uint32_t t0=rc();sink+=f(a,b,c);uint32_t t1=rc();uint32_t d=t1-t0;if(d<best)best=d;}return best-ovh;}
int main(void){
  ovh=~0u;for(int i=0;i<200;i++){uint32_t a=rc(),b=rc();uint32_t d=b-a;if(d<ovh)ovh=d;}
  ps_("RV-NATIVE rv32imac (qemu -icount proxy)\n");
  ps_("E,filter_axis,cyc=");pd_((int32_t)meas_f(filter_axis_decide,1000,100,500));ps_("\n");
  /* control_step (4 args; measure via direct calls) */
  {uint32_t best=~0u;for(int i=0;i<200;i++){uint32_t t0=rc();sink+=control_step_decide(3000,50,40,0);uint32_t t1=rc();uint32_t d=t1-t0;if(d<best)best=d;}
   ps_("E,control_step,cyc=");pd_((int32_t)(best-ovh));ps_(",chk=");pd_((int32_t)control_step_decide(3000,50,40,0));ps_(" (exp 2165333)\n");}
  /* flat_flight (composed; pointers) */
  {struct imu_sample s={0};s.gyro_x=100;s.gyro_y=-50;s.gyro_z=30;s.accel_x=300;s.accel_y=-200;
   uint32_t best=~0u;for(int i=0;i<200;i++){struct flight_state st={0};st.pitch_mdeg=1000;st.roll_mdeg=-500;st.yaw_mdeg=200;st.updates=7;uint32_t t0=rc();sink+=flat_flight(&st,&s);uint32_t t1=rc();uint32_t d=t1-t0;if(d<best)best=d;}
   struct flight_state st={0};st.pitch_mdeg=1000;st.roll_mdeg=-500;st.yaw_mdeg=200;st.updates=7;
   uint32_t r=flat_flight(&st,&s);ps_("E,flat_flight,cyc=");pd_((int32_t)(best-ovh));ps_(",chk=0x");for(int n=28;n>=0;n-=4){int d=(r>>n)&0xf;pc_(d<10?'0'+d:'a'+d-10);}ps_(" (exp 0x07fdf307)\n");}
  ps_("=== END ===\n");for(;;){}
}
