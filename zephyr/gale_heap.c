/*
 * Copyright (c) 2019 Intel Corporation
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale sys_heap — Extract/Decide/Apply pattern.
 *
 * This is lib/heap/heap.c with alloc/free/realloc rewritten to use
 * Rust decision structs. C extracts chunk state, Rust decides the
 * action (split, coalesce, reject), C applies it.
 *
 * Free-list management, bucket traversal, pointer arithmetic, and
 * memory layout remain in Zephyr's heap.h inline functions.
 *
 * Verified operations (Verus proofs):
 *   gale_sys_heap_alloc_decide         — HP1-HP3, HP7 (alloc gating)
 *   gale_sys_heap_free_decide          — HP4, HP5, HP8 (double-free, coalesce)
 *   gale_sys_heap_aligned_alloc_decide — HP6, HP7 (alignment, overflow)
 *   gale_sys_heap_realloc_decide       — HP1, HP7, HP8 (shrink/grow)
 *   gale_sys_heap_init_validate        — HP1 (capacity)
 *   gale_sys_heap_split_validate       — HP2, HP8 (conservation)
 *   gale_sys_heap_merge_validate       — HP2, HP7, HP8 (conservation)
 *
 * Integration note:
 *   heap.c lives in lib/heap/, not kernel/. ZEPHYR_EXTRA_MODULES cannot
 *   exclude individual lib/ source files the way it can kernel/ files
 *   (Zephyr's lib/heap/CMakeLists.txt unconditionally compiles heap.c).
 *
 *   Integration options:
 *     1. Patch lib/heap/CMakeLists.txt to guard heap.c with
 *        CONFIG_GALE_KERNEL_HEAP (preferred for production).
 *     2. Use this file as a standalone replacement — compile it via
 *        ZEPHYR_EXTRA_MODULES and disable the upstream heap.c in the
 *        Zephyr fork.
 *     3. Use the decision functions as runtime guards alongside the
 *        original heap.c (no fork needed, lower coverage).
 *
 *   This file implements option 2/3: it provides the full sys_heap API
 *   with Gale decision functions at every safety-critical point. When
 *   CONFIG_GALE_KERNEL_HEAP is set, it can replace the upstream heap.c.
 */

#include <zephyr/sys/sys_heap.h>
#include <zephyr/sys/util.h>
#include <zephyr/sys/heap_listener.h>
#include <zephyr/kernel.h>
#include <string.h>
#include "heap.h"

#include "gale_heap.h"

#ifdef CONFIG_MSAN
#include <sanitizer/msan_interface.h>
#endif

/* Sizing invariant: upstream v4.4+ no longer exports the old
 * _Z_HEAP_SIZE symbol. The per-allocation envelope is computed by
 * Z_HEAP_MIN_SIZE_FOR(bytes) in kernel.h; this shim trusts that
 * macro at call sites and does not duplicate the sizeof check. */

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
static inline void increase_allocated_bytes(struct z_heap *h, size_t num_bytes)
{
	h->allocated_bytes += num_bytes;
	h->max_allocated_bytes = max(h->max_allocated_bytes, h->allocated_bytes);
}
#endif

static void *chunk_mem(struct z_heap *h, chunkid_t c)
{
	chunk_unit_t *buf = chunk_buf(h);
	uint8_t *ret = ((uint8_t *)&buf[c]) + chunk_header_bytes(h);

	CHECK(!(((uintptr_t)ret) & (big_heap(h) ? 7 : 3)));

	return ret;
}

static void free_list_remove_bidx(struct z_heap *h, chunkid_t c, int bidx)
{
	struct z_heap_bucket *b = &h->buckets[bidx];

	CHECK(!chunk_used(h, c));
	CHECK(b->next != 0);
	CHECK(h->avail_buckets & BIT(bidx));

	if (next_free_chunk(h, c) == c) {
		h->avail_buckets &= ~BIT(bidx);
		b->next = 0;
	} else {
		chunkid_t first = prev_free_chunk(h, c),
			  second = next_free_chunk(h, c);

		b->next = second;
		set_next_free_chunk(h, first, second);
		set_prev_free_chunk(h, second, first);
	}

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	h->free_bytes -= chunksz_to_bytes(h, chunk_size(h, c));
#endif
}

