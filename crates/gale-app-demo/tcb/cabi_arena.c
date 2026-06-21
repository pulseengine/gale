/*
 * SPDX-License-Identifier: Apache-2.0
 * Copyright (c) 2026 PulseEngine
 *
 * Reference TCB arena for the dissolved library-OS image: the embedder symbol
 * `__cabi_arena_realloc` that the wit-bindgen `cabi-realloc-extern` build
 * (pulseengine/wit-bindgen#4) routes the canonical-ABI realloc to, INSTEAD of
 * the global growing allocator. Fixed static arena, **traps on exhaustion** —
 * never calls anything grow-like — so the dissolved component carries no
 * `memory.grow` and is single-address-space (MCU) lowerable (gale#89).
 *
 * For gale's scalar `gale:kernel` ABI this is never actually called at runtime
 * (no lists/strings to lift), so the arena stays empty; it exists to satisfy
 * the link and to be correct if a non-scalar interface is ever added. In the
 * real OS this lives in the gust/BYO-OS TCB, one arena/policy per fused image.
 */
#include <stdint.h>

#ifndef GALE_CABI_ARENA_SIZE
#define GALE_CABI_ARENA_SIZE 4096u
#endif

static uint8_t  gale_cabi_arena[GALE_CABI_ARENA_SIZE];
static uint32_t gale_cabi_off;

/* Canonical realloc contract: (ptr, old_size, align, new_size) -> ptr.
 * ptr==0 => allocate; else preserve min(old,new) bytes. Bump-only (no free);
 * trap on exhaustion (the bounded-static analogue of memory.grow). */
void *__cabi_arena_realloc(void *ptr, uint32_t old_size,
			   uint32_t align, uint32_t new_size)
{
	if (new_size == 0u) {
		return (void *)0;
	}
	uint32_t a = align ? align : 1u;
	uint32_t p = (gale_cabi_off + (a - 1u)) & ~(a - 1u);
	if (p + new_size > GALE_CABI_ARENA_SIZE) {
		for (;;) {
		}	/* trap: arena exhausted (bounded, no grow) */
	}
	uint8_t *np = &gale_cabi_arena[p];
	gale_cabi_off = p + new_size;
	if (ptr && old_size) {
		uint32_t n = old_size < new_size ? old_size : new_size;
		const uint8_t *src = (const uint8_t *)ptr;
		for (uint32_t i = 0; i < n; i++) {
			np[i] = src[i];
		}
	}
	return np;
}
