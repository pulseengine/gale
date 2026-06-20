#include <stdint.h>
#define RB 20
#define LB 20
extern const int8_t spark_advance_table[RB][LB];
extern const uint16_t fuel_duration_table[RB][LB];
static inline uint8_t rb_(uint32_t r){uint32_t b=r/500;if(b>=RB)b=RB-1;return b;}
static inline uint8_t lb_(uint16_t l){uint16_t b=l/5;if(b>=LB)b=LB-1;return b;}
static inline uint32_t enr_(int16_t c){if(c>=80)return 0;if(c<=0)return 300;return (uint32_t)((80-c)*300/80);}
uint32_t control_step_native(uint32_t rpm,uint32_t load,int32_t cool,uint32_t kn){
  uint8_t r=rb_(rpm),l=lb_((uint16_t)load);int16_t a=(int16_t)spark_advance_table[r][l];a-=(int16_t)kn;if(a<0)a=0;
  uint32_t bf=fuel_duration_table[r][l],e=enr_((int16_t)cool),c=bf+(bf*e/1000U);if(c>65535)c=65535;
  return ((uint32_t)(uint16_t)a<<16)|(uint16_t)c;}
