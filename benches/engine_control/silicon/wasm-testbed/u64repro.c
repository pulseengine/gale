#include <stdint.h>
__attribute__((noinline)) uint64_t make(uint32_t a, uint32_t b) {
    return (uint64_t)(a + b + 1) << 32 | 1u;
}
__attribute__((export_name("check"))) uint32_t check(uint32_t a, uint32_t b) {
    uint64_t r = make(a, b);
    uint32_t action = (uint32_t)(r & 0xFFu);     /* expect 1 */
    uint32_t val    = (uint32_t)(r >> 32);       /* expect a+b+1 */
    return action == 1u ? val : 0xDEADu;
}
