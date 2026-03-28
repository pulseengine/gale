/*
 * Copyright (c) 2017 Linaro Limited
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale memory domain — Extract/Decide/Apply pattern.
 *
 * This is kernel/mem_domain.c with partition validation and slot
 * management rewritten to use Rust decision structs.  C extracts
 * partition arrays (start, size), Rust decides the action, C applies it.
 *
 * Verified operations (Verus proofs):
 *   gale_mem_domain_check_partition             — MD1, MD3, MD6
 *   gale_k_mem_domain_add_partition_decide      — MD1-MD6
 *   gale_k_mem_domain_remove_partition_decide   — MD5
 *   gale_mem_domain_init_validate_partition      — MD1, MD3, MD6
 *
 * Thread list management, arch domain init/sync, W^X policy, SYS_INIT,
 * deinit, add/remove thread, spinlock — all remain native Zephyr C.
 */

#include <zephyr/init.h>
#include <zephyr/kernel.h>
#include <zephyr/kernel_structs.h>
#include <kernel_internal.h>
#include <zephyr/sys/__assert.h>
#include <stdbool.h>
#include <zephyr/spinlock.h>
#include <zephyr/sys/check.h>
#include <zephyr/sys/libc-hooks.h>
#include <zephyr/logging/log.h>
LOG_MODULE_DECLARE(os, CONFIG_KERNEL_LOG_LEVEL);

#include "gale_mem_domain.h"

struct k_spinlock z_mem_domain_lock;
static uint8_t max_partitions;

struct k_mem_domain k_mem_domain_default;

/*
 * Helper: extract start[] and size[] arrays from the domain's partition
 * array for passing to the Rust decision functions.
 *
 * The Rust FFI takes flat uint32_t arrays (no struct-of-arrays in C ABI),
 * so we copy out just the fields Rust needs.
 */
static void extract_partition_arrays(const struct k_mem_domain *domain,
				     uint32_t starts[CONFIG_MAX_DOMAIN_PARTITIONS],
				     uint32_t sizes[CONFIG_MAX_DOMAIN_PARTITIONS])
{
	for (int i = 0; i < CONFIG_MAX_DOMAIN_PARTITIONS; i++) {
		starts[i] = (uint32_t)domain->partitions[i].start;
		sizes[i]  = (uint32_t)domain->partitions[i].size;
	}
}

int k_mem_domain_init(struct k_mem_domain *domain, uint8_t num_parts,
		      struct k_mem_partition *parts[])
{
	k_spinlock_key_t key;
	int ret = 0;

	CHECKIF(domain == NULL) {
		ret = -EINVAL;
		goto out;
	}

	CHECKIF(!(num_parts == 0U || parts != NULL)) {
		LOG_ERR("parts array is NULL and num_parts is nonzero");
		ret = -EINVAL;
		goto out;
	}

	CHECKIF(!(num_parts <= max_partitions)) {
		LOG_ERR("num_parts of %d exceeds maximum allowable partitions (%d)",
			num_parts, max_partitions);
		ret = -EINVAL;
		goto out;
	}

	key = k_spin_lock(&z_mem_domain_lock);

	domain->num_partitions = 0U;
	(void)memset(domain->partitions, 0, sizeof(domain->partitions));

#ifdef CONFIG_MEM_DOMAIN_HAS_THREAD_LIST
	sys_dlist_init(&domain->thread_mem_domain_list);
#endif /* CONFIG_MEM_DOMAIN_HAS_THREAD_LIST */

#ifdef CONFIG_ARCH_MEM_DOMAIN_DATA
	ret = arch_mem_domain_init(domain);

	if (ret != 0) {
		LOG_ERR("architecture-specific initialization failed for domain %p with %d",
			domain, ret);
		ret = -ENOMEM;
		goto unlock_out;
	}
#endif /* CONFIG_ARCH_MEM_DOMAIN_DATA */
	if (num_parts != 0U) {
		uint32_t i;

		for (i = 0U; i < num_parts; i++) {
			/* Extract: gather current partition state */
			uint32_t starts[CONFIG_MAX_DOMAIN_PARTITIONS];
			uint32_t sizes[CONFIG_MAX_DOMAIN_PARTITIONS];

			extract_partition_arrays(domain, starts, sizes);

			/* Decide: Rust validates the partition */
			struct gale_mem_domain_init_part_decision d =
				gale_mem_domain_init_validate_partition(
					(uint32_t)parts[i]->start,
					(uint32_t)parts[i]->size,
					starts, sizes,
					domain->num_partitions);

			CHECKIF(d.ret != 0) {
				LOG_ERR("invalid partition index %d (%p)",
					i, parts[i]);
				ret = -EINVAL;
				goto unlock_out;
			}

			/* Apply: place partition */
			domain->partitions[i] = *parts[i];
			domain->num_partitions++;
#ifdef CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API
			int ret2 = arch_mem_domain_partition_add(domain, i);

			ARG_UNUSED(ret2);
			CHECKIF(ret2 != 0) {
				ret = ret2;
			}
#endif /* CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API */
		}
	}

unlock_out:
	k_spin_unlock(&z_mem_domain_lock, key);

out:
	return ret;
}

