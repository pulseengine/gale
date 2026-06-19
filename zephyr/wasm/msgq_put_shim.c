/*
 * Minimal wasm-side host of z_impl_k_msgq_put — the message-queue analogue of
 * sem_give_shim.c / mutex_unlock_shim.c. Replicates gale_msgq.c's put hot path
 * with the Zephyr kernel APIs as externs (which become wasm imports), so the
 * shim itself compiles to wasm32-unknown-unknown without pulling in Zephyr
 * headers. This puts the C <-> Rust seam (gale_k_msgq_put_decide) INSIDE the
 * wasm bundle: wasm-ld merges it with gale-ffi.wasm, loom inlines through it,
 * synth produces ARM with the seam dissolved (no `bl gale_k_msgq_put_decide`).
 *
 * Surface: z_impl_k_msgq_put (put hot path). k_msgq_get / init / cleanup stay
 * native (gale_msgq.c). All four decided actions are handled faithfully:
 *   WAKE_READER  — copy bytes into the waiting reader's buffer + wake
 *   PUT_OK       — copy bytes into the ring buffer at the write slot
 *   RETURN_FULL  — non-blocking full: return d.ret (-ENOMSG)
 *   PUT_PEND     — blocking full: delegate to gale_w_msgq_pend (native
 *                  z_pend_curr; the wait queue / scheduling stay native C,
 *                  exactly as docs/wasm-module-distribution.md prescribes).
 *
 * SHIM DISCIPLINE (default config: !SMP / !SPIN_VALIDATE / !NONZERO_SPINLOCK):
 * struct k_spinlock is a ZERO-size empty struct, so we OMIT the embedded `lock`
 * field (modelling it as sized would shift every later field and corrupt the
 * struct on store) and use a file-scope static spinlock. In this config every
 * k_spin_lock degenerates to arch_irq_lock(), so the shim's static lock and
 * gale_msgq.c's per-object lock are the SAME critical section — which is what
 * makes the pend(here)/wake(native get) handshake on msgq->wait_q safe across
 * the wasm-put / native-get boundary. Same constraint the sem/mutex modules
 * assume; not valid for SMP builds (those keep the native z_impl_k_msgq_put).
 *
 * ABI: the third argument k_timeout_t is a struct { k_ticks_t ticks } (one
 * 8-byte member), passed in r2:r3 under AAPCS — identical to a bare int64_t.
 * The shim therefore takes int64_t timeout_ticks (K_NO_WAIT.ticks == 0,
 * K_FOREVER.ticks == -1) and reconstructs k_timeout_t inside gale_w_msgq_pend.
 *
 * Faithful Zephyr v4.4.0 struct k_msgq field offsets (0-byte spinlock omitted,
 * WAITQ_DUMB): wait_q@0(8) | msg_size@8 | max_msgs@12 | buffer_start@16 |
 * buffer_end@20 | read_ptr@24 | write_ptr@28 | used_msgs@32. (Z_DECL_POLL_EVENT
 * / flags come AFTER used_msgs, so CONFIG_POLL doesn't affect any touched field;
 * poll-event handling on PUT_OK stays native — not exercised by this surface.)
 */

#include <stdint.h>

/* Opaque k_thread — never deref'd in the shim; the kernel owns its layout. */
struct k_thread;
struct k_spinlock { uint8_t lock_internal; };   /* type only; never embedded (0-byte real) */
typedef struct { uint32_t key; } k_spinlock_key_t;

struct k_msgq {
	void     *wq_head;       /* @0  _wait_q_t wait_q (head,tail) */
	void     *wq_tail;       /* @4  */
	uint32_t  msg_size;      /* @8  (size_t on 32-bit) — 0-byte k_spinlock omitted */
	uint32_t  max_msgs;      /* @12 */
	char     *buffer_start;  /* @16 */
	char     *buffer_end;    /* @20 */
	char     *read_ptr;      /* @24 */
	char     *write_ptr;     /* @28 */
	uint32_t  used_msgs;     /* @32 */
};
static struct k_spinlock msgq_lock;

