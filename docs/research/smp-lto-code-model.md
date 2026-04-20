# SMP x86_64 LTO Code Model Conflict — Research Brief

Research into the LLVM cross-language LTO failure that breaks the three
SMP Zephyr tests (`smp_threads`, `smp_semaphore`, `smp_mutex`) on
`qemu_x86_64` via the `llvm-lto-test` matrix. The failure manifests during
`gale-ffi` Rust compilation with:

```
warning: linking module flags 'Code Model': IDs have conflicting values:
  'i32 2' from , and 'i32 1' from gale_ffi.df12e0ab079ecb2c-cgu.0
error: failed to load bitcode of module
  "core-fcc82a7537a7267a.core.bb170dea1013a3a3-cgu.0.rcgu.o"
```

LLVM `Code Model` flag values: 1 = Small, 2 = Kernel, 3 = Medium, 4 = Large.

## Root cause confirmation

- Gale already overrides the Rust code model for x86 targets at
  `/Users/r/git/pulseengine/z/gale/zephyr/CMakeLists.txt:90-102`: for any
  `CONFIG_X86` it injects `-Crelocation-model=static -Ccode-model=small`
  into `RUSTFLAGS`. The comment at lines 90–94 explicitly states the
  `x86_64-unknown-none` target defaults to `code-model=kernel` and this
  must be overridden because Zephyr maps the kernel in the lower half.
- The Rust target spec `x86_64-unknown-none` sets
  `code_model: Some(CodeModel::Kernel)`, documented at
  https://doc.rust-lang.org/beta/rustc/platform-support/x86_64-unknown-none.html
  and discussed in rust-lang/rust#101209.
- Zephyr x86_64 does NOT set `-mcmodel=kernel`. `arch/x86/intel64.cmake`
  only adds `-m64 -mno-red-zone`, so clang uses the GCC default
  (`-mcmodel=small`) on `qemu_x86_64`. The `kernel` side of the conflict
  therefore originates entirely from Rust's precompiled `libcore.rlib`
  (compiled with `code_model=Kernel` when the stdlib team shipped it),
  not from Zephyr C code.
