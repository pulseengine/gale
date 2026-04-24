/*
 * Copyright (c) 2020 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale kheap — Extract/Decide/Apply pattern.
 *
 * This is kernel/kheap.c with alloc/free rewritten to use Rust
 * decision structs.  C extracts kernel state (spinlock, wait queue
 * side effects), Rust decides the action, C applies it.
 *
 * sys_heap internals, free-list management, coalescing, wait queues,
 * and tracing remain in Zephyr.
 *
 * Verified operations (Verus proofs):
 *   gale_k_kheap_alloc_decide — KH2 (alloc), KH3 (full), KH6 (no overflow)
 *   gale_k_kheap_free_decide  — KH4 (free), KH5 (conservation)
 */

#include <zephyr/kernel.h>
#include <zephyr/init.h>
#include <zephyr/linker/linker-defs.h>
#include <zephyr/sys/iterable_sections.h>
/* private kernel APIs */
#include <ksched.h>
#include <wait_q.h>

#include "gale_kheap.h"

int k_heap_array_get(struct k_heap **heap)
{
	int num;

	/* Pointer to the start of the heap array */
	STRUCT_SECTION_GET(k_heap, 0, heap);
	/* Number of statically defined heaps */
	STRUCT_SECTION_COUNT(k_heap, &num);
	return num;
}

void k_heap_init(struct k_heap *heap, void *mem, size_t bytes)
{
	z_waitq_init(&heap->wait_q);
	heap->lock = (struct k_spinlock) {};
	sys_heap_init(&heap->heap, mem, bytes);

	SYS_PORT_TRACING_OBJ_INIT(k_heap, heap);
}

static int statics_init(void)
{
	STRUCT_SECTION_FOREACH(k_heap, heap) {
#if defined(CONFIG_DEMAND_PAGING) && !defined(CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT)
		/* Some heaps may not present at boot, so we need to wait for
		 * paging mechanism to be initialized before we can initialize
		 * each heap.
		 */
		extern bool z_sys_post_kernel;
		bool do_clear = z_sys_post_kernel;

		/* During pre-kernel init, z_sys_post_kernel == false,
		 * initialize if within pinned region. Otherwise skip.
		 * In post-kernel init, z_sys_post_kernel == true, skip those in
		 * pinned region as they have already been initialized and
		 * possibly already in use. Otherwise initialize.
		 */
		if (lnkr_is_pinned((uint8_t *)heap) &&
		    lnkr_is_pinned((uint8_t *)&heap->wait_q) &&
		    lnkr_is_region_pinned((uint8_t *)heap->heap.init_mem,
					  heap->heap.init_bytes)) {
			do_clear = !do_clear;
		}

		if (do_clear)
#endif /* CONFIG_DEMAND_PAGING && !CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT */
		{
			k_heap_init(heap, heap->heap.init_mem, heap->heap.init_bytes);
		}
	}
	return 0;
}

SYS_INIT_NAMED(statics_init_pre, statics_init, PRE_KERNEL_1, CONFIG_KERNEL_INIT_PRIORITY_OBJECTS);

#if defined(CONFIG_DEMAND_PAGING) && !defined(CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT)
/* Need to wait for paging mechanism to be initialized before
 * heaps that are not in pinned sections can be initialized.
 */
SYS_INIT_NAMED(statics_init_post, statics_init, POST_KERNEL, 0);
#endif /* CONFIG_DEMAND_PAGING && !CONFIG_LINKER_GENERIC_SECTIONS_PRESENT_AT_BOOT */

typedef void * (sys_heap_allocator_t)(struct sys_heap *heap, size_t align, size_t bytes);

static void *z_heap_alloc_helper(struct k_heap *heap, size_t align, size_t bytes,
				 k_timeout_t timeout,
				 sys_heap_allocator_t *sys_heap_allocator)
{
	k_timepoint_t end = sys_timepoint_calc(timeout);
	void *ret = NULL;

	k_spinlock_key_t key = k_spin_lock(&heap->lock);

	__ASSERT(!arch_is_in_isr() || K_TIMEOUT_EQ(timeout, K_NO_WAIT), "");

	bool blocked_alloc = false;

	while (ret == NULL) {
		/* Extract: attempt allocation from sys_heap */
		ret = sys_heap_allocator(&heap->heap, align, bytes);

		/* Extract: determine wait policy */
		uint32_t is_no_wait = (!IS_ENABLED(CONFIG_MULTITHREADING) ||
				       (ret != NULL) ||
				       K_TIMEOUT_EQ(timeout, K_NO_WAIT)) ? 1U : 0U;

		/* Decide: Rust determines action */
		struct gale_kheap_alloc_decision d = gale_k_kheap_alloc_decide(
			ret != NULL ? 1U : 0U, is_no_wait);

		/* Apply: execute Rust's decision */
		if (d.action == GALE_KHEAP_ACTION_RETURN_PTR ||
		    d.action == GALE_KHEAP_ACTION_RETURN_NULL) {
			break;
		}

		/* PEND: wait for a free to wake us */
		if (!blocked_alloc) {
			blocked_alloc = true;

			SYS_PORT_TRACING_OBJ_FUNC_BLOCKING(k_heap, alloc_helper, heap, timeout);
		} else {
			/**
			 * @todo	Trace attempt to avoid empty trace segments
			 */
		}

		timeout = sys_timepoint_timeout(end);
		(void) z_pend_curr(&heap->lock, key, &heap->wait_q, timeout);
		key = k_spin_lock(&heap->lock);
	}

	k_spin_unlock(&heap->lock, key);
	return ret;
}

