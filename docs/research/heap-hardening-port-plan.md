# SYS_HEAP_HARDENING Port Plan

**Issue:** gh #15 — `gale_heap.c` lacks the Zephyr 4.4 runtime hardening.
**Scope:** This is the scoping doc. A draft BASIC-tier patch is attempted in
`heap-hardening-draft` (see Part B at the end).

## 1. Upstream feature inventory

Upstream file: `/Users/r/git/pulseengine/z/zephyr/lib/heap/heap.c` (v4.4.0-rc3, 794 lines).
Header: `/Users/r/git/pulseengine/z/zephyr/lib/heap/heap.h` (350 lines).

Level predicates are defined in `heap.h:23-30` as compile-time `#define`s that
reduce to `CONFIG_SYS_HEAP_HARDENING_LEVEL >= N`. Dead branches fold at `-O2`.

### BASIC tier (`LEVEL >= 1`) — cheap double-free + overflow

- `heap.c:284-287` — `!chunk_used(h, c)` double-free check in `sys_heap_free`.
- `heap.c:305-309` — `left_chunk(right_chunk(c)) != c` round-trip check in `sys_heap_free`.
- `heap.c:556-559` + `560-564` — same two checks in `inplace_realloc`.

BASIC adds ~20 lines of C, zero bytes per chunk, zero new data members.

### MODERATE tier (`LEVEL >= 2`) — left-neighbor + free-list linkage

- `heap.c:102-107` — free-list linkage sanity in `free_list_remove_bidx`.
- `heap.c:146-150` — same in `free_list_add_bidx`.
- `heap.c:178-204` — `free_chunk_check` helper: `chunk_used` / left-right linkage.
- `heap.c:317-321` — `right_chunk(left_chunk(c)) != c` in `sys_heap_free`.
- `heap.c:565-569` — same in `inplace_realloc`.
- Callers of `free_chunk_check`: `free_chunk` (`:239,248`), `alloc_chunk`
  (`:390,408`), `inplace_realloc` (`:607,638`).

MODERATE adds ~40 lines cumulative over BASIC. Still zero bytes per chunk.

### FULL tier (`LEVEL >= 3`) — per-chunk trailer canaries

- `heap.h:73-85` — `struct z_heap_chunk_trailer { uint64_t canary; }`, 8-byte
  trailer, gated by `CONFIG_SYS_HEAP_CANARIES` (auto-selected by `_FULL`).
- `heap.h:326-338` — `chunk_trailer()` accessor.
- `heap.h:79` — `CHUNK_TRAILER_SIZE` enters `bytes_to_chunksz`, `min_chunk_size`,
  `chunk_usable_bytes`, `undersized_chunk` (`:260-287`). This is the load-bearing
  layout change: every size computation gains a trailer slot.
- `heap.c:28-75` — `HEAP_CANARY_MAGIC/_POISON`, `compute_canary`,
  `set_chunk_canary`, `verify_chunk_canary`, `poison_chunk_canary`.
- `heap.c:200-203,252-259` — `free_chunk_check` canary verify (left-neighbor).
- `heap.c:323-326,440-442,530-532,596-598,648-650,780-782` — `verify`/`set`/
  `poison_chunk_canary` scattered across free/alloc/aligned\_alloc/realloc/init.
- `heap.c:351-353` — canary verify in `sys_heap_usable_size`.
- `heap.c:514` — `CHUNK_TRAILER_SIZE` in `aligned_alloc`'s `c_end` math.

FULL adds ~60 lines plus **8 bytes/chunk** layout change plus every size
arithmetic site that currently omits `CHUNK_TRAILER_SIZE`.

### EXTREME tier (`LEVEL >= 4`)

- `heap.c:328-331,360-363,573-576` — calls `z_heap_full_check` (defined in
  `heap_validate.c`, pulled in by `CONFIG_SYS_HEAP_VALIDATE` which EXTREME selects).
  ~9 lines.

## 2. Gap against `zephyr/gale_heap.c`

Gale shim file: `/Users/r/git/pulseengine/z/gale/zephyr/gale_heap.c` (576 lines).
**Every hardening hook is absent.** Concrete deltas:

