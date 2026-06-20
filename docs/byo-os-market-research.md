---
id: DOC-BYOOS-MARKET
title: "gale BYO-OS — market & research positioning (sweep, 2026-06-20)"
type: documentation
status: proposed
tags: [byo-os, gale74, market-research, positioning, novelty]
links:
  - type: related-to
    target: FIND-BYOOS-001
  - type: related-to
    target: DOC-BYOOS-ARCH
---

> **Source:** 7-dimension parallel research workflow (8 agents, 104 web lookups), 2026-06-20.
> **Maintainer correction (status of the proof-carrying pipeline):** the report cites
> `synth#167` (from a stale memory note) as the pipeline being non-linkable. That is
> outdated — the gust scout has since PROVEN hot-path dissolution: the kiln-async
> scheduler `gust_poll` dissolves wasm→loom→synth to **812 B of linkable Cortex-M3**
> (and re-targets to RISC-V from the same wasm). The *current* open links are
> synth#382 (fixed in v0.11.50) and synth#383 (`.bss` sizing, open) — not #167. So
> recommendation #1 ("validate the pipeline end-to-end") is **further along than the
> report implies**: hot path proven; remaining gap is the on-MCU `.bss` link (#383)
> and carrying the *proofs* through fusion (the genuinely open research claim).

# gale / gust: Market & Research Positioning

## 1. Executive summary

- **Where gale sits:** gale occupies the empty intersection of three mature-but-separate lineages — (a) compositional verified-OS construction (CertiKOS/DeepSpec, seL4), (b) Rust+Verus verified kernels (Atmosphere, Tock/TickTock), and (c) no-runtime wasm-to-bare-metal AOT (aWsm, w2c2). Every neighbor owns one column; **no surveyed project owns the intersection.**
- **The one genuine differentiator:** authoring the OS kernel primitives *themselves* (sem/mutex/msgq/event/poll/scheduler) as WebAssembly, machine-checking them across multiple backends (Verus+Rocq+Lean+Kani), then statically **fusing (meld) + inlining (loom) + AOT-compiling (synth)** the wasm *away* into bare-metal native with a ~4-function TCB. The entire rest of the field treats wasm as an *app sandbox running on top of* a native kernel — the inverse of gale's intent. This "proof-carrying wasm-fusion-to-firmware" pipeline is simultaneously gale's biggest moat and its least externally-validated, partly-unshipped piece (synth#167 arm backend was emitting non-linkable placeholders per project memory).
- **The biggest threat:** the "certified open-source OS *without* machine-checked proofs" wave. If **Zephyr** lands IEC 61508 SIL 3 / ISO 26262 ASIL D (concept approval done; artifacts 2026–27) and **Auterion** ships DO-178 PX4 Pro, the market may judge process-evidence certification "good enough," making gale's proof premium a hard sell unless it *also* cuts cert-evidence cost. The secondary structural threat: **aWsm/eWASM (GWU)** already ships the exact codegen mechanic (wasm→LLVM-LTO→Cortex-M .o, no interpreter) and is attached to an OS group (Composite); if they author primitives in wasm and add proofs, they close the gap fastest.
- **Honest novelty boundary:** none of gale's *individual* ingredients are novel — composition (wac), no-runtime AOT (w2c2/aWsm), verified Rust kernels (Atmosphere/Tock), HAL-in-wasm/timing-in-native seam (Wasm-IO, EMSOFT 2025), and verified wasm semantics (WasmCert) all have credible owners. The moat is the *combination*, not any column.

## 2. Landscape map

| Dimension | Closest player(s) | URL | How close to gale | Key gap vs gale |
|---|---|---|---|---|
| wasm-on-MCU (no-runtime AOT) | aWsm / eWASM (GWU) | github.com/gwsystems/aWsm | **Very close (mechanism)** | App SFI sandbox, not kernel-as-wasm; C runtime LTO'd in (not C-free); no proofs; no component fusion |
| | WAMR (BCA) | github.com/bytecodealliance/wasm-micro-runtime | Far | Keeps ~50K on-device runtime/loader; wasm = app layer on native RTOS |
| | Mewz unikernel | github.com/mewz-project/mewz | Medium (build-your-own idea) | x86 cloud on QEMU; native Zig scaffolding; no MCU; no proofs |
| Component-model fusion | wac (BCA) | github.com/bytecodealliance/wac | **Close (fusion axis)** | Halts at composed .wasm; no inlining, no native, no proofs; cloud-oriented |
| | w2c2 / wasm2c | github.com/turbolent/w2c2 | **Close (no-runtime axis)** | Per-module (no component fusion); emits C; no verification |
| | wac → wasmtime compile → .cwasm | docs.wasmtime.dev | Closest *shipping* end-to-end | Run by heavyweight Wasmtime/Cranelift host runtime, not a ~4-fn TCB |
| Verified OS (composition) | CertiKOS / DeepSpec layers | flint.cs.yale.edu/certikos | **Closest in spirit** | C/CompCert, one kernel image; layers = internal proof decomposition, not a shippable retargetable library; no wasm; no MCU |
| Verified OS (tech twin) | Atmosphere (Verus, SOSP'25) | mars-research.github.io/projects/atmo | **Nearest twin (Rust+Verus)** | Single monolithic kernel; x86/MMU; single-prover; no wasm seam; not "parts you compose" |
| | Tock / TickTock (Flux, SOSP'25) | ranjitjhala.github.io/static/sosp25-ticktock.pdf | Close (same M-class/RISC-V silicon, production) | Verifies *isolation* only, not primitive functional correctness; single prover; fixed kernel |
| | seL4 / Microkit / LionsOS | sel4.systems | Brand threat | C kernel; isolates *unverified* PDs; re-verify per arch; no wasm |
| Embedded Rust async | Embassy | github.com/embassy-rs/embassy | **Closest (executor template)** | kiln-async is "Embassy-inspired"; Embassy more mature/portable; no proofs, no WCET |
| | embedded-hal-async | github.com/rust-embedded/embedded-hal | gale *adopts* it | Community standard; the unverified driver seam gale inherits |
| | RTIC | rtic.rs | Close (bounded-by-construction) | SRP/type-based, not machine-checked; no WCET |
| | Hubris (Oxide) | hubris.oxide.computer | Close (attestable image, tiny TCB) | Synchronous IPC; isolation via MPU, not proofs |
| Commercial cert RTOS | SAFERTOS / VxWorks Cert / QNX / INTEGRITY-178 | highintegritysystems.com | Owns the value prop ("kernel + evidence pack") | Process/MC-DC test evidence, not machine-checked proof; C; not composable |
| Open-source safety | Zephyr Safety | zephyrproject.org | **Co-opetition / drop-in host** | Process-evidence cert; gale must prove machine-checked adds value the Zephyr safety case lacks |
| Drone/market pull | Auterion PX4 Pro / Dronecode | auterion.com | GTM wedge + threat | DO-178 process evidence; no formally-verified primitives |
| Standards/academia (HAL seam) | Wasm-IO (EMSOFT'25, FAU/Dortmund) | dl.acm.org/doi/10.1145/3760387 | **Closest on the wasm/native seam** | Runtime-based; no machine-checked proofs; driver-containerization, not OS kit |
| wasm formal semantics | WasmCert-Coq/Isabelle | conrad-watt.github.io/papers/watt2021.pdf | Closest on verification axis | Verifies the *language/interpreter*, not gale's components; runs interpreted |
| Verified compilation | CompCert / RustCompCert (2026) | trustworthy.systems/projects/OLD/camkes | Flank synth will need | C/Rust-native; no wasm-as-boundary; no component-OS kit |

## 3. gale's genuine novelty (the unoccupied intersection)

The defensible white space is the **conjunction**, and only the conjunction. Precisely:

1. **A library of *independently* verified, reusable, retargetable kernel primitives** — versus everyone shipping *one* verified kernel (seL4, CertiKOS, Atmosphere) or merely *isolating unverified parts* (seL4 Microkit PDs, Tock capsules). Closest on compositionality is CertiKOS/DeepSpec (flint.cs.yale.edu/certikos), but its layers are an internal proof decomposition compiled to one C image, not a shippable cross-target component library.

2. **wasm as the verification *and* portability boundary, then fused away.** The wasm-on-MCU field (aWsm, WAMR, AkiraOS, Wasm-IO) uses wasm as a *runtime sandbox*; gale uses it as a fuse-and-discard build IR. Closest mechanically is aWsm (github.com/gwsystems/aWsm — wasm→LLVM-LTO→Cortex-M object, no interpreter), but it is C-based SFI sandboxing with zero proofs and no kernel library.

3. **Component fusion (meld) + cross-component inlining (loom) + AOT-to-firmware (synth) as one pipeline.** wac fuses but doesn't compile; wasmtime/WAMR compile but keep a host runtime; w2c2 compiles-to-native-no-runtime but does per-module C with no component semantics. **No one does fusion + inlining + AOT-to-bare-metal together** — let alone carrying machine-checked proofs through the fusion.

4. **Multi-backend simultaneous proof** (Verus/SMT + Rocq + Lean + Kani/BMC) of the *same* primitive code, at the *component* level. Every neighbor is single-prover (Tock=Flux, Atmosphere=Verus, seL4=Isabelle; WasmCert verifies only the language). This redundancy is genuinely unmatched in the surveyed set.

5. **Dual-mode delivery** — drop-in C-primitive replacement (Zephyr) *and* build-your-own bespoke OS (gust, 8KB RAM, Cortex-M3) with a fuel-bounded-WCET async executor and ~4-function native TCB — plus **same-wasm retarget across M7/M4/M3/RISC-V by re-AOT + TCB swap**, versus seL4/CertiKOS re-verifying per architecture.

**Claims to kill or soften (findings do not support them):**
- ❌ "First to verify a Rust kernel on M-class silicon" — **false**; TickTock/Tock (SOSP'25) is published and in production (~10M devices).
- ❌ "First to prove a Rust microkernel functionally correct with Verus" — **false**; Atmosphere (SOSP'25).
- ❌ "Novel: wasm AOT'd to bare-metal Cortex-M" — **false**; aWsm, WAMR-AOT, w2c2, AkiraOS. Only the *fuse-it-entirely-away intent* + proofs are gale's.
- ❌ "Novel: static composition of wasm components" — **false**; wac and wasm-tools compose are mature.
- ❌ "Novel: logic-in-wasm / timing-DMA-in-thin-native-seam" — **overlapped**; Wasm-IO (EMSOFT'25) independently drew the same seam with temporal+spatial isolation.
- ❌ "Novel: verified wasm" — **false**; WasmCert, VeriWasm, Veri-ISLE. gale's edge is verifying *kernel-primitive invariants*, not language/sandbox safety.
- ⚠️ "Proof-carrying AOT fusion (synth)" — **unverified / partly roadmap.** Per project memory synth#167 (arm backend) was emitting non-linkable placeholders; this is the most differentiated *and* least externally-validated link. State it as roadmap, not shipped.
- ⚠️ gale "closes the driver-verification gap" — **false.** The embedded-hal-async timing/DMA code is exactly the unverified part living in gale's TCB. gale *relocates and names* the gap (declared ~4-fn TCB); it does not eliminate it.

## 4. Threats / where gale is NOT unique

- **aWsm/eWASM convergence (fastest structural threat).** GWU already ships the codegen mechanic on Cortex-M *and* runs an OS group (Composite). Authoring primitives in wasm + adding proofs would close most of the moat. (github.com/gwsystems/aWsm)
- **Bytecode Alliance convergence.** If BCA wires `wac` fusion into a Cranelift/LLVM no-runtime bare-metal backend (a plausible WAMR+wac merge), the structural moat collapses to *just the formal-verification layer*. WASI 0.3 async (Feb 2026) + the WAMR Embedded SIG push in this direction.
- **Verus/Flux research front is crowded and well-resourced.** If Atmosphere or the Tock/TickTock teams reframe as a "verified primitive library," the technology overlap is large — they have the provers and the silicon already.
- **Process-evidence certification "good enough."** SAFERTOS/VxWorks Cert/QNX/INTEGRITY-178 own the "kernel + evidence pack" market; Zephyr is bringing it open-source and free. If customers accept process evidence, gale's proof premium must be justified by *cost* (auto-generated cert evidence), not just rigor.
- **The build-your-own idea is not gale's.** Mewz (unikernel fuse-to-native), R3-OS (assemble RTOS from primitive crates), Embassy/Ariel (async BYO) all exist. gale's delta is MCU bare-metal + proofs, not the BYO framing itself.
- **The async executor is a reimplementation.** Embassy already nails no_std/no_alloc/single-stack/interrupt-sleep across many MCU families, and is arguably more mature than kiln-async today. Per-ingredient novelty here is low; the only defensible delta is *machine-checked + fuel-bounded-WCET + wasm-verifiable*.
- **Could "WAMR-AOT + a verified kernel" close the gap?** Partially. WAMR-AOT gives production codegen; a verified kernel (Atmosphere/Tock) gives proofs. But the combination would still (a) keep WAMR's ~50K on-device runtime (not a ~4-fn TCB), (b) treat wasm as app layer not kernel substrate, and (c) lack proof-carrying fusion. The architectural inversion (kernel-as-wasm, compiled away) is the part that *cannot* be assembled from existing parts without redoing gale's pipeline.

## 5. Market pull & positioning

**Who buys this**
- **Drones / PX4 / Dronecode (clearest wedge):** PX4-on-NuttX is the de-facto open autopilot; Auterion's DO-178/254 PX4 Pro is the NDAA-compatible US-defense OEM standard — and *nobody there offers formally-verified primitives* (auterion.com). gale's pitch: drop verified flight-critical components (sem/mutex/msgq/scheduler) under PX4 and *reduce DO-178 verification evidence cost* while raising assurance.
- **Automotive / industrial safety (ASIL D / SIL 3 / IEC 62304):** the incumbent buyers of SAFERTOS/VxWorks Cert/QNX. gale competes on machine-checked-vs-process-evidence and on the auto-generated evidence pack.
- **BCA / wasm-component ecosystem:** the component-model community is cloud-focused; gale is the embedded/verified outlier. Positioning here is "the verified embedded member of the component-model family," potentially via the WAMR Embedded SIG.
- **Root-of-trust / secure elements:** Tock's existing footprint (Google/HP/WD, ~10M devices) shows real demand for verified Rust at the MCU root of trust — a natural target for verified primitives.

**White space**
The unserved buyer is the safety customer who wants **machine-checked correctness AND a build-your-own minimal OS AND cross-target retarget without per-arch re-verification AND lower cert-evidence cost** — none of the incumbents offer all four. gale's gust (8KB RAM, Cortex-M3, ~4-fn TCB) demonstrates the build-your-own end credibly.

**README / pitch framing (recommended)**
> "A library of formally-verified OS kernel primitives, authored in Rust→wasm, proven across Verus + Rocq + Lean + Kani, then statically fused and compiled *away* to bare-metal native with a ~4-function trusted base. Build a bespoke OS (gust) or drop verified primitives into Zephyr. The same proven wasm retargets across Cortex-M7/M4/M3 and RISC-V."

Lead with the *inversion* ("we author the kernel in wasm and compile it away — nobody else does"), not with "verified wasm" (prior art) or "wasm on MCU" (crowded). Be explicit that the TCB is *named and tiny*, not zero. Do **not** claim first-verified-Rust-kernel or novel-AOT.

## 6. Research gaps to claim (publishable, gale-led)

1. **Proof-carrying AOT fusion to bare metal.** No one carries machine-checked component proofs *through* fuse+inline+AOT into a firmware image. CompCert/RustCompCert/Veri-ISLE cover adjacent compiler-correctness flanks but not wasm-component-fusion-to-MCU. This is gale's flagship open problem — *and* its biggest risk (synth#167 status: unverified/blocked per memory). Owning the proof that meld/loom/synth preserve primitive invariants would be a first.
2. **Native-substrate WCET for a fuel-bounded async executor.** None of Embassy/RTIC/Tock/Hubris give a static execution-time bound; WCET in the field is an external-tool problem (LDRA/AbsInt) over compiled binaries. A *fuel-bounded WCET model that is a property of the executor* and survives AOT to native is unoccupied.
3. **IRQ→waker correctness proof.** The interrupt→async-waker path is the trust seam between the verified wasm logic and the native HAL. Proving the IRQ-to-waker handoff (the gust rigor gap) is novel against Tock's MPU/IRQ-context-switch isolation proofs, which establish *isolation*, not *functional waker correctness*.
4. **Compiler-TCB trust quantification.** What exactly must be trusted in synth, and how small can it be made (the ~4-function TCB claim)? A precise, mechanized statement of the residual TCB across an AOT-fuse pipeline is a contribution in its own right.
5. **Multi-prover intersection methodology.** Writing primitives to the *intersection* of Verus + Rocq + Lean + Kani restrictions (no trait objects/closures/async in verified code) is itself a methodological result — what does redundant multi-backend proof buy over single-prover, and at what authoring cost? No surveyed project does this.
6. **Retarget-without-re-verification.** Formalizing why the *same* verified wasm + a swapped TCB preserves correctness across M7/M4/M3/RISC-V — versus seL4/CertiKOS per-arch re-verification — is a portability-of-proof claim worth publishing.

## 7. Recommendations (ranked)

1. **Land and externally validate the proof-carrying synth pipeline (meld→loom→synth) end-to-end on gust.** This is the single load-bearing, least-validated claim (synth#167). Until a verified primitive demonstrably fuses+inlines+AOTs into a linkable Cortex-M image with proofs intact, the central moat is roadmap, not product. Highest priority, technical.
2. **Benchmark codegen against aWsm and LLVM-LTO publicly.** aWsm reports ~40% MCU overhead; gale's own acceptable milestone is 10–20%. A head-to-head (size, overhead, TCB LOC) turns the "we beat the status quo" thesis into evidence and pre-empts the "aWsm already does this" rebuttal. Technical + positioning.
3. **Reframe the pitch around the inversion + named TCB, and retire the overclaims** (first-verified-kernel, novel-AOT, novel-verified-wasm, closes-driver-gap). Ship a README that leads with "author the kernel in wasm, compile it away, ~4-function TCB, multi-prover." Cheap, high-leverage, positioning.
4. **Pick the PX4/Dronecode wedge and produce one concrete artifact** — a verified primitive (e.g., msgq or scheduler) dropped under PX4 with a cert-evidence-cost story for DO-178. The drone market has the clearest pull and *zero* formally-verified incumbents. Positioning + technical.
5. **Position against Zephyr safety as "machine-checked supplement," not replacement.** Frame gale's mode-1 drop-in as *adding* what the Zephyr process-evidence safety case structurally lacks (machine-checked functional correctness of the primitives), ideally with auto-generated evidence that *lowers* total cert cost. This directly neutralizes the biggest market threat. Positioning.
6. **Publish the WCET + IRQ→waker + compiler-TCB results as the research flagship** (gaps 1–4 above), and engage the WAMR Embedded SIG / component-model community to establish gale as the "verified embedded" branch before BCA converges on the same architecture independently. Research + ecosystem defense.

—
Note: novelty claims are grounded in the supplied findings (confidence med-high across dimensions); all rest partly on absence-of-evidence for stealth/proprietary/non-English efforts, so "no competitor occupies the intersection" should be stated as *unverified-absent*, not proven-absent. The synth proof-carrying-fusion claim is **unverified** pending #2/#1 above.