int k_mem_domain_deinit(struct k_mem_domain *domain)
{
#if defined(CONFIG_ARCH_MEM_DOMAIN_SUPPORTS_DEINIT)
	k_spinlock_key_t key;
	int ret = 0;

	CHECKIF(domain == NULL) {
		ret = -EINVAL;
		goto out;
	}

	if (domain == &k_mem_domain_default) {
		/* Default memory domain must be there forever. */
		ret = -EINVAL;
		goto out;
	}

	key = k_spin_lock(&z_mem_domain_lock);

	/* Must make sure there are no threads associated with this memory
	 * domain anymore. Or else these threads will run with an invalid
	 * memory domain.
	 */
	if (!sys_dlist_is_empty(&domain->thread_mem_domain_list)) {
		ret = -EBUSY;
		goto unlock_out;
	}

	ret = arch_mem_domain_deinit(domain);
	if (ret != 0) {
		LOG_ERR("architecture-specific de-initialization failed for domain %p with %d",
			domain, ret);
		ret = -ENOMEM;
		goto unlock_out;
	}

unlock_out:
	k_spin_unlock(&z_mem_domain_lock, key);

out:
	return ret;
#else  /* CONFIG_ARCH_MEM_DOMAIN_SUPPORTS_DEINIT */
	return -ENOTSUP;
#endif /* CONFIG_ARCH_MEM_DOMAIN_SUPPORTS_DEINIT */
}

int k_mem_domain_add_partition(struct k_mem_domain *domain,
			       struct k_mem_partition *part)
{
	k_spinlock_key_t key;
	int ret = 0;

	CHECKIF(domain == NULL) {
		ret = -EINVAL;
		goto out;
	}

	CHECKIF(part == NULL) {
		LOG_ERR("NULL k_mem_partition provided");
		ret = -EINVAL;
		goto out;
	}

	key = k_spin_lock(&z_mem_domain_lock);

	/* Extract: gather current partition state */
	uint32_t starts[CONFIG_MAX_DOMAIN_PARTITIONS];
	uint32_t sizes[CONFIG_MAX_DOMAIN_PARTITIONS];

	extract_partition_arrays(domain, starts, sizes);

	/* Decide: Rust validates partition + finds free slot */
	struct gale_mem_domain_add_decision d =
		gale_k_mem_domain_add_partition_decide(
			(uint32_t)part->start,
			(uint32_t)part->size,
			(uint32_t)part->attr,
			starts, sizes,
			domain->num_partitions);

	if (d.action == GALE_MEM_DOMAIN_ACTION_ADD_ERROR) {
		if (d.ret == -ENOSPC) {
			LOG_ERR("no free partition slots available");
		} else {
			LOG_ERR("invalid partition %p", part);
		}
		ret = d.ret;
		goto unlock_out;
	}

	/* Apply: place partition at Rust-chosen slot */
	LOG_DBG("add partition base %lx size %zu to domain %p",
		part->start, part->size, domain);

	domain->partitions[d.slot].start = part->start;
	domain->partitions[d.slot].size = part->size;
	domain->partitions[d.slot].attr = part->attr;

	domain->num_partitions = (uint8_t)d.new_num_partitions;

#ifdef CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API
	ret = arch_mem_domain_partition_add(domain, d.slot);
#endif /* CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API */

unlock_out:
	k_spin_unlock(&z_mem_domain_lock, key);

out:
	return ret;
}

int k_mem_domain_remove_partition(struct k_mem_domain *domain,
				  struct k_mem_partition *part)
{
	k_spinlock_key_t key;
	int ret = 0;

	CHECKIF((domain == NULL) || (part == NULL)) {
		ret = -EINVAL;
		goto out;
	}

	key = k_spin_lock(&z_mem_domain_lock);

	/* Extract: gather current partition state */
	uint32_t starts[CONFIG_MAX_DOMAIN_PARTITIONS];
	uint32_t sizes[CONFIG_MAX_DOMAIN_PARTITIONS];

	extract_partition_arrays(domain, starts, sizes);

	/* Decide: Rust finds matching partition */
	struct gale_mem_domain_remove_decision d =
		gale_k_mem_domain_remove_partition_decide(
			(uint32_t)part->start,
			(uint32_t)part->size,
			starts, sizes,
			domain->num_partitions);

