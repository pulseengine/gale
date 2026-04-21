# MMU Finding — region_align_decide silent overflow + MM4 proof drift

**File:** `/Users/r/git/pulseengine/z/gale/src/mmu.rs`
**Severity:** High (ASIL-D safety-relevant)
**Category:** Integer overflow / proof-code drift between spec and FFI helper (matches two priors simultaneously)

## Summary

`region_align_decide` (lines 240–256) silently clamps `aligned_size` to `u32::MAX` on overflow and still returns a seemingly-valid `AlignResult`. Its `ensures` clause does **not** mention `align_result_valid_spec` and omits the coverage invariant `aligned_addr + aligned_size >= addr + size`. This is exactly the MM4 property advertised in the module header ("MM4: region alignment preserves page alignment and no overflow") — the spec exists at line 226 (`align_result_valid_spec`) but is never wired into any function's postcondition. Classic proof-code drift.

Secondary finding: `validate_unmap_request` (lines 348–360) only checks `addr >= page_size` (before-guard) and `size + 2*page_size` fits, but fails to check `addr + size + page_size <= u32::MAX` (after-guard overflow). An unmap at `addr = 0xFFFF_F000`, `size = 0x1000`, `page_size = 0x1000` passes all checks yet the trailing guard page wraps past `u32::MAX`.

## Vulnerable code (region_align_decide)

```rust
pub fn region_align_decide(addr: u32, size: u32, align: u32) -> (result: AlignResult)
    requires
        align > 0,
        addr as int + size as int <= u32::MAX as int,
    ensures
        result.aligned_addr as int <= addr as int,
        result.addr_offset == addr - result.aligned_addr,
        // MISSING: result.aligned_addr + result.aligned_size >= addr + size
        // MISSING: result.aligned_size % align == 0
{
    let aligned_addr = (addr / align) * align;
    let addr_offset = addr - aligned_addr;
    let raw: u64 = (size as u64 + addr_offset as u64 + align as u64 - 1u64)
        / align as u64
        * align as u64;
    let aligned_size = if raw > u32::MAX as u64 { u32::MAX } else { raw as u32 };
    // ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    // Silent truncation: u32::MAX is NOT a multiple of `align`, breaking MM4,
    // and may not cover [addr, addr+size) if ROUND_UP overflowed.
    AlignResult { aligned_addr, addr_offset, aligned_size }
}
```

Also, the precondition allows `addr + size <= u32::MAX` but `ROUND_UP(size + addr_offset, align)` can still overflow u32 (e.g., `addr = 1`, `size = u32::MAX - 1`, `align = 4096` → offset 1, size+offset = u32::MAX, ROUND_UP yields a multiple of 4096 exceeding u32::MAX). The clamp masks this as success.

## Why it is safety-relevant under ASIL-D

1. Callers on the C side (`k_mem_region_align` in `mmu.c:1008-1021`) consume `aligned_size` directly to size a virtual region. A truncated `u32::MAX` that is *not* a multiple of `align` breaks downstream page-table setup invariants and may under-cover the requested physical range, exposing adjacent memory.
2. The module docstring explicitly lists MM4 and MM8 as **verified properties**, but no `ensures` clause proves either for `region_align_decide`. ASIL-D requires the ensures-set to match claimed invariants; this is a certification-blocking gap.
3. Under the unmap secondary finding, a trailing-guard wraparound lets Zephyr unmap `[addr .. u32::MAX]` and then touch `[0 .. page_size]` as the "after" guard, potentially unmapping low-memory mappings.

## Oracle sketch

### (1) Verus counter-example

Adding the missing postcondition `result.aligned_addr as int + result.aligned_size as int >= addr as int + size as int` to `region_align_decide` will fail to verify when `raw > u32::MAX`, because clamping to `u32::MAX` loses bytes. Likewise `result.aligned_size as int % align as int == 0` fails because `u32::MAX % 4096 != 0`.

```rust
// Counter-example witness (addr, size, align):
// addr = 0x0000_0001, size = 0xFFFF_F000, align = 0x1000
// => addr_offset = 1, raw = ((0xFFFF_F000 + 1 + 0xFFF) / 0x1000) * 0x1000
//                         = 0x1_0000_0000 (overflow u32)
// => aligned_size clamped to u32::MAX = 0xFFFF_FFFF (not a multiple of 0x1000)
// => aligned_addr(0) + aligned_size(0xFFFF_FFFF) = 0xFFFF_FFFF
//    but addr(1) + size(0xFFFF_F000) = 0xFFFF_F001 -> coverage holds here but alignment broken.
// Stronger witness: addr = 0xFFFF_F001, size = 0xF00, align = 0x1000
// precondition addr+size = 0xFFFF_FF01 <= u32::MAX passes,
// offset = 1, ROUND_UP(0xF00 + 1, 0x1000) = 0x1000, total ok.
// Move to: align = 0x8000_0000 -> ROUND_UP overflows, silent clamp.
```

### (2) Runtime test

```rust
#[test]
fn region_align_overflow_silently_clamps() {
    let r = region_align_decide(1, u32::MAX - 1, 0x8000_0000);
    // Expect either panic, Err, or (aligned_size % align == 0).
    // Actual: aligned_size = u32::MAX, u32::MAX % 0x8000_0000 == 0x7FFF_FFFF != 0.
    assert_eq!(r.aligned_size % 0x8000_0000, 0, "MM4 alignment violated");
}

#[test]
fn unmap_wraparound_at_top_of_address_space() {
    // page_size = 0x1000, addr near top, size = 0x1000.
    // before-guard OK (addr >= page_size), guard_total fits (0x3000 < u32::MAX).
    // But addr + size + page_size = 0xFFFF_F000 + 0x1000 + 0x1000 = 0x1_0000_1000 -> wraps.
    assert!(!validate_unmap_request(0xFFFF_F000, 0x1000, 0x1000),
            "unmap must reject trailing-guard wraparound");
    // Currently returns true -> FAIL.
}
```

## Recommended fix

1. Add to `region_align_decide` ensures:
   ```rust
   ensures
       result.aligned_size as int % align as int == 0,
       result.aligned_addr as int + result.aligned_size as int
           >= addr as int + size as int,
   ```
   and make the overflow branch return an error (`Option<AlignResult>` or `Result`) instead of clamping silently.
2. Tighten `validate_unmap_request` to additionally assert
   `(addr as u64) + (size as u64) + (page_size as u64) <= u32::MAX as u64`.
3. Wire `align_result_valid_spec` into the `ensures` of `region_align_decide` so MM4 is actually proved, not just declared.

## References

- `src/mmu.rs:219-256` (region_align_decide + round_down_spec + AlignResult)
- `src/mmu.rs:226-231` (align_result_valid_spec — orphaned spec, never used in ensures)
- `src/mmu.rs:347-360` (validate_unmap_request — missing after-guard wrap check)
- Zephyr `kernel/mmu.c:1008-1021` (k_mem_region_align reference)
- Module header lines 18-26 (claimed properties MM1–MM8)
