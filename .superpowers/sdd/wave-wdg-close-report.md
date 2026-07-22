# wdg-thin close — wave report

Branch: `feat/gust-close-wdg`, pushed to origin.
Commits:
- `b2a40ec` — rivet REQ-DRV-WDG-001 + VER-DRV-WDG-001 (proposed)
- `a167c42` — qemu demonstrator + Renode content-gate + VER-DRV-WDG-001 flipped to verified

HEAD: `a167c42a330791f2ff0aa4122804a41a930fbfc0`

## What was built

1. **rivet (commit 1, `artifacts/gust_os_roadmap.yaml`)**
   - `REQ-DRV-WDG-001` — the IWDG watchdog backstop requirement (mmio-only thin
     seam, key-sequence lifecycle, cannot-un-start, 0-SRAM), `status: implemented`,
     `related-to REQ-DRV-BREADTH-001`.
   - `VER-DRV-WDG-001` opened `status: proposed` (Kani 7/7 real, demonstrator/gate
     not yet built).
   - `rivet validate`: PASS (332 warnings — identical to the pre-change baseline,
     confirmed by running validate before and after).

2. **`benches/gust/src/bin/gust_wdg_probe.rs`** — local qemu-semihosting oracle.
   Points the dissolved `wdg-thin-cm3.o` at a `[u32;8]` RAM window and drives:
   config-before-unlock (write-protection) → unlock → configure → lock → start →
   [cannot-un-start block: unlock/configure/lock/restart all attempted while
   Running] → refresh ×2. Asserts the exact KR values (0x5555/0xCCCC/0xAAAA),
   PR/RLR contents, and `wdg_is_running`.

3. **`benches/gust/build.rs`** — added the wdg-thin link block (mirrors the
   spi-thin block exactly), linking `wdg-thin-cm3.o` into both `gust_wdg` and
   `gust_wdg_probe`. Verified other bins' link args are untouched (`cargo build
   --bins` still links gust_spi/gust_spi_probe/gust_gpio/etc. — see gates below).

4. **Renode content-gate**
   - `benches/gust/src/bin/gust_wdg.rs` — same lifecycle as the probe, reporting
     each step over USART1 as `wdg-*-ok`/`wdg-*-bad` lines.
   - `benches/gust/renode-test/gust_wdg.repl` — Cortex-M3 + RAM-mapped IWDG window
     at the real STM32F1 address `0x4000_3000` (+0x400), USART1 at `0x4001_3800`
     (no overlap).
   - `benches/gust/renode-test/gust_wdg.robot` — waits for
     `wdg-gate begin` → `wdg-protect-ok` → `wdg-unlock-ok` → `wdg-config-ok` →
     `wdg-start-ok` → `wdg-cannot-un-start-ok` → `wdg-refresh-ok` → `wdg-gate done`.
   - `benches/gust/renode-test/gust_wdg.elf` — built
     `cargo build --release --bin gust_wdg --target thumbv7m-none-eabi`, copied in
     (same pattern as `gust_spi.elf`).
   - `benches/gust/renode-test/BUILD.bazel` — new `renode_test` target
     `gust-wdg-renode`.
   - `.github/workflows/gust-renode.yml` — `//:gust-wdg-renode` added to the bazel
     test list (alongside `//:gust-spi-renode`).

5. **`VER-DRV-WDG-001` flipped to `status: verified`** citing the 7 Kani harness
   names (`p1_write_protection` … `p7_unlock_gates_config`), the probe evidence,
   and the Renode gate. Scope stated explicitly: qemu probe + Renode content-gate
   (register-level, real M3 model) — not silicon.

## Verbatim probe output (gate 1)

```
$ cargo run --bin gust_wdg_probe
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.06s
     Running `qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic -icount shift=1 -semihosting-config enable=on,target=native -kernel target/thumbv7m-none-eabi/debug/gust_wdg_probe`
wdg-protect ok: config-from-Idle faulted, no register write
wdg-unlock ok: KR=0x5555
wdg-config ok: PR=0x5 RLR=0x123
wdg-lock ok: s3=0x80005123, KR unchanged=0x5555, running=0
wdg-start ok: KR=0xcccc running=1
wdg-cannot-un-start ok: unlock/config/lock/restart all faulted while Running, no register touched, running stays 1
wdg-refresh ok: KR=0xaaaa running stays 1 across repeated refresh
wdg-probe ALL OK
```
Exit code: `0` (confirmed via `echo EXIT=$?` immediately after the run).

## Register-value assertions (what the probe/gate actually check, not just "OK")

- Config attempted from Idle (packed state `0`) returns the `WDG_FAULT` sentinel
  (`0xFFFF_FFFF`) AND leaves KR/PR/RLR at `0` — write-protection is enforced, not
  a no-op that happens to look fine.
- `wdg_unlock` writes `KR == 0x5555` exactly.
- `wdg_configure(prescaler=5, reload=0x123)` (after unlock) writes `PR == 0x5` and
  `RLR == 0x123` exactly (both within their field widths, so masking is a no-op
  here — the mask behavior itself is covered by Kani p5, not by this probe).
- `wdg_start` writes `KR == 0xCCCC` and `wdg_is_running` flips `0 → 1`.
- With the watchdog Running: `wdg_unlock`, `wdg_configure`, `wdg_lock`, and
  `wdg_start` (restart) each return `WDG_FAULT`, and KR/PR/RLR are byte-for-byte
  unchanged from their post-start values (`0xCCCC` / `0x5` / `0x123`) — no call
  touches a register. `wdg_is_running` stays `1`.
- Two successive `wdg_refresh` calls each write `KR == 0xAAAA` and
  `wdg_is_running` stays `1` after both.

## Gates run (verbatim exits)

```
$ cargo run --bin gust_wdg_probe        → wdg-probe ALL OK, exit 0
$ cargo run --bin gust_spi_probe        → spi-probe ALL OK, exit 0   (regression)
$ cargo build --bins                    → Finished, exit 0
                                           (one PRE-EXISTING unrelated warning:
                                           unnecessary `unsafe` in gust_breadth_probe.rs)
$ rivet validate                        → Result: PASS (332 warnings), exit 0
$ bazel query //:gust-wdg-renode        → //:gust-wdg-renode  (loads)
$ bazel query "<all 10 gust-renode targets, incl. gust-wdg-renode>"
                                         → all 10 resolve cleanly
```

## Honest gaps

- **No silicon run.** Verification scope is qemu (local probe, differentially
  trusted mmio bridge) + Renode (real M3 model, register-level content-gate).
  Matches the same honesty scope already recorded for VER-DRV-SPI-001/GPIO/TIMER
  (see `docs/safety/verification-honesty.md`).
- **The Renode bazel test itself did not execute in this environment** — macOS has
  no toolchain for `rules_renode`'s hermetic Linux/.NET Renode fetch, which is a
  pre-existing limitation shared by every `renode_test` target in this suite
  (not specific to wdg). `bazel query` was used to confirm the target loads and
  is correctly wired; actual green/red is CI's call (`.github/workflows/gust-renode.yml`
  runs on `ubuntu-22.04`).
- **The wasm→native dissolve is differentially trusted, not proven equivalent** —
  stated explicitly in VER-DRV-WDG-001, per repo-wide policy.
- The `wdg-thin` driver's Cargo.lock/target/ artifacts were pre-existing (already
  dissolved per the task's constraints) and were not touched.
