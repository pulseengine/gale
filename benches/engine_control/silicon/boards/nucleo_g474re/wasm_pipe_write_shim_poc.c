/*
 * Minimal wasm-side host of z_impl_k_pipe_write (the dissolved write path).
 *
 * Same construction as sem_give_shim.c: kernel APIs are externs (become wasm
 * imports), the verified Rust decide (gale_k_pipe_write_decide) is in the same
 * wasm module after wasm-ld merge, so loom inlines through the C↔Rust seam and
 * synth emits ARM with the seam dissolved. Pipe is the second u64-shaped
 * primitive (after sem): its decide returns a packed u64 (no sret), so unlike
 * the 51 struct-return decides it does NOT need --native-pointer-abi and is not
 * gated on synth#345. The open question this shim answers is gate-2: does the
 * (more complex, 5-action) write body stay in registers, or spill to a wasm
 * linmem stack frame? sem's body is simple enough to stay in registers; this
 * is the empirical check for pipe.
 *
 * Faithful Zephyr v4.4.0 layout: k_pipe { size_t waiting; struct ring_buf buf;
 * k_spinlock lock; _wait_q_t data; _wait_q_t space; uint8_t flags; }.
 */

#include <stdint.h>

struct k_thread;
struct k_spinlock { uint8_t lock_internal; };
typedef struct { uint32_t key; } k_spinlock_key_t;

/* Faithful struct ring_buf (v4.4.0): buffer ptr + two 3-word indices + size. */
struct ring_buf_index { uint32_t head; uint32_t tail; uint32_t base; };
struct ring_buf {
    uint8_t                *buffer;
    struct ring_buf_index   put;
    struct ring_buf_index   get;
    uint32_t                size;
};

/* Faithful struct k_pipe (v4.4.0, CONFIG_POLL=n / WAITQ_SCALABLE=n:
 * _wait_q_t == sys_dlist_t == {head, tail}). */
struct k_pipe {
    uint32_t          waiting;          /* size_t on 32-bit ARM */
    struct ring_buf   buf;
    struct k_spinlock lock;
    void             *data_wq_head;     /* _wait_q_t data  (readers) */
    void             *data_wq_tail;
    void             *space_wq_head;    /* _wait_q_t space (writers) */
    void             *space_wq_tail;
    uint8_t           flags;
};

/* Kernel API externs — wasm imports at link time, native bl at synth-emit. */
extern k_spinlock_key_t  k_spin_lock(struct k_spinlock *);
extern void              k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern uint32_t          ring_buf_size_get(struct ring_buf *);
extern struct k_thread * z_unpend_first_thread(void *wait_q);
extern void              z_ready_thread(struct k_thread *);
extern void              arch_thread_return_value_set(struct k_thread *, uint32_t);
extern int               z_pend_curr(struct k_spinlock *, k_spinlock_key_t,
                                     void *wait_q, uint64_t timeout_ticks);
extern int               z_reschedule(struct k_spinlock *, k_spinlock_key_t);

/* The verified Rust decide — same wasm module after wasm-ld merge. */
extern uint64_t gale_k_pipe_write_decide(uint32_t used, uint32_t size,
                                         uint8_t flags, uint32_t request_len,
                                         uint32_t has_reader);

/* AAPCS-packed decision: matches #[repr(C)] GalePipeWriteDecision (8 bytes). */
union gale_pipe_write_decision_u {
    uint64_t raw;
    struct {
        uint8_t  action;
        uint8_t  pad[3];
        uint32_t actual_bytes;
    } dec;
};

#define GALE_PIPE_ACTION_WRITE_OK              0
#define GALE_PIPE_ACTION_WAKE_READER           1
#define GALE_PIPE_ACTION_WRITE_PEND            2
#define GALE_PIPE_ACTION_WRITE_ERROR_ECANCELED 3
#define GALE_PIPE_ACTION_WRITE_ERROR_EPIPE     4

#define GALE_EPIPE     (-32)
#define GALE_ECANCELED (-125)

/* The hot path. THIS IS THE FFI SEAM. */
int z_impl_k_pipe_write(struct k_pipe *pipe, const uint8_t *data, uint32_t len) {
    (void)data; /* kernel does the ring-buffer copy after the decision */
    k_spinlock_key_t key = k_spin_lock(&pipe->lock);

    /* Extract: current fill + capacity + a waiting reader (kernel side effect). */
    uint32_t used = ring_buf_size_get(&pipe->buf);
    uint32_t size = pipe->buf.size;
    struct k_thread *reader = z_unpend_first_thread((void *)&pipe->data_wq_head);

    /* Decide: Rust packs the action + writable byte count into a u64. */
    union gale_pipe_write_decision_u du;
    du.raw = gale_k_pipe_write_decide(used, size, pipe->flags, len,
                                      reader != (struct k_thread *)0 ? 1U : 0U);

    /* Apply. */
    int ret;
    switch (du.dec.action) {
    case GALE_PIPE_ACTION_WAKE_READER:
        arch_thread_return_value_set(reader, du.dec.actual_bytes);
        z_ready_thread(reader);
        ret = (int)du.dec.actual_bytes;
        break;
    case GALE_PIPE_ACTION_WRITE_PEND:
        /* z_pend_curr releases the lock and blocks the caller. */
        return z_pend_curr(&pipe->lock, key, (void *)&pipe->space_wq_head, len);
    case GALE_PIPE_ACTION_WRITE_ERROR_EPIPE:
        ret = GALE_EPIPE;
        break;
    case GALE_PIPE_ACTION_WRITE_ERROR_ECANCELED:
        ret = GALE_ECANCELED;
        break;
    case GALE_PIPE_ACTION_WRITE_OK:
    default:
        ret = (int)du.dec.actual_bytes;
        break;
    }

    z_reschedule(&pipe->lock, key);
    return ret;
}