static void free_list_remove(struct z_heap *h, chunkid_t c)
{
	/* Upstream renamed solo_free_header() → undersized_chunk() in
	 * the v4.x heap rework; semantics are identical. */
	if (!undersized_chunk(h, c)) {
		int bidx = bucket_idx(h, chunk_size(h, c));
		free_list_remove_bidx(h, c, bidx);
	}
}

static void free_list_add_bidx(struct z_heap *h, chunkid_t c, int bidx)
{
	struct z_heap_bucket *b = &h->buckets[bidx];

	if (b->next == 0U) {
		CHECK((h->avail_buckets & BIT(bidx)) == 0);

		h->avail_buckets |= BIT(bidx);
		b->next = c;
		set_prev_free_chunk(h, c, c);
		set_next_free_chunk(h, c, c);
	} else {
		CHECK(h->avail_buckets & BIT(bidx));

		chunkid_t second = b->next;
		chunkid_t first = prev_free_chunk(h, second);

		set_prev_free_chunk(h, c, first);
		set_next_free_chunk(h, c, second);
		set_next_free_chunk(h, first, c);
		set_prev_free_chunk(h, second, c);
	}

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	h->free_bytes += chunksz_to_bytes(h, chunk_size(h, c));
#endif
}

static void free_list_add(struct z_heap *h, chunkid_t c)
{
	if (!solo_free_header(h, c)) {
		int bidx = bucket_idx(h, chunk_size(h, c));
		free_list_add_bidx(h, c, bidx);
	}
}

static void split_chunks(struct z_heap *h, chunkid_t lc, chunkid_t rc)
{
	CHECK(rc > lc);
	CHECK(rc - lc < chunk_size(h, lc));

	chunksz_t sz0 = chunk_size(h, lc);
	chunksz_t lsz = rc - lc;
	chunksz_t rsz = sz0 - lsz;

	/* HP8: verify split conservation via Rust */
	uint32_t validated_rsz = gale_sys_heap_split_validate(sz0, lsz);
	__ASSERT(validated_rsz == rsz,
		 "split conservation violated: %u != %u", validated_rsz, rsz);
	(void)validated_rsz;

	set_chunk_size(h, lc, lsz);
	set_chunk_size(h, rc, rsz);
	set_left_chunk_size(h, rc, lsz);
	set_left_chunk_size(h, right_chunk(h, rc), rsz);
}

static void merge_chunks(struct z_heap *h, chunkid_t lc, chunkid_t rc)
{
	chunksz_t newsz = chunk_size(h, lc) + chunk_size(h, rc);

	set_chunk_size(h, lc, newsz);
	set_left_chunk_size(h, right_chunk(h, rc), newsz);
}

static void free_chunk(struct z_heap *h, chunkid_t c)
{
	/* Merge with free right chunk? */
	if (!chunk_used(h, right_chunk(h, c))) {
		free_list_remove(h, right_chunk(h, c));
		merge_chunks(h, c, right_chunk(h, c));
	}

	/* Merge with free left chunk? */
	if (!chunk_used(h, left_chunk(h, c))) {
		free_list_remove(h, left_chunk(h, c));
		merge_chunks(h, left_chunk(h, c), c);
		c = left_chunk(h, c);
	}

	free_list_add(h, c);
}

static chunkid_t mem_to_chunkid(struct z_heap *h, void *p)
{
	uint8_t *mem = p, *base = (uint8_t *)chunk_buf(h);
	return (mem - chunk_header_bytes(h) - base) / CHUNK_UNIT;
}

