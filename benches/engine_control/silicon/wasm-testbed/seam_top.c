#include <stdint.h>
struct S { int32_t a; int32_t b; };
int32_t rd(struct S *s); void wr(struct S *s, int32_t v);
__attribute__((export_name("seam")))
int32_t seam(struct S *s, int32_t v){ wr(s, v); return rd(s); }   /* rd: single call site */
