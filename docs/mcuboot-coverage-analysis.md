# MCUboot Coverage Analysis vs. Gale

Decision-support table for a potential verified bootloader replacement (`gale-boot`).
This is a coverage mapping only — not a recommendation.

## Methodology

- MCUboot source: upstream `mcu-tools/mcuboot`, shallow clone at `/tmp/mcuboot-analysis`,
  analyzed as of 2026-04-19.
- Target configuration: Zephyr port, `qemu_cortex_m`-style target, defaults from
  `boot/zephyr/Kconfig`:
  - `CONFIG_BOOT_SIGNATURE_TYPE_RSA=y` (default, RSA-2048 via mbedTLS)
  - `CONFIG_BOOT_USE_MBEDTLS=y` (pulled in by RSA default)
  - `CONFIG_BOOT_SWAP_USING_MOVE=y` (per task scope)
  - `CONFIG_BOOT_ENCRYPT_IMAGE=n` (default off)
  - Single-image, no serial recovery, no RAM load, no firmware loader,
    no FIH high profile, no measured boot.
- "In-scope" = files actually compiled into the boot image given the above,
  as determined from `boot/zephyr/CMakeLists.txt` and `boot/bootutil/zephyr/CMakeLists.txt`.
- LOC counted with `wc -l` on the raw `.c` files (comments + blank lines included;
  `cloc` not available in this environment). Headers, tests, simulator, alternate
  crypto backends (TinyCrypt, CC310, PSA), scratch / offset swap variants,
  and `single_loader`/`firmware_loader`/`ram_load` code paths are excluded.
- Files that compile but whose body is fully guarded out by `#ifdef`
  for this config (e.g. `encrypted.c` when `MCUBOOT_ENC_IMAGES` is unset,
  `image_ecdsa.c` / `image_ed25519.c` under RSA default) are counted as
  "0 effective" with the raw file LOC in parentheses.
- Gale modules inspected at `/Users/r/git/pulseengine/z/gale/src/` (50 `.rs`
  files). Grep for `sha|crypt|aes|rsa|ecdsa|ed25519|hash|flash|nvm|partition`
  confirms Gale has no cryptographic primitives and no image-flash driver;
  the only storage-adjacent module is `zms.rs` (Zephyr Memory Storage — a
  key-value store over flash, not an image partition manager).

## Coverage table

| Subsystem | LOC (in-scope) | Gale coverage today | Proof complexity if built | Notes |
|---|---|---|---|---|
| Boot loop / main entry (`main.c`, `os.c`, `keys.c`, `watchdog.c`) | 958 | None | Low | Linear startup, device init, vector-table jump. Mostly imperative with few invariants beyond "jump only after validate succeeded". |
| Image header parsing + validate driver (`image_validate.c`, `bootutil_find_key.c`) | 735 | None (Gale has `error.rs` as reusable primitive for result types) | Medium | Parses fixed-layout `image_header` struct, iterates TLV, dispatches to hash/sig. Bounded parsing — classic deserializer proof shape. |
| TLV manifest parser (`tlv.c`) | 178 | None | Medium | Offset/length arithmetic over untrusted flash; must prove no reads past `hdr->ih_protect_tlv_size + ih_img_size` and monotonic cursor advance. |
| SHA-256 digest + RSA-2048 PKCS#1 v1.5 verify (`bootutil_img_hash.c` 198, `image_rsa.c` 292) | 490 (bootutil glue only) | None — no crypto in Gale | Very High (if primitives built from scratch); Medium (if glue-only, wrapping a trusted mbedTLS / formally-verified external like HACL\* / Fiat) | MCUboot itself delegates to `mbedTLS` (not counted here — external dependency). Gale would need either a verified SHA-256 and bignum RSA or a trusted-wrapper model around an external verified library. |
| Unused-but-compiled signature stubs (`image_ecdsa.c`, `image_ed25519.c`) | 0 effective (258 raw) | n/a | n/a | `#ifdef`-guarded out under `BOOT_SIGNATURE_TYPE_RSA` default. Listed for accountability. |
| Flash driver abstraction (`io.c`, `flash_map_extended.c`, `flash_check.c`, `bootutil_area.c`) | 878 | Partial: `zms.rs` is a verified sector-state model but targets KV storage (ZMS), not the MCUboot `flash_area` / partition-map API. Reusable insight, not reusable code. | High | Must model partition geometry, erase/write alignment, and crash-safety of each individual write. The `bootutil_area` write helpers are where swap atomicity ultimately lives. |
| A/B swap state machine — MOVE strategy (`loader.c` 2530, `swap_misc.c` 254, `swap_move.c` 583, `bootutil_loader.c` 401, `bootutil_misc.c` 666, `bootutil_public.c` 802) | 5236 | None | **Very High** | Crash-safety invariant across interruptible writes: any power loss between any two flash ops must leave the device in a recoverable state. `bootutil_public.c` (trailer magic / `boot_set_pending` / `boot_set_confirmed`) and `swap_move.c` together encode the resumable state machine — this is the dominant proof burden. |
| Rollback / security counter check (`bootutil_img_security_cnt.c`) | 102 | None | Low–Medium | Monotonic-counter check against a platform-stored value; proof is "new ≥ stored, else reject". Simple once the flash/OTP read is trusted. |
| Encrypted images (`encrypted.c`) | 0 effective (723 raw) | n/a | n/a | `MCUBOOT_ENC_IMAGES` unset in default config → entire TU is `#if`-empty. Out of scope. |
| bootutil / misc (FIH, `caps.c`) | 181 (`fault_injection_hardening.c` 84 + `caps.c` 97) | None | Low | FIH is glitch-hardening helpers; `caps.c` is a constant-table reporter. Neither has non-trivial state. |
| **Total in-scope, compiled** | **~8 758** | | | Excludes `mbedTLS` (external, ~tens of kLOC pulled by linker) and the ~258 LOC of inactive ECDSA/Ed25519 stubs. |