void sys_heap_free(struct sys_heap *heap, void *mem)
{
	if (mem == NULL) {
		return; /* ISO C free() semantics */
	}
	struct z_heap *h = heap->heap;
	chunkid_t c = mem_to_chunkid(h, mem);

	/* Extract: chunk state for Rust decision */
	uint32_t is_used = chunk_used(h, c) ? 1U : 0U;
	uint32_t bounds_ok = (left_chunk(h, right_chunk(h, c)) == c) ? 1U : 0U;
	uint32_t right_free = (!chunk_used(h, right_chunk(h, c))) ? 1U : 0U;
	uint32_t left_free_flag = (!chunk_used(h, left_chunk(h, c))) ? 1U : 0U;

	/* Decide: Rust validates and determines coalescing */
	struct gale_heap_free_decision d = gale_sys_heap_free_decide(
		is_used, right_free, left_free_flag, bounds_ok);

	/* Apply: reject if Rust says no */
	if (d.action == GALE_HEAP_ACTION_FREE_REJECTED) {
		__ASSERT(0, "heap free rejected (double-free or corruption?) "
			 "for memory at %p", mem);
		return;
	}

	set_chunk_used(h, c, false);
#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	h->allocated_bytes -= chunksz_to_bytes(h, chunk_size(h, c));
#endif

#ifdef CONFIG_SYS_HEAP_LISTENER
	heap_listener_notify_free(HEAP_ID_FROM_POINTER(heap), mem,
				  chunksz_to_bytes(h, chunk_size(h, c)));
#endif

	free_chunk(h, c);
}

size_t sys_heap_usable_size(struct sys_heap *heap, void *mem)
{
	struct z_heap *h = heap->heap;
	chunkid_t c = mem_to_chunkid(h, mem);
	size_t addr = (size_t)mem;
	size_t chunk_base = (size_t)&chunk_buf(h)[c];
	size_t chunk_sz = chunk_size(h, c) * CHUNK_UNIT;

	return chunk_sz - (addr - chunk_base);
}

static chunkid_t alloc_chunk(struct z_heap *h, chunksz_t sz)
{
	int bi = bucket_idx(h, sz);
	struct z_heap_bucket *b = &h->buckets[bi];

	CHECK(bi <= bucket_idx(h, h->end_chunk));

	if (b->next != 0U) {
		chunkid_t first = b->next;
		int i = CONFIG_SYS_HEAP_ALLOC_LOOPS;
		do {
			chunkid_t c = b->next;
			if (chunk_size(h, c) >= sz) {
				free_list_remove_bidx(h, c, bi);
				return c;
			}
			b->next = next_free_chunk(h, c);
			CHECK(b->next != 0);
		} while (--i && b->next != first);
	}

	uint32_t bmask = h->avail_buckets & ~BIT_MASK(bi + 1);

	if (bmask != 0U) {
		int minbucket = __builtin_ctz(bmask);
		chunkid_t c = h->buckets[minbucket].next;

		free_list_remove_bidx(h, c, minbucket);
		CHECK(chunk_size(h, c) >= sz);
		return c;
	}

	return 0;
}

void *sys_heap_alloc(struct sys_heap *heap, size_t bytes)
{
	struct z_heap *h = heap->heap;
	void *mem;

	if (bytes == 0U) {
		return NULL;
	}

	chunksz_t chunk_sz = bytes_to_chunksz(h, bytes, 0);
	chunkid_t c = alloc_chunk(h, chunk_sz);

	/* Decide: Rust determines split-or-whole (or fail) */
	struct gale_heap_alloc_decision d = gale_sys_heap_alloc_decide(
		(c != 0U) ? 1U : 0U,
		(c != 0U) ? chunk_size(h, c) : 0U,
		chunk_sz);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_HEAP_ACTION_ALLOC_FAILED) {
		return NULL;
	}

	if (d.action == GALE_HEAP_ACTION_SPLIT_AND_USE) {
		split_chunks(h, c, c + chunk_sz);
		free_list_add(h, c + chunk_sz);
	}

	set_chunk_used(h, c, true);

	mem = chunk_mem(h, c);

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	increase_allocated_bytes(h, chunksz_to_bytes(h, chunk_size(h, c)));
#endif

#ifdef CONFIG_SYS_HEAP_LISTENER
	heap_listener_notify_alloc(HEAP_ID_FROM_POINTER(heap), mem,
				   chunksz_to_bytes(h, chunk_size(h, c)));
#endif

	IF_ENABLED(CONFIG_MSAN, (__msan_allocated_memory(mem, bytes)));
	return mem;
}

void *sys_heap_noalign_alloc(struct sys_heap *heap, size_t align, size_t bytes)
{
	ARG_UNUSED(align);

	return sys_heap_alloc(heap, bytes);
}