void *k_heap_alloc(struct k_heap *heap, size_t bytes, k_timeout_t timeout)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_heap, alloc, heap, timeout);

	void *ret = z_heap_alloc_helper(heap, 0, bytes, timeout,
					sys_heap_noalign_alloc);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_heap, alloc, heap, timeout, ret);

	return ret;
}

void *k_heap_aligned_alloc(struct k_heap *heap, size_t align, size_t bytes,
			k_timeout_t timeout)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_heap, aligned_alloc, heap, timeout);

	/* A power of 2 as well as 0 is OK */
	__ASSERT((align & (align - 1)) == 0,
		 "align must be a power of 2");

	void *ret = z_heap_alloc_helper(heap, align, bytes, timeout,
					sys_heap_aligned_alloc);

	/*
	 * modules/debug/percepio/TraceRecorder/kernelports/Zephyr/include/tracing_tracerecorder.h
	 * contains a concealed non-parameterized direct reference to a local
	 * variable through the SYS_PORT_TRACING_OBJ_FUNC_EXIT macro below
	 * that is no longer in scope. Provide a dummy stub for compilation
	 * to still succeed until that module's layering violation is fixed.
	 */
	bool blocked_alloc = false; ARG_UNUSED(blocked_alloc);

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_heap, aligned_alloc, heap, timeout, ret);

	return ret;
}

void *k_heap_calloc(struct k_heap *heap, size_t num, size_t size, k_timeout_t timeout)
{
	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_heap, calloc, heap, timeout);

	void *ret = NULL;
	size_t bounds = 0U;

	if (!size_mul_overflow(num, size, &bounds)) {
		ret = k_heap_alloc(heap, bounds, timeout);
	}
	if (ret != NULL) {
		(void)memset(ret, 0, bounds);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_heap, calloc, heap, timeout, ret);

	return ret;
}

void *k_heap_realloc(struct k_heap *heap, void *ptr, size_t bytes, k_timeout_t timeout)
{
	k_timepoint_t end = sys_timepoint_calc(timeout);
	void *ret = NULL;

	k_spinlock_key_t key = k_spin_lock(&heap->lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_heap, realloc, heap, ptr, bytes, timeout);

	__ASSERT(!arch_is_in_isr() || K_TIMEOUT_EQ(timeout, K_NO_WAIT), "");

	while (ret == NULL) {
		ret = sys_heap_realloc(&heap->heap, ptr, bytes);

		if (!IS_ENABLED(CONFIG_MULTITHREADING) ||
		    (ret != NULL) || K_TIMEOUT_EQ(timeout, K_NO_WAIT)) {
			break;
		}

		timeout = sys_timepoint_timeout(end);
		(void) z_pend_curr(&heap->lock, key, &heap->wait_q, timeout);
		key = k_spin_lock(&heap->lock);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_heap, realloc, heap, ptr, bytes, timeout, ret);

	k_spin_unlock(&heap->lock, key);
	return ret;
}

void k_heap_free(struct k_heap *heap, void *mem)
{
	k_spinlock_key_t key = k_spin_lock(&heap->lock);

	/* Extract: free via sys_heap */
	sys_heap_free(&heap->heap, mem);

	SYS_PORT_TRACING_OBJ_FUNC(k_heap, free, heap);

	/* Extract: check for waiters */
	uint32_t has_waiters = 0U;

	if (IS_ENABLED(CONFIG_MULTITHREADING)) {
		has_waiters = (z_unpend_all(&heap->wait_q) != 0) ? 1U : 0U;
	}

	/* Decide: Rust determines action */
	struct gale_kheap_free_decision d = gale_k_kheap_free_decide(has_waiters);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_KHEAP_ACTION_FREE_AND_RESCHEDULE) {
		z_reschedule(&heap->lock, key);
	} else {
		k_spin_unlock(&heap->lock, key);
	}
}

/*
 * Variant of k_heap_free() for callers that already hold
 * _sched_spinlock. Uses z_unpend_all_locked() so the scheduler lock
 * is not recursively acquired.
 *
 * Any woken threads are readied but not rescheduled. The caller is
 * responsible for ensuring a reschedule happens after releasing
 * _sched_spinlock.
 *
 * Matches upstream Zephyr fix 9cef0da05c3. The Gale decision layer
 * is skipped here because the caller owns the reschedule — there is
 * no decision to make at this call site.
 */
void k_heap_free_sched_locked(struct k_heap *heap, void *mem)
{
	k_spinlock_key_t key = k_spin_lock(&heap->lock);

	sys_heap_free(&heap->heap, mem);

	SYS_PORT_TRACING_OBJ_FUNC(k_heap, free, heap);
	k_spin_unlock(&heap->lock, key);

	if (IS_ENABLED(CONFIG_MULTITHREADING)) {
		z_unpend_all_locked(&heap->wait_q);
	}
}
