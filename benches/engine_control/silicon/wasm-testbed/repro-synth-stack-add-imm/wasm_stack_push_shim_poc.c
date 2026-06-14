/*
 * Minimal wasm-side host of z_impl_k_stack_push — a 2nd struct-return primitive
 * (after mutex) to confirm synth#345 (.bss + PC-relative) generalizes the whole
 * struct-return decide family, and to de-risk the gale#63 component epic (which
 * will dissolve many struct-return primitives via the kernel-primitives WIT world).
 *
 * gale_k_stack_push_decide returns GaleStackPushDecision (12-byte #[repr(C)]:
 * ret i32 / new_count u32 / action u8) BY VALUE → sret → linmem-frame path, the
 * same shape that USAGE-FAULTed mutex pre-#345. With v0.11.43 the expected shape
 * is link-survivable: .data bounded (~4), .bss NOBITS, MOVW/MOVT_ABS=0, ABS32 pool
 * words — the "mutex shape", NOT the old 64 KB .data blob. Built WITH
 * --native-pointer-abi (host k_stack* stays base=0 via the r11=0 trampoline).
 *
 * Faithful Zephyr v4.4.0 struct k_stack: wait_q first (so &stack == &stack->wait_q).
 */

#include <stdint.h>

typedef uintptr_t stack_data_t;
struct k_thread;
struct k_spinlock { uint8_t lock_internal; };
typedef struct { uint32_t key; } k_spinlock_key_t;

struct k_stack {
    void         *wq_head;   /* _wait_q_t wait_q (head,tail) */
    void         *wq_tail;
    struct k_spinlock lock;
    stack_data_t *base;
    stack_data_t *next;
    stack_data_t *top;
    uint8_t       flags;
};

extern k_spinlock_key_t  k_spin_lock(struct k_spinlock *);
extern void              k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern struct k_thread * z_unpend_first_thread(void *wait_q);
extern void              z_ready_thread(struct k_thread *);
extern void              z_thread_return_value_set_with_data(struct k_thread *, unsigned int, void *);
extern int               z_reschedule(struct k_spinlock *, k_spinlock_key_t);

/* #[repr(C)] GaleStackPushDecision — 12 bytes, returned by value (sret). */
struct gale_stack_push_decision { int32_t ret; uint32_t new_count; uint8_t action; };
extern struct gale_stack_push_decision gale_k_stack_push_decide(
    uint32_t count, uint32_t capacity, uint32_t has_waiter);

#define GALE_STACK_PUSH_STORE 0
#define GALE_STACK_PUSH_WAKE  1
#define GALE_STACK_PUSH_FULL  2

int z_impl_k_stack_push(struct k_stack *stack, stack_data_t data)
{
    k_spinlock_key_t key = k_spin_lock(&stack->lock);

    uint32_t capacity = (uint32_t)(stack->top - stack->base);
    uint32_t count    = (uint32_t)(stack->next - stack->base);
    struct k_thread *waiter = z_unpend_first_thread((void *)stack); /* wait_q is first member */

    struct gale_stack_push_decision d =
        gale_k_stack_push_decide(count, capacity, waiter != (struct k_thread *)0 ? 1U : 0U);

    switch (d.action) {
    case GALE_STACK_PUSH_WAKE:
        z_thread_return_value_set_with_data(waiter, 0U, (void *)data);
        z_ready_thread(waiter);
        z_reschedule(&stack->lock, key);
        return d.ret;
    case GALE_STACK_PUSH_FULL:
        k_spin_unlock(&stack->lock, key);
        return d.ret; /* -ENOMEM */
    case GALE_STACK_PUSH_STORE:
    default:
        *(stack->next) = data;
        stack->next++;
        k_spin_unlock(&stack->lock, key);
        return d.ret;
    }
}
