/*
 * Copyright (c) 2026 PulseEngine
 *
 * SPDX-License-Identifier: Apache-2.0
 *
 * Gale IPC service FFI — formally verified IPC endpoint lifecycle.
 *
 * These functions replace the validation and state-machine logic from
 * subsys/ipc/ipc_service/ipc_service.c.  Backend transport (rpmsg,
 * icmsg), interrupt wiring, and shared-memory setup remain native
 * Zephyr C.
 *
 * Verified properties (Verus SMT/Z3):
 *   IPC1: Endpoint state is always a valid variant
 *   IPC2: Open only succeeds from Closed state (no double-open)
 *   IPC3: send/send_critical only when endpoint is Bound
 *   IPC4: Close always returns endpoint to Closed
 *   IPC5: Registered endpoint count never exceeds MAX_ENDPOINTS (16)
 *   IPC6: Buffer length for send is within [1, MAX_MSG_LEN] (4096)
 */

#ifndef GALE_IPC_H_
#define GALE_IPC_H_

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

/* -------------------------------------------------------------------------
 * Constants
 * ---------------------------------------------------------------------- */

/** Maximum simultaneously registered endpoints per instance. */
#define GALE_IPC_MAX_ENDPOINTS  16U

/** Maximum message payload length in bytes. */
#define GALE_IPC_MAX_MSG_LEN    4096U

/* -------------------------------------------------------------------------
 * Endpoint state
 * ---------------------------------------------------------------------- */

/** Endpoint state codes returned / accepted by the shim. */
#define GALE_IPC_STATE_CLOSED  0U  /**< Not registered. */
#define GALE_IPC_STATE_OPEN    1U  /**< Registered, awaiting remote bind. */
#define GALE_IPC_STATE_BOUND   2U  /**< Both sides ready; transfers allowed. */

/* -------------------------------------------------------------------------
 * Instance-level operations
 * (ipc_service_open_instance / ipc_service_close_instance)
 * ---------------------------------------------------------------------- */

/**
 * Decide whether the IPC instance may be opened.
 *
 * ipc_service.c:17-39 — validates instance and backend pointers.
 *
 * @param instance_valid  true when the device pointer is non-NULL and
 *                        the backend API pointer is non-NULL.
 *
 * @return  0 on success, -EINVAL when instance_valid is false.
 *
 * Verified: IPC2.
 */
int32_t gale_ipc_open_decide(bool instance_valid);

/**
 * Decide whether the IPC instance may be closed.
 *
 * ipc_service.c:41-63 — validates instance pointer.
 *
 * @param instance_valid  true when the device pointer is non-NULL.
 *
 * @return  0 on success, -EINVAL when instance_valid is false.
 *
 * Verified: IPC4.
 */
int32_t gale_ipc_close_decide(bool instance_valid);

/* -------------------------------------------------------------------------
 * Endpoint registration
 * (ipc_service_register_endpoint / ipc_service_deregister_endpoint)
 * ---------------------------------------------------------------------- */

/**
 * Decide whether an endpoint may be registered.
 *
 * ipc_service.c:65-88 — checks instance/endpoint/cfg and capacity.
 *
 * @param params_valid       true when instance, ept, and cfg are non-NULL.
 * @param registered_count   Current number of registered endpoints.
 * @param max_endpoints      Capacity of this instance (<=GALE_IPC_MAX_ENDPOINTS).
 * @param new_count_out      Receives the updated count on success.
 *
 * @return  0 on success, -EINVAL on null params, -ENOMEM when full.
 *
 * Verified: IPC1, IPC2, IPC5.
 */
int32_t gale_ipc_register_decide(bool     params_valid,
                                  uint32_t registered_count,
                                  uint32_t max_endpoints,
                                  uint32_t *new_count_out);

/**
 * Decide whether an endpoint may be deregistered.
 *
 * ipc_service.c:90-120 — checks endpoint validity and registration.
 *
 * @param endpoint_valid      true when the ipc_ept pointer is non-NULL.
 * @param endpoint_registered true when ept->instance is non-NULL.
 * @param registered_count    Current number of registered endpoints.
 * @param new_count_out       Receives the updated count on success.
 *
 * @return  0 on success, -EINVAL on null endpoint, -ENOENT if not registered.
 *
 * Verified: IPC1, IPC4, IPC5.
 */
