# Finding: mem_domain FFI hardcodes 16-slot array scan — OOB read / silent invariant violation vs. `CONFIG_MAX_DOMAIN_PARTITIONS`

- **File**: `/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs`
- **Functions**:
  - `gale_mem_domain_check_partition` (lines 5443–5482)
  - `gale_k_mem_domain_add_partition_decide` (lines 5516–5575)
  - `gale_k_mem_domain_remove_partition_decide` (lines 5606–5656)
- **Prior matched**: *size / alignment assumption on C-side struct that Rust treats as known* (also ties to proof-code drift and the STPA GAP-2 audit entries for mem_domain)
- **Oracle**: Kani (preferred) + differential C→Rust test

## Defect

`MEM_DOMAIN_MAX_PARTITIONS` is a Rust-side `const u32 = 16` (line 5414). The three FFI functions above iterate `while i < MEM_DOMAIN_MAX_PARTITIONS { ... *domain_starts.add(i as usize) ... *domain_sizes.add(i as usize) ... }` inside `unsafe`, with no length parameter. The actual length is the C-side `CONFIG_MAX_DOMAIN_PARTITIONS`, which Zephyr defines as:

```
# zephyr/kernel/Kconfig.mem_domain
config MAX_DOMAIN_PARTITIONS
    int "Maximum number of partitions per memory domain"
    default 16
    range 0 $(UINT8_MAX)           # 0..255
```

And multiple SoCs override it:

- `zephyr/soc/arm/fvp_aemv8r/Kconfig.defconfig`: `default 8 if SOC_FVP_AEMV8R_AARCH64`
- `zephyr/soc/arm/fvp_aemv8r/Kconfig.defconfig`: `default 24 if USERSPACE && SOC_FVP_AEMV8R_AARCH32`
- `zephyr/soc/snps/nsim/arc_classic/hs/Kconfig.defconfig.hs_mpuv6`: `default 32 if USERSPACE`

The C caller (`zephyr/gale_mem_domain.c`) honours Kconfig and passes arrays sized to `CONFIG_MAX_DOMAIN_PARTITIONS`:

```c
// lines 49-57 of gale_mem_domain.c
static void extract_partition_arrays(const struct k_mem_domain *domain,
                                     uint32_t starts[CONFIG_MAX_DOMAIN_PARTITIONS],
                                     uint32_t sizes[CONFIG_MAX_DOMAIN_PARTITIONS])
{
    for (int i = 0; i < CONFIG_MAX_DOMAIN_PARTITIONS; i++) {
        starts[i] = (uint32_t)domain->partitions[i].start;
        sizes[i]  = (uint32_t)domain->partitions[i].size;
    }
}
```

And the `_num_partitions` parameter Rust *could* use to bound its loop is explicitly dropped on the floor — note the underscore in `gale_mem_domain_check_partition`:

```rust
pub extern "C" fn gale_mem_domain_check_partition(
    part_start: u32,
    part_size: u32,
    domain_starts: *const u32,
    domain_sizes: *const u32,
    _num_partitions: u32,          // <-- ignored
) -> GaleMemDomainCheckPartitionDecision {
    ...
    let mut i: u32 = 0;
    while i < MEM_DOMAIN_MAX_PARTITIONS {     // <-- always 16
        unsafe {
            let dsize = *domain_sizes.add(i as usize);
            ...
```

### Two concrete failure modes

**(A) OOB read — UB + false rejection (CONFIG < 16).**
On `fvp_aemv8r` AArch64 (`CONFIG_MAX_DOMAIN_PARTITIONS = 8`) the C arrays are 8 × u32 on the caller's stack. Rust reads `domain_starts.add(8..15)` and `domain_sizes.add(8..15)` — 8 u32 words past the end of the caller's stack buffer. This is undefined behavior in Rust (`ptr::add` past one-past-end of the allocation is UB regardless of dereference), and in practice reads whatever sits immediately after `sizes[]` on the C stack (padding, another local, saved LR, canary, …). If any of those aliased stack words happens to be non-zero (taken as `dsize > 0`), `gale_mem_domain_check_partition` then calls `partitions_overlap_decide` with a junk start/size pair and can spuriously return `EINVAL` — i.e. refuse a perfectly valid `k_mem_domain_add_partition` based on stack garbage. In `gale_k_mem_domain_remove_partition_decide`, a junk read matching `(part_start, part_size)` would return `ret = OK, slot = <OOB index>`; the C shim then writes back `domain->partitions[slot]` at a slot >= CONFIG_MAX_DOMAIN_PARTITIONS, i.e. past the end of `k_mem_domain::partitions[]`, because `slot` is returned unvalidated against `CONFIG_MAX_DOMAIN_PARTITIONS`. That path converts the OOB read into a controllable OOB *write* into the neighbouring fields of `struct k_mem_domain` (`thread_mem_domain_list`, `num_partitions`, or arch data), which is a classic isolation-domain-escape primitive on a security-critical USERSPACE boundary.

