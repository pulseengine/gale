# "A compiler with no verifier cannot reach it" — falsified, and the real claim is better

`gust_mix` is `clamp(1500 + (ch - 1024), 1000, 2000)`. Our claim has been that
when a composition proves `ch ∈ [524,1524]`, both clamp branches are dead and the
function collapses to `add r0,#476; bx lr` — and that **LLVM cannot reach that**,
because it never had the bound.

The first half is true. The second half was overstated, and a reviewer persona
caught it. LLVM will take the bound if you hand it to it.

## Measured (rustc → thumbv7m-none-eabi, opt-level="s", lto, panic=abort)

```rust
pub extern "C" fn mix_plain(ch: u16) -> u16 {
    let v = 1500i32 + (ch as i32 - 1024);
    v.clamp(1000, 2000) as u16
}

pub extern "C" fn mix_hinted(ch: u16) -> u16 {
    unsafe { core::hint::assert_unchecked(ch >= 524 && ch <= 1524) };
    let v = 1500i32 + (ch as i32 - 1024);
    v.clamp(1000, 2000) as u16
}
```

| | `.text` |
|---|---|
| `mix_plain` | **30 B** |
| `mix_hinted` | **12 B** |

```
00000000 <mix_hinted>:
   0:	b580      	push	{r7, lr}
   2:	466f      	mov	r7, sp
   4:	f500 70ee 	add.w	r0, r0, #476
   8:	b280      	uxth	r0, r0
   a:	bd80      	pop	{r7, pc}
```

`add.w r0, r0, #476` — the same fold, from stock LLVM, given the same premise.

## So what is actually ours

Not the optimization. **The provenance of the premise.**

`assert_unchecked` is an *assertion by the programmer*. It is unchecked by
construction — write the wrong range and you have introduced undefined behaviour
with no diagnostic. To write it correctly for `gust_mix` you must already know a
bound that is established in a *different component*, hand-audit that the
interface still guarantees it, and re-audit by hand every time either side
changes.

What the pipeline does instead: the bound is discharged where it is known, travels
as `wsc.facts` keyed to a value, and the consumer proves its own specialization
correct *given* the fact, per site, with a certificate. Nobody writes an
unchecked assertion, and nothing has to be re-audited by hand when either side
moves.

## The claim, corrected

- ~~"A compiler with no verifier cannot reach it."~~ **False.** Measured above.
- "LLVM never *had* the bound." **True, and the point.** The fact originates
  across a component boundary that neither Rust's type system nor the canonical
  ABI carries it across.
- The honest headline: **the same codegen, reachable today only by a hand-written
  unchecked assertion, obtained instead from a machine-checked fact that crosses
  the boundary on its own.**

This is the stronger claim, and it is the Component Model story rather than a
codegen story.

## Reproduce

    cd /tmp/hint && cargo build --release --target thumbv7m-none-eabi
    arm-zephyr-eabi-nm --print-size --size-sort target/thumbv7m-none-eabi/release/libhint.a | grep mix_
