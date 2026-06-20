#include <stdint.h>
struct S { int32_t a; int32_t b; };
int32_t rd(struct S *s){ int32_t x = s->a + s->b; if (x>127) x=127; if (x<-127) x=-127; return x; }
void    wr(struct S *s, int32_t v){ s->a = v; s->b = v >> 1; }
