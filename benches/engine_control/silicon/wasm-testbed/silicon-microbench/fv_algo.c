#include <stdint.h>
__attribute__((export_name("full")))  int32_t full(int32_t p,int32_t g,int32_t a){ int32_t t=p+g; return (t*980+a*20)/1000; }
__attribute__((export_name("nodiv"))) int32_t nodiv(int32_t p,int32_t g,int32_t a){ int32_t t=p+g; return t*980+a*20; }
__attribute__((export_name("div")))   int32_t divv(int32_t x){ return x/1000; }