**(B) Silent invariant loss — CONFIG > 16.**
On ARC (`CONFIG_MAX_DOMAIN_PARTITIONS = 32`) or FVP AArch32 (24), Rust only inspects the first 16 slots. Any partition in slots 16..N is invisible to the overlap check, so `gale_mem_domain_check_partition` and `gale_k_mem_domain_add_partition_decide` can accept a new partition that *does* overlap one already in slot ≥ 16. The Verus property MD1 ("partitions don't overlap") is advertised as verified by this function (docstring: `Verified: MD1, MD3, MD6`) but the actual emitted code silently skips the check it claims to perform. This is the "proof-code drift" the STPA GAP-2 audit flags for these three functions (audit table line 314, "Overlap detection loop over 16 partitions with u64 arithmetic. Model has `add_partition()`"). The model only inspects whatever the FFI passes it, so the Verus proof is vacuously satisfied over the 16 visible slots while partitions 16..31 violate the invariant at runtime.

### Why the existing Kani proofs miss it

`kani_mem_domain_proofs` (lines 7403–7491) hardcodes every harness with `let starts = [0u32; 16]; let sizes = [0u32; 16];`. The arrays happen to be exactly 16, so the OOB read never goes past their stack allocation and Kani reports clean. The harnesses test the algorithm on a fixed-size input, not the FFI contract across varying C-side array lengths. No proof currently varies `MEM_DOMAIN_MAX_PARTITIONS` vs. the actual passed array size.

## Oracle — Kani harness that reproduces both failure modes

```rust
#[cfg(all(kani, feature = "mem_domain"))]
mod kani_mem_domain_boundary_proofs {
    use super::*;

    /// Case (A): caller supplies an 8-entry array (CONFIG_MAX_DOMAIN_PARTITIONS=8).
    /// The FFI must not read past index 7. Kani will flag the pointer
    /// arithmetic .add(8..15) as out-of-bounds of the allocation.
    #[kani::proof]
    fn mem_domain_check_respects_caller_length_8() {
        let starts: [u32; 8] = kani::any();
        let sizes:  [u32; 8] = kani::any();
        let _ = gale_mem_domain_check_partition(
            0x1000, 0x100,
            starts.as_ptr(), sizes.as_ptr(),
            8,            // num_partitions advertised to Rust
        );
        // Kani fails on UB (OOB deref) before this assertion is reached.
    }

    /// Case (B): caller supplies 32 slots, partition in slot 20 overlaps
    /// the candidate. Rust today returns OK, but MD1 says it must reject.
    #[kani::proof]
    fn mem_domain_check_rejects_overlap_beyond_slot_16() {
        let mut starts = [0u32; 32];
        let mut sizes  = [0u32; 32];
        starts[20] = 0x1000;
        sizes[20]  = 0x100;          // existing partition [0x1000, 0x1100)
        let d = gale_mem_domain_check_partition(
            0x1050, 0x100,           // overlaps [0x1000, 0x1100)
            starts.as_ptr(), sizes.as_ptr(),
            21,
        );
        assert!(d.ret == EINVAL);    // fails: Rust never inspected slot 20
    }
}
```

The first proof fails on memory-safety (Kani's pointer-provenance check on `.add(8)..`). The second proof fails the functional assertion, demonstrating silent MD1 violation.

## Fix

Take an explicit length parameter (already present but ignored) and loop to `min(num_partitions_cfg, ARRAY_STRIDE)` where `ARRAY_STRIDE` is supplied by the C side, or better, expose the constant from C via a dedicated accessor:

```rust
pub extern "C" fn gale_mem_domain_check_partition(
    part_start: u32,
    part_size: u32,
    domain_starts: *const u32,
    domain_sizes: *const u32,
    num_partitions_cfg: u32,   // == CONFIG_MAX_DOMAIN_PARTITIONS
) -> GaleMemDomainCheckPartitionDecision {
    ...
    let bound = num_partitions_cfg;    // use the real length, no cap at 16
    let mut i: u32 = 0;
    while i < bound {
        ...
    }
}
```

Add a `gale_mem_domain_max_partitions_assert(u32)` startup check so that if the C side is ever built with a value exceeding Rust's tested bound, initialization aborts loudly instead of silently under-checking.

And update the three Kani proofs to parameterize the array length (`const N: usize` generic or a second harness family), so future `CONFIG_MAX_DOMAIN_PARTITIONS` values are covered automatically.

## Severity

High. This is the USERSPACE memory-isolation boundary (ASIL-relevant per the Verus MD1 proof). Failure mode (A) is a direct stack-OOB-read primitive, escalating to OOB-write through the unchecked `slot` return from `gale_k_mem_domain_remove_partition_decide`. Failure mode (B) silently breaks the non-overlap invariant the docstring claims is verified, and it does so on at least one in-tree SoC default (ARC hs, 32 partitions).
