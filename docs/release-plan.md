
## Wasm-module distribution (added 2026-06-11)

Each release ships the wasm-cross-LTO artifacts per `docs/wasm-module-distribution.md`:
the dissolved `.wasm` (verification artifact), per-target `.o`, and the sha256+toolchain
manifest (sigil-signed once the signing flow lands). v0.1.0 ships the **sem** module;
consumption is `CONFIG_GALE_WASM_LTO_SEM=y` + `-DGALE_WASM_LTO_OBJ_DIR=<assets>`.
Measured: the released object passes the kernel semaphore suite (mps2/an385 qemu) —
functional equivalence with the native-FFI build. Release-readiness for the wasm lane =
`release-wasm.yml` green + the manifest's falsifiable claims verified (see the
distribution doc §Falsifiable claims).