int32_t gale_ipc_deregister_decide(bool     endpoint_valid,
                                    bool     endpoint_registered,
                                    uint32_t registered_count,
                                    uint32_t *new_count_out);

/* -------------------------------------------------------------------------
 * Send / receive validation
 * (ipc_service_send / ipc_service_send_critical)
 * ---------------------------------------------------------------------- */

/**
 * Decide whether a send operation is valid.
 *
 * ipc_service.c:123-145.
 *
 * @param endpoint_valid      true when the ipc_ept pointer is non-NULL.
 * @param endpoint_registered true when ept->instance is non-NULL.
 * @param state               Current endpoint state (GALE_IPC_STATE_*).
 * @param len                 Payload length in bytes.
 *
 * @return  0 on success, -EINVAL or -ENOENT on failure.
 *
 * Verified: IPC1, IPC3, IPC6.
 */
int32_t gale_ipc_send_decide(bool     endpoint_valid,
                              bool     endpoint_registered,
                              uint32_t state,
                              uint32_t len);

/**
 * Decide whether a critical send is valid (same rules as send).
 *
 * ipc_service.c:147-169.
 *
 * Verified: IPC1, IPC3, IPC6.
 */
int32_t gale_ipc_send_critical_decide(bool     endpoint_valid,
                                       bool     endpoint_registered,
                                       uint32_t state,
                                       uint32_t len);

/**
 * Validate a receive operation before delivering to the callback.
 *
 * @param endpoint_valid      true when the ipc_ept pointer is non-NULL.
 * @param endpoint_registered true when ept->instance is non-NULL.
 * @param state               Current endpoint state (GALE_IPC_STATE_*).
 * @param len                 Payload length in bytes.
 *
 * @return  0 on success, -EINVAL or -ENOENT on failure.
 *
 * Verified: IPC1, IPC3, IPC6.
 */
int32_t gale_ipc_receive_decide(bool     endpoint_valid,
                                 bool     endpoint_registered,
                                 uint32_t state,
                                 uint32_t len);

/**
 * Validate a TX buffer-size query.
 *
 * ipc_service.c:171-198 — get_tx_buffer_size.
 *
 * @param endpoint_valid      true when the ipc_ept pointer is non-NULL.
 * @param endpoint_registered true when ept->instance is non-NULL.
 * @param reported_size       Size returned by the backend (bytes).
 *
 * @return  0 when size is valid, -EINVAL or -ENOENT otherwise.
 *
 * Verified: IPC1, IPC5, IPC6.
 */
int32_t gale_ipc_validate_buffer_size(bool     endpoint_valid,
                                       bool     endpoint_registered,
                                       uint32_t reported_size);

/* -------------------------------------------------------------------------
 * Endpoint state transition helpers
 * (called from the backend-bound callback and deregister path)
 * ---------------------------------------------------------------------- */

/**
 * Validate a transition from Closed to Open.
 *
 * @param current_state  Current endpoint state (GALE_IPC_STATE_*).
 * @param new_state_out  Receives GALE_IPC_STATE_OPEN on success.
 *
 * @return  0 on success, -EALREADY when not in Closed.
 *
 * Verified: IPC2.
 */
int32_t gale_ipc_transition_open(uint32_t current_state,
                                  uint32_t *new_state_out);

/**
 * Validate a transition from Open to Bound.
 *
 * @param current_state  Current endpoint state (GALE_IPC_STATE_*).
 * @param new_state_out  Receives GALE_IPC_STATE_BOUND on success.
 *
 * @return  0 on success, -EINVAL when not in Open.
 *
 * Verified: IPC1.
 */
int32_t gale_ipc_transition_bound(uint32_t current_state,
                                   uint32_t *new_state_out);

/**
 * Force the endpoint to Closed (deregister or error path).
 *
 * Always succeeds and sets *new_state_out = GALE_IPC_STATE_CLOSED.
 *
 * @param new_state_out  Receives GALE_IPC_STATE_CLOSED.
 *
 * @return  0.
 *
 * Verified: IPC4.
 */
int32_t gale_ipc_transition_close(uint32_t *new_state_out);

#ifdef __cplusplus
}
#endif

#endif /* GALE_IPC_H_ */