/* Kernel API externs -> wasm imports -> native `bl` after synth-emit (renamed
 * to the gale_w_* wrappers by build-wasm-dist.sh's objcopy pass). */
extern k_spinlock_key_t  k_spin_lock(struct k_spinlock *);
extern void              k_spin_unlock(struct k_spinlock *, k_spinlock_key_t);
extern struct k_thread * z_unpend_first_thread(void *wait_q);
extern void              z_ready_thread(struct k_thread *);
extern void              arch_thread_return_value_set(struct k_thread *, uint32_t);
extern int               z_reschedule(struct k_spinlock *, k_spinlock_key_t);
/* k_thread is opaque here: these out-of-line wrappers do what the native
 * z_impl_k_msgq_put reaches via struct fields / static helpers. */
extern void *            gale_w_thread_swap_data(struct k_thread *);     /* reader's dest buffer */
extern void              gale_w_memcpy(void *dst, const void *src, uint32_t n);
extern int               gale_w_msgq_pend(void *wait_q, k_spinlock_key_t key,
					  const void *data, int64_t timeout_ticks);

/* #[repr(C)] GaleMsgqPutDecision — 16 bytes, returned by value (sret). */
struct gale_msgq_put_decision { int32_t ret; uint8_t action; uint32_t new_write_idx; uint32_t new_used; };
extern struct gale_msgq_put_decision gale_k_msgq_put_decide(
	uint32_t write_idx, uint32_t used_msgs, uint32_t max_msgs,
	uint32_t has_waiter, uint32_t is_no_wait);

#define GALE_MSGQ_ACTION_PUT_OK      0
#define GALE_MSGQ_ACTION_WAKE_READER 1
#define GALE_MSGQ_ACTION_PUT_PEND    2
#define GALE_MSGQ_ACTION_RETURN_FULL 3

int z_impl_k_msgq_put(struct k_msgq *msgq, const void *data, int64_t timeout_ticks)
{
	uint32_t is_no_wait = (timeout_ticks == 0) ? 1U : 0U;   /* K_NO_WAIT.ticks == 0 */

	k_spinlock_key_t key = k_spin_lock(&msgq_lock);

	/* Extract: try to unpend first waiter (side effect: removes from queue).
	 * wait_q is k_msgq's first member, so &msgq == &msgq->wait_q. */
	struct k_thread *reader = z_unpend_first_thread((void *)msgq);

	uint32_t write_idx = (uint32_t)(msgq->write_ptr - msgq->buffer_start) / msgq->msg_size;

	struct gale_msgq_put_decision d = gale_k_msgq_put_decide(
		write_idx, msgq->used_msgs, msgq->max_msgs,
		reader != (struct k_thread *)0 ? 1U : 0U, is_no_wait);

	switch (d.action) {
	case GALE_MSGQ_ACTION_WAKE_READER:
		/* Receiver was waiting — copy the message into its buffer
		 * (reader stashed its dest in swap_data before pending in get). */
		gale_w_memcpy(gale_w_thread_swap_data(reader), data, msgq->msg_size);
		arch_thread_return_value_set(reader, 0U);
		z_ready_thread(reader);
		z_reschedule(&msgq_lock, key);
		return d.ret;
	case GALE_MSGQ_ACTION_PUT_OK:
		/* Space available — store at the write slot, advance ring. */
		gale_w_memcpy(msgq->write_ptr, data, msgq->msg_size);
		msgq->write_ptr = msgq->buffer_start + (uint64_t)d.new_write_idx * msgq->msg_size;
		msgq->used_msgs = d.new_used;
		k_spin_unlock(&msgq_lock, key);
		return d.ret;
	case GALE_MSGQ_ACTION_PUT_PEND:
		/* Queue full, blocking — pend current thread natively. The
		 * unpend above found no reader, so the wait_q is untouched. */
		return gale_w_msgq_pend((void *)msgq, key, data, timeout_ticks);
	case GALE_MSGQ_ACTION_RETURN_FULL:
	default:
		k_spin_unlock(&msgq_lock, key);
		return d.ret;   /* -ENOMSG */
	}
}
