# `cabi-realloc-extern` recovers 318 B of the componentization overhead (2026-08-04)

REQ-DRV-COMPONENT-001 cost the thin drivers roughly **+700–1100 B of `.text`
each** for canonical-ABI glue (loom#303). Part of that is `cabi_realloc` and the
global growing allocator behind it — visible in the dissolved object as
`T cabi_realloc` and `T cabi_realloc_wit_bindgen_0_52_0`.

Our own wit-bindgen fork already carries the fix. On branch
`integration/embedded-rt-no-grow`, the `cabi-realloc-extern` feature makes
`cabi_realloc` delegate to an embedder-provided `__cabi_arena_realloc` over a
bounded arena that **traps on exhaustion instead of calling `memory.grow`**.
`crates/gale-kiln` and `crates/gale-app-demo` already depend on it. **The gust
drivers do not** — they use stock `wit-bindgen = "0.52"` from crates.io plus a
`NoAlloc` trapping global allocator as a workaround.

## Measured, on `wdg-thin`

Same source, same `loom optimize --passes inline`, same
`synth compile --target cortex-m3 --all-exports --relocatable`:

| build | `.text` |
|---|---|
| stock `wit-bindgen 0.52` (what the drivers ship today) | 1726 B |
| fork branch, `cabi-realloc-extern` **off** | 1746 B |
| fork branch, `cabi-realloc-extern` **on** | **1428 B** |

**The feature is worth −318 B (−18.2%).** The middle row is the control and it
matters: the fork is at 0.58, so a naive 0.52-vs-patched comparison would have
credited the feature with −298 B while the version bump alone *costs* +20 B.
Against the +1088 B this driver pays to be a component, −318 B is **29% of the
overhead recovered**.

`.data`/`.bss` stay 0 → 0. The undefined-symbol seam is unchanged
(`read32`, `write32`) — the arena provider is a local definition, so it adds no
TCB atom.

## What it costs to adopt

The world must provide `__cabi_arena_realloc` honouring the canonical realloc
contract. For a scalar-only world (u32 in, u32 out) nothing can legally call it,
so the provider is the zero-size case plus a trap:

```rust
#[no_mangle]
pub unsafe extern "C" fn __cabi_arena_realloc(
    _old: *mut u8, old_len: usize, align: usize, new_len: usize,
) -> *mut u8 {
    if old_len == 0 && new_len == 0 { return align as *mut u8; }
    core::ptr::null_mut()   // failure -> trap; this world never allocates
}
```

## Status — not adopted

Measured in a scratch copy (`/tmp/wdgx`), **not landed**. Adopting it means
moving all nine thin drivers onto the fork branch and giving each world an arena
provider, which also moves them 0.52 → 0.58. That is a real change with a real
regression surface (every driver's dissolved object and symbol contract gets
re-pinned), so it is proposed here rather than done quietly.

Reproduce: copy a driver, switch its `wit-bindgen` dependency to
`{ git = "https://github.com/pulseengine/wit-bindgen", branch = "integration/embedded-rt-no-grow", default-features = false, features = ["macros", "cabi-realloc-extern"] }`,
add the provider above, rebuild and dissolve.