| Upstream | Gale shim location | What to change |
|---|---|---|
| double-free check | `gale_heap.c:213-227` (free path) | Rust decision already rejects when `is_used==0`; add `SYS_HEAP_HARDENING_BASIC` gate around the extract (no Rust change) |
| overflow round-trip | `gale_heap.c:214` (`bounds_ok`) | Already extracted; gate under BASIC |
| left-neighbor check | `gale_heap.c:215-216` | Add `right_chunk(left_chunk(c)) != c` extraction; extend `gale_heap_free_decision` with a MODERATE flag or keep the check C-side |
| `free_list_remove_bidx` linkage | `gale_heap.c:80-103` | Add upstream's MODERATE-gated check at `:95` |
| `free_list_add_bidx` linkage | `gale_heap.c:115-141` | Add upstream's MODERATE-gated check at `:130` |
| `free_chunk_check` helper | not present | New static; call from `free_chunk` (`:180-196`), `alloc_chunk` (`:253-286`), `inplace_realloc` (`:406-483`) |
| canary trailer type | `gale_heap.c` uses `heap.h` — nothing to add in shim, but all `bytes_to_chunksz`/`chunk_usable_bytes` calls already read the header macros and will auto-pick up `CHUNK_TRAILER_SIZE` |
| `set/verify/poison_chunk_canary` | not present | Copy verbatim from `heap.c:39-74`; insert at `alloc:316`, `aligned_alloc:391`, `init:566`, `free:229`, `usable_size:250`, both realloc branches (`:438,470`) |
| `aligned_alloc` `c_end` arith | `gale_heap.c:376` — `c_end = end - chunk_buf(h)` | Upstream adds `+ CHUNK_TRAILER_SIZE`; must mirror |

## 3. Proof impact on `src/heap.rs`

`src/heap.rs` models **aggregate chunk accounting** only: `capacity`,
`allocated_bytes`, `total_chunks`, `free_chunks`, `next_slot_id` (lines 88-102).
It does **not** model per-chunk byte layout, trailer bytes, or canary state.

**BASIC and MODERATE: no proof impact.** All checks are structural (use-flag,
linkage round-trips) and happen before the extract-decide-apply boundary. The
existing `gale_sys_heap_free_decide` already rejects `is_used==0`, which is
exactly what BASIC does. Adding MODERATE left-neighbor extraction changes the
C-side witness computation but not the decision predicate.

**FULL: indirect proof impact.** Adding `CHUNK_TRAILER_SIZE` to `bytes_to_chunksz`
inflates every `chunks_need` computation by 1. The Rust side receives
`chunk_size(h, c)` and `chunks_need` directly from C, so the Rust model is still
sound — but two ensures clauses deserve audit:

- `Heap::alloc` ensures `self.allocated_bytes == old(self).allocated_bytes + bytes`
  (heap.rs:203). The `bytes` parameter in the proof is the *user-requested size*,
  not chunk size, so this still holds. But `increase_allocated_bytes` in the C
  shim (gale_heap.c:321) passes `chunksz_to_bytes(h, chunk_size(h, c))` which now
  includes trailer bytes. The Rust accounting will drift vs. C accounting once
  FULL is enabled. **Action: re-frame `allocated_bytes` to "reserved bytes
  including trailer" or split into `user_bytes` vs. `reserved_bytes`.**
- `Heap::aligned_alloc` computes `padding = align - CHUNK_UNIT` (heap.rs:332).
  With trailers, the worst-case padding is `align - CHUNK_UNIT + TRAILER_BYTES`.
  **Action: flag as a precision gap; the check is still overflow-safe but less
  tight.**

No `ensures` clause needs to change for BASIC/MODERATE. Do not modify proofs
in this port — land BASIC/MODERATE first, open a separate proof-refresh issue
for FULL.

## 4. Kconfig wiring

Upstream symbols (from `zephyr/lib/heap/Kconfig:80-155`):
- `SYS_HEAP_HARDENING_{NONE,BASIC,MODERATE,FULL,EXTREME}` — choice group.
- `SYS_HEAP_HARDENING_LEVEL` — int 0-4, derived.
- `SYS_HEAP_CANARIES` — hidden, auto-selected by FULL/EXTREME.

`CONFIG_GALE_KERNEL_HEAP` in gale's Kconfig (`zephyr/Kconfig:312-323`) does
**not** currently depend on or suppress these symbols. Because upstream's
symbols live in `zephyr/lib/heap/Kconfig`, they are defined unconditionally and
produce valid `CONFIG_SYS_HEAP_HARDENING_LEVEL` macros regardless of the Gale
guard. The gale shim just ignores the resulting `#defines`.

