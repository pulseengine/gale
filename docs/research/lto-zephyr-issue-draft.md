# Draft: Zephyr upstream issue — cross-language LTO

Status: **draft, not submitted yet**. Pending prior-art research.
Authored 2026-04-26.

---

## Title

> Cross-language LLVM LTO with Rust on bare-metal ARM: three integration gaps

## Body

In [pulseengine/gale](https://github.com/pulseengine/gale) we recently got cross-language LLVM Link-Time Optimisation working end-to-end between formally-verified Rust (Verus) and Zephyr's C kernel on Cortex-M3 with `ZEPHYR_TOOLCHAIN_VARIANT=llvm` and `-flto=thin` on both sides. Final result on hosted CI: **0 surviving Rust FFI shim symbols** in the linked ELF — every Rust decision function is inlined directly into its C caller's basic block, no `bl <gale_*>` instructions remain.

Getting there hit three barriers that weren't obvious from documentation and cost several days of debugging. Two of them are straightforwardly fixable in the Zephyr tree; the third is more of an FFI-design topic that probably wants a documentation note. Filing this in case the maintainers are interested in upstream fixes — happy to send PRs if there's appetite.

### Barrier 1 — Zephyr's clang toolchain doesn't honour `CONFIG_LTO`

Under `ZEPHYR_TOOLCHAIN_VARIANT=llvm + CONFIG_LTO=y`, the CMake property `optimization_lto` expands to the empty string. `cmake/compiler/gcc/compiler_flags.cmake` (lines ~27–28) sets it to `-flto=auto`; `cmake/compiler/clang/compiler_flags.cmake` does not override the default empty list. Same on the linker side: `cmake/linker/lld/linker_flags.cmake` explicitly clears `lto_arguments` / `lto_arguments_st`.

The result is that under the LLVM variant, `CONFIG_LTO=y` accepts the config but passes no `-flto` flag to clang or lld. C `.o` files are emitted as plain ARM ELF, not LLVM bitcode. With no bitcode on the C side, the linker has nothing to inline a (correctly-bitcoded) Rust static archive against, and every Rust FFI shim survives the link as an externally-visible symbol. There is no warning.

Fix proposal: extend `cmake/compiler/clang/compiler_flags.cmake` and `cmake/linker/lld/linker_flags.cmake` to set `optimization_lto` / `lto_arguments` analogously to the GCC paths — probably `-flto=thin` for clang since ThinLTO is the recommended mode for cross-language LTO. Our local workaround injects this from a downstream module — see [`gale/zephyr/CMakeLists.txt`](https://github.com/pulseengine/gale/blob/main/zephyr/CMakeLists.txt) (the `if(... STREQUAL "llvm" AND CONFIG_LTO)` block).

### Barrier 2 — function attribute mismatch between rustc and clang silently blocks inlining

Once bitcode flows on both sides, LLVM's `TargetTransformInfo::areInlineCompatible` still rejects the cross-language inline at every call site. The reason is that rustc and clang emit *different* `target-cpu` and `target-features` attributes on every function, even when the underlying target triple matches:

| Side | `target-cpu` | `target-features` |
|---|---|---|
| C (clang for `cortex-m3`) | `cortex-m3` | `+armv7-m,+hwdiv,+soft-float,+thumb-mode,-aes,…` (~50 entries) |
| Rust (rustc for `thumbv7m-none-eabi`) | `generic` | _absent_ |

The inliner's compatibility check requires callee features ⊆ caller features. The build succeeds, no warning fires, and every cross-language inline silently doesn't happen.

The fix on the Rust side is `RUSTFLAGS=-Ctarget-cpu=cortex-mN -Ctarget-feature=…` matching clang's strict subset (rustc doesn't recognise some of clang's features like `armv7-m` since they're implied by the target spec). It would be valuable if Zephyr's Rust integration (or a documented helper) computed and exported this automatically when both `ZEPHYR_TOOLCHAIN_VARIANT=llvm` and a Rust component are present. Same workaround file as above carries the per-CPU `-Ctarget-cpu` mapping.

### Barrier 3 — `#[repr(C)]` struct returns lower to incompatible `sret` types

This one isn't a Zephyr bug; it's a cross-language ABI gotcha that probably deserves a note in the Rust-on-Zephyr integration documentation. For an FFI function returning a `#[repr(C)]` struct on bare-metal ARM:

- rustc emits `void @fn(ptr … sret([N x i8]) …)` — opaque byte-array sret
- clang emits `void @fn(ptr … sret(%struct.X) …)` — named-struct sret

The LLVM inliner refuses to merge call sites whose `sret` argument types disagree, even when the underlying bytes are semantically identical. This is conservative behaviour preserving semantics, but it means *every* `sret`-returning FFI shim quietly fails to inline under LTO.

The workable fix is at the FFI level: avoid `sret` returns. Pack ≤ 8-byte structs into `uint64_t` so they return via the AAPCS r0/r1 register pair (no `sret` involved, both sides emit `i64` IR). Larger structs need redesign — drop fields the caller can derive (e.g. `new_used = old_used ± actual_bytes`), split single-`ERROR` actions into per-error-code variants where the caller needs the distinction. We redesigned three decision structs in gale to land this.

This is a footgun for anyone trying cross-language LTO with `#[repr(C)]`-style FFI on bare-metal ARM. A short note in the Rust-on-Zephyr integration docs — *"prefer scalar return types over `sret` struct returns; pack into `uint64_t` for ≤ 8-byte payloads"* — would save the next person several days.

### Evidence

All three barriers are documented with reproducible commits and CI evidence in [pulseengine/gale#10](https://github.com/pulseengine/gale/issues/10) (now closed). The closing comment includes the full archaeology trail. Final CI run on commit [`6b3dc0f`](https://github.com/pulseengine/gale/commit/6b3dc0f): GCC + Gale 10 surviving `gale_` symbols, LLVM no-LTO 10, **LLVM + Gale + LTO 0**.

### Asks

- Interest in PRs for Barrier 1 (the clang/lld toolchain plumbing fix)?
- Open to a `cmake/compiler/clang/` parallel of the GCC `optimization_lto` path?
- Any preferred shape for the documentation note around Barrier 3?

Happy to split this into separate focused issues or PRs as the maintainers prefer.

---

## Submission notes

- File against: `zephyrproject-rtos/zephyr` (Issues tab)
- Submit from: Ralf's account (carries authorship for archival)
- Optional labels: `area: Build System`, `area: Toolchains`, `RFC` (set by submitter; maintainers re-label)
- US-timezone advantage: submit late evening Europe → US maintainers see it top-of-queue next morning

## Pending before submission

- [x] Search for prior art — done. No prior issue/PR on this topic in
      `zephyrproject-rtos/zephyr` or `zephyr-lang-rust`. The two adjacent
      LTO PRs (#99124, #100034) are GCC-only.
- [x] Adjacent active PR identified: #102286 ("doc: update Enabling Rust
      Support guide with west.yml and clang"), author Dooleweerdt,
      reviewer d3zd3z. Open since 2026-01-14, in iteration. Author
      explicitly asked on 2026-04-15 about SDK clang vs brew LLVM on
      macOS — exactly our environment question.
- [ ] Submit issue (A)
- [ ] Then submit comment (B) on #102286 referencing the issue number

---

## Companion: comment to drop on #102286 (after the issue is filed)

The author of #102286 asked on 2026-04-15 whether the Zephyr SDK 1.0
clang is sufficient on macOS or whether brew LLVM is needed. We answered
this question by hitting it ourselves — so a brief data point would help
that PR's iteration without trying to expand its scope.

Use as comment after the LTO issue is filed (replace `#NEW` with the
real number):

> Late to this thread but a data point on the open question about SDK
> clang vs brew LLVM on macOS:
>
> In [pulseengine/gale](https://github.com/pulseengine/gale) we use both
> for different purposes:
>
> - **Basic Rust + Zephyr build (no LTO)**: Zephyr SDK 1.0.x clang
>   works fine on macOS aarch64.
> - **Cross-language LLVM LTO** (Rust ↔ C inlining across the FFI
>   boundary): the SDK clang doesn't ship `ld.lld`, so we use Homebrew
>   LLVM 22.x (`brew install llvm lld`) with matching rustc 1.95 (LLVM
>   22.1.2). Versions need to match for the linker plugin to accept
>   rustc-emitted bitcode.
>
> So for this PR's scope (the "Enabling Rust Support" guide), I think
> SDK clang is sufficient and the doc can stay focused on the basic
> setup. Filed zephyrproject-rtos/zephyr#107948 separately with the
> LTO-specific plumbing details (clang `optimization_lto` injection,
> `target-cpu`/`target-features` matching, `sret` ABI quirks) — that's
> a deeper rabbit hole than what this PR is trying to cover. Happy to
> provide more detail on the LTO setup if it ever becomes relevant to
> a future "advanced topics" section, but I don't think it should block
> this PR.

Notes on this comment:

- **Answers the open question first** (SDK vs brew), gives concrete
  versions, doesn't make readers guess.
- **Explicitly says it shouldn't block this PR** — anti-scope-creep
  signal so reviewers don't worry we're trying to redirect their work.
- **References the new issue with a placeholder** — fill in once A is
  filed.
- **No demands, no asks** — pure data + a polite handoff.
- **David Brown (`d3zd3z`) is the Rust support maintainer** and active
  reviewer on this PR; he'll see the comment and the cross-reference
  to our issue, which puts the LTO findings on his radar without us
  having to tag him.