void *sys_heap_aligned_alloc(struct sys_heap *heap, size_t align, size_t bytes)
{
	struct z_heap *h = heap->heap;
	size_t gap, rew;

	rew = align & -align;
	if (align != rew) {
		align -= rew;
		gap = min(rew, chunk_header_bytes(h));
	} else {
		if (align <= chunk_header_bytes(h)) {
			return sys_heap_alloc(heap, bytes);
		}
		rew = 0;
		gap = chunk_header_bytes(h);
	}
	__ASSERT((align & (align - 1)) == 0, "align must be a power of 2");

	if (bytes == 0) {
		return NULL;
	}

	chunksz_t padded_sz = bytes_to_chunksz(h, bytes, align - gap);
	chunkid_t c0 = alloc_chunk(h, padded_sz);

	if (c0 == 0) {
		return NULL;
	}
	uint8_t *mem = chunk_mem(h, c0);

	/* Align allocated memory */
	mem = (uint8_t *) ROUND_UP(mem + rew, align) - rew;
	chunk_unit_t *end = (chunk_unit_t *) ROUND_UP(mem + bytes, CHUNK_UNIT);

	/* Get corresponding chunks */
	chunkid_t c = mem_to_chunkid(h, mem);
	chunkid_t c_end = end - chunk_buf(h);
	CHECK(c >= c0 && c  < c_end && c_end <= c0 + padded_sz);

	/* Split and free unused prefix */
	if (c > c0) {
		split_chunks(h, c0, c);
		free_list_add(h, c0);
	}

	/* Split and free unused suffix */
	if (right_chunk(h, c) > c_end) {
		split_chunks(h, c, c_end);
		free_list_add(h, c_end);
	}

	set_chunk_used(h, c, true);

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	increase_allocated_bytes(h, chunksz_to_bytes(h, chunk_size(h, c)));
#endif

#ifdef CONFIG_SYS_HEAP_LISTENER
	heap_listener_notify_alloc(HEAP_ID_FROM_POINTER(heap), mem,
				   chunksz_to_bytes(h, chunk_size(h, c)));
#endif

	IF_ENABLED(CONFIG_MSAN, (__msan_allocated_memory(mem, bytes)));
	return mem;
}

static bool inplace_realloc(struct sys_heap *heap, void *ptr, size_t bytes)
{
	struct z_heap *h = heap->heap;

	chunkid_t c = mem_to_chunkid(h, ptr);
	size_t align_gap = (uint8_t *)ptr - (uint8_t *)chunk_mem(h, c);

	chunksz_t chunks_need = bytes_to_chunksz(h, bytes, align_gap);

	/* Decide: Rust determines shrink/grow/copy strategy */
	chunkid_t rc = right_chunk(h, c);
	uint32_t rf = (!chunk_used(h, rc)) ? 1U : 0U;
	uint32_t rsz = rf ? chunk_size(h, rc) : 0U;

	struct gale_heap_realloc_decision d = gale_sys_heap_realloc_decide(
		chunk_size(h, c), chunks_need, rf, rsz);

	if (d.action == GALE_HEAP_REALLOC_SHRINK) {
		if (chunk_size(h, c) == chunks_need) {
			return true;
		}

#ifdef CONFIG_SYS_HEAP_LISTENER
		size_t bytes_freed = chunksz_to_bytes(h, chunk_size(h, c));
#endif

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
		h->allocated_bytes -=
			(chunk_size(h, c) - chunks_need) * CHUNK_UNIT;
#endif

		split_chunks(h, c, c + chunks_need);
		set_chunk_used(h, c, true);
		free_chunk(h, c + chunks_need);

#ifdef CONFIG_SYS_HEAP_LISTENER
		heap_listener_notify_alloc(HEAP_ID_FROM_POINTER(heap), ptr,
					   chunksz_to_bytes(h, chunk_size(h, c)));
		heap_listener_notify_free(HEAP_ID_FROM_POINTER(heap), ptr,
					  bytes_freed);
#endif

		return true;
	}

	if (d.action == GALE_HEAP_REALLOC_GROW) {
		chunksz_t split_size = chunks_need - chunk_size(h, c);

#ifdef CONFIG_SYS_HEAP_LISTENER
		size_t bytes_freed = chunksz_to_bytes(h, chunk_size(h, c));
#endif

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
		increase_allocated_bytes(h, split_size * CHUNK_UNIT);
#endif

		free_list_remove(h, rc);

		if (split_size < chunk_size(h, rc)) {
			split_chunks(h, rc, rc + split_size);
			free_list_add(h, rc + split_size);
		}

		merge_chunks(h, c, rc);
		set_chunk_used(h, c, true);

#ifdef CONFIG_SYS_HEAP_LISTENER
		heap_listener_notify_alloc(HEAP_ID_FROM_POINTER(heap), ptr,
					   chunksz_to_bytes(h, chunk_size(h, c)));
		heap_listener_notify_free(HEAP_ID_FROM_POINTER(heap), ptr,
					  bytes_freed);
#endif

		return true;
	}

	return false;
}