No overlay change required for BASIC/MODERATE once the C code honors the
macros — users who set `CONFIG_SYS_HEAP_HARDENING_MODERATE=y` will get it.
For FULL, the layout change via `CHUNK_TRAILER_SIZE` must be picked up by
`heap.h` (it already is; gale_heap.c includes `heap.h`).

Add to `gale_overlay.conf` (comment-only, for documentation):

```
# sys_heap hardening is honored when CONFIG_GALE_KERNEL_HEAP=y:
#   BASIC/MODERATE — free-path structural checks (zero overhead bytes)
#   FULL           — adds 8-byte trailer per chunk + canary compute
# Default inherits from ASSERT (MODERATE) or BASIC, per upstream Kconfig.
```

## 5. Port order (5 PRs)

1. **PR-1: BASIC tier, no layout change.** Add the two `sys_heap_free` guards
   (`:284,305`) and the two `inplace_realloc` guards (`:556,560`) behind
   `SYS_HEAP_HARDENING_BASIC`. ~20 C lines, zero Rust change. CI stays green
   because the macro collapses to 0 at default level. **Part B below attempts
   exactly this.**

2. **PR-2: MODERATE tier linkage checks.** Add the two free-list linkage guards
   (`:102,146`), the right/left round-trip in `sys_heap_free` (`:317`) and
   `inplace_realloc` (`:565`). ~10 C lines. Still no layout change.

3. **PR-3: `free_chunk_check` helper.** Port the helper verbatim (minus
   `verify_chunk_canary` — that's FULL), wire into `free_chunk`, `alloc_chunk`,
   `inplace_realloc`. ~30 C lines. Completes MODERATE coverage.

4. **PR-4: FULL canary infrastructure.** Copy canary helpers (`compute_canary`,
   `set_chunk_canary`, `verify_chunk_canary`, `poison_chunk_canary`), add the
   `aligned_alloc` `c_end + CHUNK_TRAILER_SIZE` fix, set on alloc paths, verify
   on free/realloc/usable\_size. ~60 C lines. **Blocks on Rust proof refresh
   (see §3).**

5. **PR-5: EXTREME + proof sync.** Wire `z_heap_full_check` calls; re-frame
   `Heap::allocated_bytes` in `src/heap.rs` to include trailer bytes; update
   `Heap::aligned_alloc` padding spec. Separate issue.

Each PR is independently bisectable and independently CI-green.

## 6. Risks

- **Binary size:** BASIC/MODERATE add ~50 C lines (~200-400 bytes of `.text`
  worst case, much less after dead-code elim at `LEVEL=0`). FULL adds 8 bytes
  per allocation — on a 32-chunk app heap that's 256 bytes of RAM, non-trivial
  on Cortex-M0.
- **Performance:** MODERATE adds ~4 loads per alloc and per free (three field
  reads + one compare). FULL adds one XOR + one 64-bit compare per alloc/free.
  The `-O2` branch predictor absorbs this; measured overhead is typically <2%
  on stress tests (upstream commit message claim — not independently verified).
- **Proof drift:** The largest risk. FULL quietly changes the meaning of
  `allocated_bytes` because trailers count against capacity. If PR-4 lands
  before PR-5's proof refresh, the Rust model's HP1 bound becomes imprecise
  but not unsound (still `allocated <= capacity`). **Gate PR-4 behind PR-5.**
- **Verification churn:** `gale_sys_heap_aligned_alloc_decide` takes
  `chunk_header_bytes` (4 or 8) today; if we want tight proofs with trailers
  it should also take `chunk_trailer_bytes` (0 or 8). Breaks the FFI
  signature — mark as a future refactor, do not ship inside this series.
- **Silent feature loss:** Until PR-1 lands, enabling `CONFIG_GALE_KERNEL_HEAP=y`
  still silently disables hardening. Documenting that in `gale_overlay.conf`
  (item 4 above) is the minimum-viable mitigation and should accompany PR-1.

## Part B: BASIC-tier draft

**Status: attempted.** PR-1 is small enough (~20 line diff) for a one-pass
agent edit. The draft lives on throwaway branch `heap-hardening-draft`;
uncommitted diff is in the working tree of `/Users/r/git/pulseengine/z/gale/`
for human review. Full integration build (`west build`) was not run — this
agent sandbox cannot reach the Zephyr SDK. Syntax check only; flag a full
build pending before merge.
