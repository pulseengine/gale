# MPU region validation — end-of-address-space overflow defeats overlap check

## Severity
High — silently defeats userspace isolation (ASIL-D claim void for high-addr regions).

## Location
`/Users/r/git/pulseengine/z/gale/src/mpu.rs`

- `validate_region` (lines 131–144)
- `regions_overlap` (lines 159–172), preconditions lines 161–162
- `validate_region_set` (lines 185–219), annotated `#[verifier::external_body]`

## Vulnerability

`validate_region` enforces the three ARMv7-M rules (power-of-two size ≥ 32,
base aligned to size) but does **not** bound `base + size` against `u32::MAX`.
`is_pow2_spec` admits `2147483648u32 = 2^31` (line 74). The region

    base = 0x8000_0000, size = 0x8000_0000   (attr arbitrary)

passes every check in `validate_region`:
- size = 2^31 is in the flat power-of-two enumeration,
- size ≥ MIN_REGION_SIZE (32),
- base & (size-1) = 0x8000_0000 & 0x7FFF_FFFF = 0 (aligned).

Yet `base + size = 0x1_0000_0000` overflows u32. This value is consumed by
`regions_overlap`, whose **`requires` clause** (lines 161–162) states

    r1.base as int + r1.size as int <= u32::MAX as int

The exec body on line 169 performs `r1.base + r1.size` as native `u32`
addition. In debug this panics (fault inside MPU configuration path); in
release it **wraps to 0**, so `r1_end = 0`, and the comparison
`r2.base < r1_end` is false for every other region ⇒ overlap is not
detected. A second region anywhere in memory can then be configured with
conflicting attributes — userspace isolation is silently defeated.

The precondition of `regions_overlap` is never discharged at the call
site because `validate_region_set` is marked `#[verifier::external_body]`
(line 185) — Verus accepts the function without inspecting its body,
so the violated precondition never shows up as a proof obligation. This
is exactly the "proof-code drift between validation model and ARM
architecture constraints" prior.

Related drift signals in the same file:

1. Lemmas `lemma_validate_region_spec`, `lemma_misaligned_rejected`,
   `lemma_below_minimum_rejected`, `lemma_common_regions_valid` have
   empty or `ensures true` bodies (lines 227–269). They assert nothing
   non-trivial about `validate_region`, so P1 in the module header is
   not actually proven.
2. `validate_region_set` performs an all-pairs `i,j` loop without
   checking `count as usize <= regions.len()`; with `external_body`
   an out-of-bounds `regions[i as usize]` is outside the verified
   surface.
3. ARMv7-M also requires the SRD (sub-region disable) 8-bit mask to be
   coherent with region size ≥ 256 for sub-regions to take effect. No
   SRD modelling is present even though the module claims to mirror
   `mpu_partition_is_valid`.

## Oracle

### Verus (counter-example to the intended P1 ∧ P3)

```rust
// verus!
proof fn witness_highaddr_overflow() {
    let r = MpuRegion { base: 0x8000_0000u32, size: 0x8000_0000u32, attr: 0 };
    // validate_region returns true (all three bit-checks hold):
    assert(r.size & (r.size - 1) == 0) by(bit_vector);
    assert(r.base & (r.size - 1) == 0) by(bit_vector);
    assert(r.size >= MIN_REGION_SIZE);
    // yet the overlap-check precondition is violated:
    assert(r.base as int + r.size as int > u32::MAX as int);
}
```

This compiles to a proof that an accepted region is unusable by
`regions_overlap`, contradicting the stated P3.

### Unit test (release-mode demonstration)

```rust
#[test]
fn highaddr_region_wraps_in_overlap_check() {
    let r1 = MpuRegion { base: 0x8000_0000, size: 0x8000_0000, attr: 0 };
    let r2 = MpuRegion { base: 0x9000_0000, size: 0x1000,       attr: 0 };
    assert!(validate_region(r1.base, r1.size));
    assert!(validate_region(r2.base, r2.size));
    // With wrapping arithmetic r1_end wraps to 0; overlap returns false
    // even though r2 lies strictly inside r1.
    let r1_end = r1.base.wrapping_add(r1.size); // 0
    let r2_end = r2.base.wrapping_add(r2.size);
    assert_eq!(r1_end, 0);
    assert!(!(r1.base < r2_end && r2.base < r1_end));
}
```

## Fix

Add to `validate_region`:

```rust
// Reject regions that would wrap the 32-bit address space.
// (base + size must fit in u32; equivalently size <= u32::MAX - base + 1,
//  or since size is pow2 ≥ 32: base <= u32::MAX - size + 1.)
if (u32::MAX - base) < size - 1 { return false; }
```

and remove `#[verifier::external_body]` from `validate_region_set` so
the precondition of `regions_overlap` is discharged by Verus. Flesh out
the stub lemmas with real `ensures` matching the module header P1–P4.