- The failure is: `gale-ffi` is compiled with `-Ccode-model=small`
  (Gale's override) AND `-Clinker-plugin-lto -Cembed-bitcode=yes`
  (`gale_lto_overlay.conf`, only relevant to the `llvm-lto-test` job).
  The precompiled `core.rlib` bitcode still carries `Code Model = 2`
  (Kernel). When lld merges Rust bitcode during LTO, `core` (2) vs
  `gale_ffi` (1) clash. This exactly matches the pattern in
  rust-lang/rust#139479 (same bug on RISC-V medium vs small).
- Note: the default `zephyr-tests.yml` SMP job (matrix `smp_*`) uses
  `gale_smp_overlay.conf` WITHOUT `gale_lto_overlay.conf`, so it should
  not hit this path. The error is reproducible from the `llvm-lto-test`
  matrix when someone adds x86_64 SMP to it (the prompt describes the
  failure as LTO-specific; stock SMP x86_64 has a separate hang issue
  flagged at `.github/workflows/zephyr-tests.yml:273-275`).

## Options

- **Option 1 — Match Rust's kernel model for LTO only on x86_64 + SMP.**
  In `zephyr/CMakeLists.txt:95`, drop the x86_64 `code-model=small`
  override when `CONFIG_LTO=y` AND `CONFIG_SMP=y`, letting `gale-ffi`
  inherit the Rust default (`kernel`). Risk: Zephyr C is still compiled
  with small model, so cross-language inlining would mix models. lld
  would merge at `kernel` (matching `core`), but small-model C
  relocations (R_X86_64_32) break if the kernel lands above 2 GB.
  Zephyr x86_64 currently links in the lower half, so this would
  probably NOT work without also relocating the image. Tractable only
  with a kernel-relocation change. Not recommended.

- **Option 2 — Force both Zephyr C and Rust to `code-model=small` and
  rebuild `core` via `-Zbuild-std`.** Add `-Zbuild-std=core` +
  target-specific `rustflags = ["-Ccode-model=small"]` for
  `x86_64-unknown-none` in `.cargo/config.toml` (or via env in
  `CMakeLists.txt`). This would rebuild `core` with matching model so
  LTO merges cleanly. Risks: requires nightly (`-Z` flags), breaks the
  stable-toolchain rule in `rust-toolchain.toml:2`. Tractable if we
  accept a nightly-only LTO path; otherwise not.

- **Option 3 — Disable cross-language LTO for x86_64 SMP only.** In
  `CMakeLists.txt:20`, add `AND NOT (CONFIG_X86 AND CONFIG_64BIT)` to
  the guard so `-Clinker-plugin-lto` is skipped on x86_64. SMP tests
  still run, just without cross-language LTO (Rust-internal LTO via the
  stock `release` profile still applies). Pragmatic, low risk, loses
  some inlining on one target. Tractable today, stable-toolchain
  compatible.

- **Option 4 — Use `x86_64-unknown-linux-none` or a custom target JSON
  with `code-model=small` baked in.** The Tier 3 target
  `x86_64-unknown-linux-none` (and custom JSON specs referenced in
  rust-lang/rust#101209 discussion) ship `core` with the small model.
  Risks: Tier 3 means no `rustup target add`, requires `-Zbuild-std`
  anyway, or a custom-built `core`. Nightly-only, brittle. Not
  recommended for a stable toolchain.

- **Option 5 — Upstream fix to `x86_64-unknown-none` spec.** Change
  Rust's default for this target from `Kernel` to `Small`, matching
  community expectation (see discussion in rust-lang/rust#101209 — many
  users overriding to `small`). Out of scope for Gale, long timeline.

## Prior art

- rust-lang/rust#101209 —
  https://github.com/rust-lang/rust/issues/101209 —
  "x86_64-unknown-none uses code-model kernel but statically linking to
  high address results in R_X86_64_32 out of range." Confirms the Rust
  default is `kernel`, describes the exact override (`RUSTFLAGS="-C
  code-model=..."`). Open since 2022, no consensus fix.

- rust-lang/rust#139479 —
  https://github.com/rust-lang/rust/issues/139479 —
  "Code model other than medium does not work with lto on riscv64."
  Same LLVM bitcode module-flag conflict pattern (small vs medium on
  RISC-V), demonstrates this is a cross-arch LLVM limitation when the
  precompiled `core.rlib` is built with a different code model than the
  user crate. No fix upstream.

- Linux kernel `rust/Makefile` —
  https://github.com/torvalds/linux/blob/master/rust/Makefile — sets
  `-C code-model=kernel -C relocation-model=static` for x86_64 and
  builds its own `core` (via `rustc` rebuild) to match, sidestepping
  the rlib mismatch. This is the Option 2 / Option 4 hybrid at kernel
  scale; they can afford the nightly-only rebuild path.

- rustc book, linker-plugin-LTO —
  https://doc.rust-lang.org/rustc/linker-plugin-lto.html — documents
  that all modules (including stdlib rlibs) participating in LTO must
  agree on module flags. Does not mention code-model as an LTO hazard
  explicitly; the conflict surfaces at link time only.

- `-C code-model` flag status: documented in the rustc book Codegen
  Options chapter
  (https://doc.rust-lang.org/rustc/codegen-options/index.html) as a
  stable flag. I could not find a dedicated tracking issue number
  confirming the exact stabilization version — treat as "stable but
  underspecified."

## Recommendation

**Option 3: disable cross-language LTO for x86_64 on the `llvm-lto-test`
matrix.** It is a one-line guard change in `zephyr/CMakeLists.txt:20`
(add `CONFIG_X86 AND CONFIG_64BIT` as a negative condition), stays on
the stable toolchain, and loses only cross-language inlining on one
target where SMP tests currently `continue-on-error` anyway. Options 2
and 4 require moving Gale off stable Rust, Option 1 requires relinking
the Zephyr kernel to the high half, Option 5 is upstream. Option 3
unblocks the SMP tests today and leaves cross-language LTO fully
functional on the ARM Cortex-M/R targets that carry the release-quality
LTO story.
