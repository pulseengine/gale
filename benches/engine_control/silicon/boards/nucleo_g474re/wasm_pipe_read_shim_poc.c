/*
 * Minimal wasm-side host of z_impl_k_pipe_read (the dissolved read path).
 *
 * Companion to wasm_pipe_write_shim_poc.c — same construction as
 * sem_give_shim.c: kernel APIs are externs (wasm imports), the verified Rust
 * decide (gale_k_pipe_read_decide) rides the same wasm module so loom inlines
 * the C↔Rust seam and synth emits ARM with it dissolved. Shipping the `pipe`
 * primitive needs BOTH directions clean; this is the read half of the gate-2
 * (shim-body-stays-in-registers) check. NOT --native-pointer-abi (u64 decide,
 * sem-class; not gated on synth#345).
 *
 * Faithful Zephyr v4.4.0 layout (identical to the write shim).
 */

#include <stdint.h>

struct k_thread;
struct k_spinlock { uint8_t lock_internal; };
typedef struct { uint32_t key; } k_spinlock_key_t;

struct ring_buf_index { uint32_t head; uint32_t tail; uint32_t base; };
struct ring_buf {
    uint8_t                *buffer;
    struct ring_buf_index   put;
    struct ring_buf_index   get;
    uint32_t                size;
};

struct k_pipe {
    uint32_t          waiting;
    struct ring_buf   buf;
    struct k_spinlock lock;
    void             *data_wq_head;     /* readers */
    void             *data_wq_tail;
    void             *space_wq_head;    /* writers */
    void             *space_wq_tail;
    uint8_t           flags;
};

extern k_spinlock_key_t  k_spin_lock(struct k_spinlock *);
extern void              k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern uint32_t          ring_buf_size_get(struct ring_buf *);
extern struct k_thread * z_unpend_first_thread(void *wait_q);
extern void              z_ready_thread(struct k_thread *);
extern void              arch_thread_return_value_set(struct k_thread *, uint32_t);
extern int               z_pend_curr(struct k_spinlock *, k_spinlock_key_t,
                                     void *wait_q, uint64_t timeout_ticks);
extern int               z_reschedule(struct k_spinlock *, k_spinlock_key_t);

extern uint64_t gale_k_pipe_read_decide(uint32_t used, uint32_t size,
                                        uint8_t flags, uint32_t request_len,
                                        uint32_t has_writer);

union gale_pipe_read_decision_u {
    uint64_t raw;
    struct {
        uint8_t  action;
        uint8_t  pad[3];
        uint32_t actual_bytes;
    } dec;
};

#define GALE_PIPE_ACTION_READ_OK              0
#define GALE_PIPE_ACTION_WAKE_WRITER          1
#define GALE_PIPE_ACTION_READ_PEND            2
#define GALE_PIPE_ACTION_READ_ERROR_ECANCELED 3
#define GALE_PIPE_ACTION_READ_ERROR_EPIPE     4

#define GALE_EPIPE     (-32)
#define GALE_ECANCELED (-125)

/* The hot path. THIS IS THE FFI SEAM. */
int z_impl_k_pipe_read(struct k_pipe *pipe, uint8_t *data, uint32_t len) {
    (void)data; /* kernel does the ring-buffer copy after the decision */
    k_spinlock_key_t key = k_spin_lock(&pipe->lock);

    uint32_t used = ring_buf_size_get(&pipe->buf);
    uint32_t size = pipe->buf.size;
    struct k_thread *writer = z_unpend_first_thread((void *)&pipe->space_wq_head);

    union gale_pipe_read_decision_u du;
    du.raw = gale_k_pipe_read_decide(used, size, pipe->flags, len,
                                     writer != (struct k_thread *)0 ? 1U : 0U);

    int ret;
    switch (du.dec.action) {
    case GALE_PIPE_ACTION_WAKE_WRITER:
        arch_thread_return_value_set(writer, du.dec.actual_bytes);
        z_ready_thread(writer);
        ret = (int)du.dec.actual_bytes;
        break;
    case GALE_PIPE_ACTION_READ_PEND:
        return z_pend_curr(&pipe->lock, key, (void *)&pipe->data_wq_head, len);
    case GALE_PIPE_ACTION_READ_ERROR_EPIPE:
        ret = GALE_EPIPE;
        break;
    case GALE_PIPE_ACTION_READ_ERROR_ECANCELED:
        ret = GALE_ECANCELED;
        break;
    case GALE_PIPE_ACTION_READ_OK:
    default:
        ret = (int)du.dec.actual_bytes;
        break;
    }

    z_reschedule(&pipe->lock, key);
    return ret;
}