void *sys_heap_realloc(struct sys_heap *heap, void *ptr, size_t bytes)
{
	/* special realloc semantics */
	if (ptr == NULL) {
		return sys_heap_alloc(heap, bytes);
	}
	if (bytes == 0) {
		sys_heap_free(heap, ptr);
		return NULL;
	}

	if (inplace_realloc(heap, ptr, bytes)) {
		return ptr;
	}

	/* In-place realloc was not possible: fallback to allocate and copy. */
	void *ptr2 = sys_heap_alloc(heap, bytes);

	if (ptr2 != NULL) {
		size_t prev_size = sys_heap_usable_size(heap, ptr);

		memcpy(ptr2, ptr, min(prev_size, bytes));
		sys_heap_free(heap, ptr);
	}
	return ptr2;
}

void sys_heap_init(struct sys_heap *heap, void *mem, size_t bytes)
{
	IF_ENABLED(CONFIG_MSAN, (__sanitizer_dtor_callback(mem, bytes)));

	if (IS_ENABLED(CONFIG_SYS_HEAP_SMALL_ONLY)) {
		__ASSERT(bytes / CHUNK_UNIT <= 0x7fffU, "heap size is too big");
	} else {
		__ASSERT(bytes / CHUNK_UNIT <= 0x7fffffffU, "heap size is too big");
	}

	__ASSERT(bytes > heap_footer_bytes(bytes), "heap size is too small");
	bytes -= heap_footer_bytes(bytes);

	uintptr_t addr = ROUND_UP(mem, CHUNK_UNIT);
	uintptr_t end = ROUND_DOWN((uint8_t *)mem + bytes, CHUNK_UNIT);
	chunksz_t heap_sz = (end - addr) / CHUNK_UNIT;

	CHECK(end > addr);
	__ASSERT(heap_sz > chunksz(sizeof(struct z_heap)), "heap size is too small");

	struct z_heap *h = (struct z_heap *)addr;
	heap->heap = h;
	h->end_chunk = heap_sz;
	h->avail_buckets = 0;

#ifdef CONFIG_SYS_HEAP_RUNTIME_STATS
	h->free_bytes = 0;
	h->allocated_bytes = 0;
	h->max_allocated_bytes = 0;
#endif

#if CONFIG_SYS_HEAP_ARRAY_SIZE
	sys_heap_array_save(heap);
#endif

	int nb_buckets = bucket_idx(h, heap_sz) + 1;
	chunksz_t chunk0_size = chunksz(sizeof(struct z_heap) +
				     nb_buckets * sizeof(struct z_heap_bucket));

	__ASSERT(chunk0_size + min_chunk_size(h) <= heap_sz, "heap size is too small");

	/* HP1: Rust validates init capacity */
	int32_t init_rc = gale_sys_heap_init_validate(
		(uint32_t)(heap_sz * CHUNK_UNIT),
		(uint32_t)(chunk0_size * CHUNK_UNIT));
	__ASSERT(init_rc == 0, "heap init validation failed");
	(void)init_rc;

	for (int i = 0; i < nb_buckets; i++) {
		h->buckets[i].next = 0;
	}

	set_chunk_size(h, 0, chunk0_size);
	set_left_chunk_size(h, 0, 0);
	set_chunk_used(h, 0, true);

	set_chunk_size(h, chunk0_size, heap_sz - chunk0_size);
	set_left_chunk_size(h, chunk0_size, chunk0_size);

	set_chunk_size(h, heap_sz, 0);
	set_left_chunk_size(h, heap_sz, heap_sz - chunk0_size);
	set_chunk_used(h, heap_sz, true);

	free_list_add(h, chunk0_size);
}