	if (d.action == GALE_MEM_DOMAIN_ACTION_REMOVE_ERROR) {
		LOG_ERR("no matching partition found");
		ret = d.ret;
		goto unlock_out;
	}

	/* Apply: clear the slot */
	LOG_DBG("remove partition base %lx size %zu from domain %p",
		part->start, part->size, domain);

#ifdef CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API
	ret = arch_mem_domain_partition_remove(domain, d.slot);
#endif /* CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API */

	/* A zero-sized partition denotes it's a free partition */
	domain->partitions[d.slot].size = 0U;

	domain->num_partitions = (uint8_t)d.new_num_partitions;

unlock_out:
	k_spin_unlock(&z_mem_domain_lock, key);

out:
	return ret;
}

static int add_thread_locked(struct k_mem_domain *domain,
			     k_tid_t thread)
{
	int ret = 0;

	__ASSERT_NO_MSG(domain != NULL);
	__ASSERT_NO_MSG(thread != NULL);

	LOG_DBG("add thread %p to domain %p", thread, domain);

#ifdef CONFIG_MEM_DOMAIN_HAS_THREAD_LIST
	sys_dlist_append(&domain->thread_mem_domain_list,
			 &thread->mem_domain_info.thread_mem_domain_node);
#endif /* CONFIG_MEM_DOMAIN_HAS_THREAD_LIST */

	thread->mem_domain_info.mem_domain = domain;

#ifdef CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API
	ret = arch_mem_domain_thread_add(thread);
#endif /* CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API */

	return ret;
}

static int remove_thread_locked(struct k_thread *thread)
{
	int ret = 0;

	__ASSERT_NO_MSG(thread != NULL);
	LOG_DBG("remove thread %p from memory domain %p",
		thread, thread->mem_domain_info.mem_domain);

#ifdef CONFIG_MEM_DOMAIN_HAS_THREAD_LIST
	sys_dlist_remove(&thread->mem_domain_info.thread_mem_domain_node);
#endif /* CONFIG_MEM_DOMAIN_HAS_THREAD_LIST */

#ifdef CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API
	ret = arch_mem_domain_thread_remove(thread);
#endif /* CONFIG_ARCH_MEM_DOMAIN_SYNCHRONOUS_API */

	return ret;
}

/* Called from thread object initialization */
void z_mem_domain_init_thread(struct k_thread *thread)
{
	int ret;
	k_spinlock_key_t key = k_spin_lock(&z_mem_domain_lock);

	/* New threads inherit memory domain configuration from parent */
	ret = add_thread_locked(_current->mem_domain_info.mem_domain, thread);
	__ASSERT_NO_MSG(ret == 0);
	ARG_UNUSED(ret);

	k_spin_unlock(&z_mem_domain_lock, key);
}

/* Called when thread aborts during teardown tasks. _sched_spinlock is held */
void z_mem_domain_exit_thread(struct k_thread *thread)
{
	int ret;

	k_spinlock_key_t key = k_spin_lock(&z_mem_domain_lock);

	ret = remove_thread_locked(thread);
	__ASSERT_NO_MSG(ret == 0);
	ARG_UNUSED(ret);

	k_spin_unlock(&z_mem_domain_lock, key);
}

int k_mem_domain_add_thread(struct k_mem_domain *domain, k_tid_t thread)
{
	int ret = 0;
	k_spinlock_key_t key;

	key = k_spin_lock(&z_mem_domain_lock);
	if (thread->mem_domain_info.mem_domain != domain) {
		ret = remove_thread_locked(thread);

		if (ret == 0) {
			ret = add_thread_locked(domain, thread);
		}
	}
	k_spin_unlock(&z_mem_domain_lock, key);

	return ret;
}

static int init_mem_domain_module(void)
{
	int ret;

	ARG_UNUSED(ret);

	max_partitions = arch_mem_domain_max_partitions_get();
	/*
	 * max_partitions must be less than or equal to
	 * CONFIG_MAX_DOMAIN_PARTITIONS, or would encounter array index
	 * out of bounds error.
	 */
	__ASSERT(max_partitions <= CONFIG_MAX_DOMAIN_PARTITIONS, "");

	ret = k_mem_domain_init(&k_mem_domain_default, 0, NULL);
	__ASSERT(ret == 0, "failed to init default mem domain");

#ifdef Z_LIBC_PARTITION_EXISTS
	ret = k_mem_domain_add_partition(&k_mem_domain_default,
					 &z_libc_partition);
	__ASSERT(ret == 0, "failed to add default libc mem partition");
#endif /* Z_LIBC_PARTITION_EXISTS */

	return 0;
}

SYS_INIT(init_mem_domain_module, PRE_KERNEL_1,
	 CONFIG_KERNEL_INIT_PRIORITY_DEFAULT);