## Key findings

- MCUboot's in-scope compiled footprint is ~8.8 kLOC of C; ~60% of that
  (5 236 LOC) is the swap state machine and its trailer/magic bookkeeping —
  this is the load-bearing proof target, not the crypto.
- Gale today covers **zero** of the MCUboot subsystems directly. The only
  adjacencies are `error.rs` (reusable result-type primitive) and `zms.rs`
  (verified flash sector-state model, but for KV storage, not image partitions).
- Cryptographic primitives (SHA-256, RSA-2048) are delegated by MCUboot to
  `mbedTLS` — a verified replacement would either need a formally verified
  crypto library imported from elsewhere (HACL\*, Fiat) or a trusted-wrapper
  model that treats mbedTLS as an axiomatized oracle.
- `encrypted.c` is a large file (723 LOC) but contributes 0 LOC in the default
  config; easy to mis-count if only `wc -l` on the source directory is used.

## Uncertainties

- The exact compiled LOC depends on Zephyr's preprocessor output, not just the
  CMake file list; a more accurate count would require invoking the actual
  Zephyr build and reading `.i` outputs. `wc -l` on raw sources over-counts
  comments/blank lines; a `cloc` pass would adjust each figure downward by
  roughly 20–30% but would not change the ranking.
- For `qemu_cortex_m` specifically, whether the default is truly
  `BOOT_SWAP_USING_MOVE` or falls through to `BOOT_SWAP_USING_OFFSET` (which
  is `default y` when no scratch partition exists, per `Kconfig:561`) was not
  resolved against the sample `.conf` files — the task fixed MOVE as in-scope.
- The mbedTLS subset actually linked in (how much of `rsa.c`, `bignum.c`,
  `sha256.c`) was not measured — it sits outside the MCUboot tree and depends
  on `CONFIG_MBEDTLS_*` selections pulled in transitively.
- FIH delay-RNG file (`fault_injection_hardening_delay_rng_mbedtls.c`, 47 LOC)
  compiles only under `BOOT_FIH_PROFILE_HIGH`; assumed off for the default
  profile but not confirmed against a specific board config.
- Gale's `zms.rs` is a *model* (stripped shim in `plain/src/`); whether its
  invariants (ATE/data write-pointer separation, GC trigger correctness) are
  adaptable to an image-swap trailer was not investigated — they target a
  different storage abstraction.
