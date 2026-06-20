---
id: DOC-WASM-FRONTIER
title: "What people are doing with wasm, and where gale fits (frontier sweep, 2026-06-20)"
type: documentation
status: proposed
tags: [byo-os, gale74, wasm-frontier, market-research, wasm-r3, cra, wohl]
links:
  - type: related-to
    target: FIND-BYOOS-002
  - type: related-to
    target: FIND-BYOOS-003
  - type: related-to
    target: DOC-BYOOS-MARKET
---

> **Source:** 8-dimension parallel research workflow (9 agents, 103 web lookups), 2026-06-20.
> **Maintainer corrections (do not trust the report's stale memory references):**
> 1. The report repeatedly cites **`synth#167`** as blocking the dissolved-native artifact.
>    That is OUTDATED. The gust scout has PROVEN hot-path dissolution: `gust_poll` →
>    **812 B of linkable Cortex-M3** (re-targets to RISC-V from one wasm). The real open
>    links are **synth#382 (FIXED in v0.11.50)** and **synth#383 (`.bss` sizing, OPEN)**.
>    So recommendation #1 ("unblock + bench the dissolved artifact") is closer than the
>    report implies — the codegen works; the gaps are the 8 KB `.bss` link (#383) and the
>    comparative bench (tasks #21–#22).
> 2. **Biggest honest takeaways to act on:** (a) the import-tape **record-replay
>    mechanism is prior art** — Wasm-R3 (OOPSLA 2024); ADOPT it and lead the pitch with
>    *divergence-localization* (verification-cut = replay-cut), not "novel record-replay";
>    (b) **"dissolve the runtime" is prior art** (wasm2c in Firefox, w2c2) — gale competes
>    with native/LLVM-LTO and must *beat* Cranelift-AOT, not just runtimes; (c) **EU CRA**
>    (signed/attested OTA mandatory by Dec 2027) + the **10–50× interpreted-wasm energy
>    tax** are real tailwinds for OTA-of-verified-dissolved-wasm (wohl).

# What People Are Doing with Wasm, and Where gale Fits

*Frontier report — 2026-06-13. Grounded entirely in the supplied findings; URLs cited inline; thin evidence marked "unverified".*

---

## 1. The wasm frontier in one page

- **Beyond-the-browser is the default, not the novelty.** Bytecode Alliance's 2025 survey: 67% of wasm devs work outside the browser. "Same .wasm everywhere" (browser/server/edge/embedded) is now an industry design assumption via WASI 0.2 + Component Model — portability differentiates nothing anymore ([thenewstack.io](https://thenewstack.io/4-big-developments-in-webassembly/)).
- **The Component Model crossed into production-mainstream.** WASI 0.2 stabilized the CM+WIT interop contract; WASI 0.3.0 ratified (preview Aug 2025, landed Feb 2026) adds native async (`stream<T>`/`future<T>`), closing the last server gap; CM 1.0 is the explicit next target ([Road to CM 1.0](https://bytecodealliance.org/articles/the-road-to-component-model-1-0)).
- **"AOT and dissolve the runtime" went mainstream.** Wasm 3.0 became a W3C standard (Sep 2025) — SIMD/relaxed-SIMD/tail-calls/memory64/WasmGC ([webassembly.org](https://webassembly.org/news/2025-09-17-wasm-3.0/)). wasm2c ships in **production** in Firefox via RLBox; w2c2+cc beats Wasmtime on Ed25519 (73.5s vs 125s, ~150KB binary) ([00f.net](https://00f.net/2023/12/11/webassembly-compilation-to-c/)). "The best wasm runtime may be no runtime at all" is a named, established thesis — **gale's core dissolve move is prior art in kind.**
- **Record-replay at the wasm import boundary is published, open-source technique.** Wasm-R3 (OOPSLA 2024) records the host-import trace, reduces it ~99.5%, emits a standalone replay module runnable on any engine ([github](https://github.com/sola-st/wasm-r3)). CMU is building bidirectional wasm time-travel debugging on Wasmtime (Oct 2025) ([CMU](https://www.cs.cmu.edu/~wasm/wasm-research-day-2025b.html)). **gale's boundary-tape mechanism is no longer novel.**
- **Energy reality check landed hard.** Dec-2025 benchmark (arXiv 2512.00035) and treVM (DCOSS-IoT 2026): interpreted wasm is 10–50x native energy, 130–480KB RAM — the field openly concedes native/AOT is needed for battery ([arXiv 2512.00035](https://arxiv.org/html/2512.00035v1)).
- **Verification stratified, but app-correctness end-to-end is unoccupied.** Cranelift lowering is now SMT-verified rule-by-rule (Crocus/Veri-ISLE, ASPLOS 2024); Iris-Wasm formalizes the import-seam-as-cut (PLDI 2023); RichWasm carries type proofs in custom sections (PLDI 2024). Almost nobody carries **functional-correctness** proofs from source through compilation to native ([Veri-ISLE](https://cfallin.org/pubs/asplos2024_veri_isle.pdf), [Iris-Wasm](https://dl.acm.org/doi/abs/10.1145/3591265)).
- **Market is consolidating, not gold-rushing.** Akamai acquired Fermyon (Dec 2025); pure-play wasm cloud is hard to monetize standalone. New energy went to AI-agent sandboxing, not verified-embedded ([siliconangle](https://siliconangle.com/2025/12/01/akamai-acquires-webassembly-function-service-startup-fermyon/)).
- **Home automation is hitting battery/cost walls with a wasm-on-MCU answer already shipping.** Espressif ships esp-wasmachine/esp-wdf (WAMR-based, remote install) and split system/app firmware via ESP LowCode Matter — the closest commercial pattern to wohl, but runtime-on-device, unverified ([esp-wdf](https://github.com/espressif/esp-wdf)). EU CRA (full compliance Dec 2027) turns signed/attestable OTA into a market-access requirement ([anchore](https://anchore.com/sbom/eu-cra/)).

---

## 2. Map

| Dimension | Key players (url) | What's hot | gale adjacency | Overlap / threat |
|---|---|---|---|---|
| **Debug / dev-loop** | Wasm-R3 ([github](https://github.com/sola-st/wasm-r3)); CMU TTD ([cmu](https://www.cs.cmu.edu/~wasm/wasm-research-day-2025b.html)); Chrome DevTools DWARF ([chrome](https://developer.chrome.com/docs/devtools/wasm)); LLDB-for-wasm ([fosdem '26](https://fosdem.org/2026/events/attachments/87ZLQV-wasm-debugging-lldb/slides/267097/webassemb_alaohmr.pdf)) | Record-replay at import boundary; bidirectional time-travel; DWARF source-level step in-browser | Verification cut **is** the replay cut; record on MCU, replay in host/browser; ship DWARF for free in-browser stepping | **High.** Boundary-tape no longer novel; if CMU/Wasmtime add an embedded record backend, generic loop becomes commodity |
| **Embedded / IoT** | WAMR ([github](https://github.com/bytecodealliance/wasm-micro-runtime)); wasm3; Atym ([atym.io](https://www.atym.io/post/a-look-into-atym-s-containerization-technology-leveraging-webassembly)); treVM ([arXiv](https://arxiv.org/html/2604.27570)); arXiv 2512.00035 | Component-as-firmware over fleets; energy reality check; OTA wasm plugins/drivers | Dissolve-to-native is the energy-correct answer; same component browser/host/MCU | **High.** WAMR is the de-facto baseline; Atym owns commercial narrative; AOT is becoming table-stakes |
| **Component Model / WASI** | Bytecode Alliance ([CM 1.0](https://bytecodealliance.org/articles/the-road-to-component-model-1-0)); Wasmtime; wac ([github](https://github.com/bytecodealliance/wac)); Fermyon; wasmCloud | WASI 0.3 native async; CM 1.0 lazy ABI; build-time composition (wac); nanoprocesses (unshipped) | Adopt WIT as official seam; map host event loop to MCU IRQ/DMA; deliver nanoprocess isolation on 8KB | **Medium-high.** Composition is mainstream; if a verifying AOT bolts onto Wasmtime's CM pipeline, moat narrows to proofs |
| **Novel uses** | Shopify Functions ([shopify](https://shopify.dev/docs/apps/build/functions)); Zed ([zed](https://zed.dev/blog/zed-decoded-extensions)); SingleStore; proxy-wasm/OPA; Arbitrum Stylus; AkiraOS ([hackster](https://www.hackster.io/Artur_R0K3R/akiraos-wasm-runtime-for-microcontrollers-dcc59e)) | Wasm as plugin/UDF/policy/contract ABI; narrow typed host↔guest seam everywhere | Verified components as drop-in proven UDFs/policies for safety-critical edge | **Medium.** The seam is commoditized; novelty is what gale does with it, not the seam |
| **AOT / perf** | Cranelift/Winch ([cranelift.dev](https://cranelift.dev/)); wasm2c+RLBox ([github](https://github.com/PLSysSec/rlbox_wasm2c_sandbox)); w2c2; WARP ([github](https://github.com/wasm-ecosystem/wasm-compiler)); Wasm 3.0 | "No runtime at all" shipping; bare-metal AOT a category; Cranelift within ~2% of TurboFan | Same spectrum, fully-dissolving + proof-carrying end; lower relaxed-SIMD/tail-calls on MCU | **High.** Dissolve is mainstream; gale competes with **native/LLVM-LTO**, not just runtimes; perf bar is near-native |
| **Verification / security** | VeriWasm; Crocus/Veri-ISLE ([pdf](https://cfallin.org/pubs/asplos2024_veri_isle.pdf)); WasmCert; Iris-Wasm ([acm](https://dl.acm.org/doi/abs/10.1145/3591265)); RichWasm ([arXiv](https://arxiv.org/pdf/2401.08287)); Enarx | Verification-in-compiler; import-seam robust-safety theory; proof-carrying custom sections | Iris-Wasm backs the seam claim; adopt RichWasm proof-carriage; VeriWasm-class post-dissolve gate | **Medium.** Each piece has an owner; gale's intersection (app-correctness + dissolve to runtime-free MCU) is unoccupied |
| **Market momentum** | Fermyon→Akamai ([siliconangle](https://siliconangle.com/2025/12/01/akamai-acquires-webassembly-function-service-startup-fermyon/)); wasmCloud/CNCF; Extism; SpecTec ([webassembly.org](https://webassembly.org/news/2025-03-27-spectec/)) | Consolidation; AI-agent sandboxing; WASI async; machine-checked spec | Ride the standard; co-opt "prove the kernel, don't just sandbox" vocabulary | **Medium.** Portability is table-stakes; verified-embedded is not a recognized buyer category (evangelism burden) |
| **Home-automation MCU** | Espressif esp-wdf/ESP LowCode ([esp-wdf](https://github.com/espressif/esp-wdf)); ESP32-C6/H2; nRF54L15 ([nordic](https://www.nordicsemi.com/Products/nRF54L15)); SiWx917; seL4/LionsOS+Verus ([sel4](https://sel4.systems/Summit/2025/abstracts2025.html)); EU CRA | Split firmware mainstream; wasm-as-app-unit shipping; battery pain (Matter-over-Thread ~8mo); CRA forcing function | OTA-of-verified-wasm dissolved-native; import seam = vendor-TCB/app contract; replay field battery bugs | **High.** esp-wdf is free/vendor-backed on volume silicon; seL4 owns "verified embedded" mindshare |

---

## 3. Adjacencies gale should exploit (ranked)

**1. Adopt the Wasm-R3 recorder design wholesale; add the verification twist.** Wasm-R3 already records import-boundary interaction (host calls + linear-memory/table deltas) and reduces traces ~99.5% ([github](https://github.com/sola-st/wasm-r3)). gale lifts the recorder but feeds the tape into the **same verified component** instantiated in wasmtime/kiln/browser — not a baked standalone module. The delta: because gale components are proven, **replay divergence is diagnostic gold** — identical import tape diverging device-vs-host means the bug is provably *not* in the verified logic, localizing it to TCB-or-HW. That localization guarantee is what Wasm-R3 and CMU-TTD cannot make. Tie directly to the silicon-anchor capture matrix already scaffolded in `benches/engine_control/silicon` (DWT counters, events.csv) so the import tape is a superset of a debug trace.

**2. Make WIT + the Component Model the official public seam, not an internal convention.** Author kernel components against real WIT worlds so the *same verified artifact* runs in wac/wasmCloud compositions, loads in Wasmtime/WAMR for host test, and dissolves to native via meld/loom/synth. Differentiator becomes "the only toolchain that takes a *standard* component all the way to runtime-free bare metal." Critically, this de-risks the interop gale is counting on — if the TCB import seam diverges from WASI/CM conventions, gale **loses** the same-component-everywhere property it leans on. Ride the standard ([CM 1.0](https://bytecodealliance.org/articles/the-road-to-component-model-1-0)).

**3. Ship DWARF with the pre-dissolve module for a free in-browser debugger.** Chrome DevTools DWARF ([chrome](https://developer.chrome.com/docs/devtools/wasm)) and the LLDB-for-wasm push ([fosdem '26](https://fosdem.org/2026/events/attachments/87ZLQV-wasm-debugging-lldb/slides/267097/webassemb_alaohmr.pdf)) give a ready-made UI seam: a replayed tape becomes steppable at source level in-browser with **zero custom debugger**. Pair with CMU's bidirectional-replay idea for reverse step-through of a captured wohl/PX4 field bug.

**4. Adopt Wizer for deterministic replay start-state.** Wizer ([github](https://github.com/bytecodealliance/wizer)) pre-init snapshots (now CM-aware) give the replay a deterministic, reproducible instantiation point — the start-of-tape state — for free. Also a legitimate boot-time win for wohl/gust cold start.

**5. Borrow Wasm 3.0 features as a richer input ISA.** Relaxed-SIMD and guaranteed tail calls are now standard ([wasm 3.0](https://webassembly.org/news/2025-09-17-wasm-3.0/)); loom-inlining + synth-AOT can lower relaxed-SIMD to MCU DSP/SIMD lanes and use TCO to flatten state machines — exploitable harder by the "tiny = optimal regalloc" thesis than by a general JIT.

**6. Map WASI 0.3 native async onto the bare-metal scheduler.** The host-owned single event loop (`stream<T>`/`future<T>`) is a clean match for an MCU's interrupt/DMA model. Exposing verified async drivers through standard async WIT is **genuinely novel** — nobody runs CM async on bare metal.

---

## 4. Validation of the replay/dev-loop idea

**Does record-on-HW / replay-in-host exist anywhere?** The *mechanism* exists and is published; the *exact gale loop* does not.

- **Closest public art: Wasm-R3 (OOPSLA 2024)** ([github](https://github.com/sola-st/wasm-r3), [arXiv 2409.00708](https://arxiv.org/abs/2409.00708)). It does precisely "record at the import boundary, replay on a different engine," with the cross-runtime shape gale claims (record in any browser, replay anywhere). **But** its stated purpose is benchmark extraction, and its artifact is a self-contained module with host emulation baked in — *not* a tape re-fed through a live verification seam. RR-Reduce (ASE 2025) extends it ([pdf](https://software-lab.org/publications/ase2025_RR-Reduce.pdf)). A second, independent record/replay paper (arXiv 2506.07834, June 2025) records a function's boundary interactions to reproduce/reduce bugs ([arXiv](https://arxiv.org/abs/2506.07834)).
- **Debugging-first, single-runtime: CMU TTD** ([cmu](https://www.cs.cmu.edu/~wasm/wasm-research-day-2025b.html)) — low-overhead bidirectional record/replay, **Wasmtime-only, no device→host story.**
- **In the reference runtime: Wasmtime** is independently building record/replay + WasmDebugging/SourceDebugging WIT interfaces (record-on-Cranelift/replay-on-Winch) ([wasmtime](https://docs.wasmtime.dev/examples-pre-compiling-wasm.html)) — *emerging, not confirmed GA (unverified as shipped)*.

**gale's genuine delta (survives the prior art):**
1. **Record on real embedded/MCU hardware.** Wasm-R3 records in browsers; CMU-TTD on host Wasmtime. Neither targets bare-metal dissolved-native. *No public instance of the exact HW-device-record → host/browser-replay-for-debugging loop was found in 2024–2026 sources (absence of evidence, not proof of absence — unverified).*
2. **Verification-cut = replay-cut.** Divergence on an identical tape provably excludes the verified logic and localizes to TCB-or-HW. This is the load-bearing, unclaimed framing — backed theoretically by Iris-Wasm's robust-safety boundary ([acm](https://dl.acm.org/doi/abs/10.1145/3591265)).
3. **Dissolved-native deployment where the replayed module and the device module are the same verified artifact** (provenance by construction through meld/loom/synth).

**Verdict: do not kill — but stop pitching the mechanism as new.** The boundary-tape idea is anticipated. Lead with the proof (divergence localization) and the bare-metal record backend; those are unclaimed. If gale keeps marketing "record-replay across runtimes" as the innovation, reviewers will point at Wasm-R3 immediately.

---

## 5. Threats / convergence risk

- **The generic loop becomes free commodity.** If CMU-TTD or Wasm-R3 adds an embedded recording backend, or Wasmtime's bidirectional record-replay lands as a first-class feature, gale's debug story collapses to "we also have proofs." This is the single sharpest convergence risk ([cmu](https://www.cs.cmu.edu/~wasm/wasm-research-day-2025b.html), [github](https://github.com/sola-st/wasm-r3)).
- **A verifying AOT bolted onto Wasmtime's CM pipeline.** If CM 1.0's lazy ABI + nanoprocess shared-nothing linking land well and someone attaches a Crocus-style verifying AOT to Wasmtime's component pipeline, the "same verified component in browser/host/MCU + replay" story is largely deliverable on stock Wasmtime — minus the bare-metal dissolve. gale's moat then narrows to the proof tracks alone ([CM 1.0](https://bytecodealliance.org/articles/the-road-to-component-model-1-0), [Veri-ISLE](https://cfallin.org/pubs/asplos2024_veri_isle.pdf)).
- **WAMR / AkiraOS / esp-wdf own embedded today.** Portable, sandboxed, OTA-updatable wasm on Cortex-M/Zephyr is in production with ~64KB runtimes and momentum ([WAMR](https://github.com/bytecodealliance/wasm-micro-runtime), [AkiraOS](https://www.hackster.io/Artur_R0K3R/akiraos-wasm-runtime-for-microcontrollers-dcc59e), [esp-wdf](https://github.com/espressif/esp-wdf)). "Wasm on MCU" reads as WAMR first. gale gets no novelty for it — only for verified + zero-runtime.
- **AOT closes the perf gap.** WAMR-AOT ~50% native and shipping on ESP32; Cranelift within ~2% of TurboFan. gale's "optimal-because-tiny regalloc" must demonstrably **beat Cranelift-AOT**, not match LLVM — and **synth's ARM backend is currently blocked (synth#167, meld-dispatch placeholders, non-linkable .o)**, so gale cannot yet demonstrate the dissolved-native artifact that beats WAMR-AOT, which is the central claim ([per memory note; not re-verified in repo this session — unverified]).
- **"No runtime" is established prior art.** wasm2c-in-Firefox and Frank Denis's thesis name gale's exact pitch ([00f.net](https://00f.net/2023/12/11/webassembly-compilation-to-c/)). The low-cost 80/20 (sandbox + native + tiny binary *without* formal verification) is a far lower adoption cost — if gale's proofs don't catch bugs wasm2c+sandbox would miss, verification is overhead, not moat.
- **Hyperscaler / consolidation.** The Fermyon→Akamai exit shows pure-play wasm is hard to fund standalone; if a hyperscaler ships a verified edge component product, gale's window narrows ([siliconangle](https://siliconangle.com/2025/12/01/akamai-acquires-webassembly-function-service-startup-fermyon/)).
- **seL4 owns "verified embedded" mindshare.** A decade of proof pedigree plus DARPA funding and Verus-on-seL4 (2025) ([sel4](https://sel4.systems/Summit/2025/abstracts2025.html)). gale's counter — seL4 is a runtime TCB on device, not the same artifact across browser/host/MCU, and not an OTA-wasm story — is nuanced and easy to lose in a soundbite.

---

## 6. wohl (home automation) read

**Is verified + dissolved-wasm + OTA a real fit? Yes on technical alignment; unvalidated on market pull.**

- **The market independently validated gale's two core moves.** (1) **Split firmware is mainstream** — Espressif ESP LowCode separates vendor "system firmware" (Matter + radios + OTA) from customer "application firmware," even pinning system to HP core / app to LP core on C6 ([esp blog](https://developer.espressif.com/blog/2025/02/introducing-esp-lowcode-matter/)). gale's verified-component + narrow import seam **is** that cut, but principled: the seam is verification boundary *and* OTA unit *and* replay cut. (2) **Wasm as the app unit is shipping** — esp-wasmachine/esp-wdf does remote wasm/AOT install over the wire ([esp-wdf](https://github.com/espressif/esp-wdf)). gale does the same shipping story but dissolves to native, claiming WAMR's portability/isolation **without** the ~20–25% runtime tax and RAM cost — decisive on coin-cell / gust-class 8KB budgets where sleep current and flash are the BOM.
- **The battery pain is real and gale's dissolve is the right answer.** A correct Thread/Zigbee node should last 12–36 months on a coin cell at 2–10µA sleep, yet real Matter-over-Thread sensors reportedly need swaps in ~8 months; arXiv 2512.00035 quantifies interpreted wasm at 10–50x native energy ([arXiv 2512.00035](https://arxiv.org/html/2512.00035v1)). Dissolve-to-native sidesteps the runtime energy tax entirely — **but the numbers are asserted, not benched** (blocked on synth#167 / the open gust comparative bench, tasks #21–#22). Until that bench lands, this is a claim, not a proof.
- **Regulation is a genuine tailwind.** EU CRA mandates machine-readable SBOMs, secure-by-design, vuln management, and secure OTA for all connected products (vuln reporting Sept 2026, full compliance Dec 2027, penalties to €15M/2.5% revenue) ([anchore](https://anchore.com/sbom/eu-cra/)). "Provably correct + attestable + signed OTA" shifts from nice-to-have to market-access requirement — directly favorable to OTA-of-verified-wasm with proof-carrying provenance.

**Honest counterweights:**
- **esp-wdf is the free, vendor-backed incumbent** on the volume-cost silicon, already doing OTA-style install. "Why not just use esp-wdf?" kills wohl unless gale shows the dissolved path beats WAMR-AOT on flash/RAM/energy *and* carries a proof. (It's still "Preview" — a window, but Espressif's distribution will close it.)
- **Inversion risk on the safety pitch.** Split firmware means the part nobody trusts and everyone wants verified is the *vendor system firmware* (radio + Matter + OTA) — which gale treats as untrusted TCB and does **not** verify. gale only verifies the app side, the less safety-critical half. This needs a crisp answer.
- **Demand is greenfield.** Matter/Thread/Zigbee 2025 coverage shows **zero** explicit wasm integration in home automation. Opportunity *and* unvalidated demand — there is no established buyer category or budget line for verified-embedded wasm; it must be evangelized.

---

## 7. Recommendations (ranked)

**1. Unblock and bench synth#167 before any external positioning — this gates the central claim.** Every differentiator (energy, flash/RAM win, "beats WAMR-AOT," dissolved-artifact-as-replay-target) depends on a dissolved-native artifact that does not yet exist on ARM. Land the gust comparative bench (tasks #21–#22) producing head-to-head flash/RAM/sleep-current numbers vs WAMR-AOT. *Tied to: §5 perf-gap threat, §6 battery claim being asserted-not-benched.* Until this lands, the moat is rhetorical.

**2. Reframe the dev-loop pitch: lead with divergence-localization, not record-replay.** Stop marketing the boundary-tape mechanism as novel (Wasm-R3 anticipated it). Build the recorder by lifting Wasm-R3's design, target the **MCU record backend** (the unclaimed part), and headline the proof-backed guarantee: identical-tape divergence localizes to TCB-or-HW, never the verified logic. *Tied to: §4 verdict, §3 adjacency #1.*

**3. Commit to WIT/Component Model as the official seam.** Author components against real WIT worlds; accept `.wac` composition graphs as front-end input; adopt Wasmtime's replay-tape / WasmDebugging WIT interfaces if/when they ship so HW tapes replay in stock Wasmtime with zero bespoke glue. This both rides the rising standard and protects the same-component-everywhere property gale depends on. *Tied to: §2 CM threat, §3 adjacency #2.* **If gale ever ships an on-device interpreter it collapses into the WAMR category — never do this.**

**4. Position wohl as "OTA-of-verified-wasm, CRA-grade," riding split firmware.** Map the import seam onto the established vendor-TCB / customer-app contract; ship signed, attested, proof-carrying components dissolved-native into A/B OTA slots. Use the CRA SBOM/secure-OTA mandate as the wedge into a category that has no incumbent. *Tied to: §6 split-firmware validation + CRA tailwind.* **Pre-empt the inversion risk** by being explicit that gale verifies app logic at the seam and treats the radio/Matter stack as declared TCB — and offer the replay loop as the field-debug tool for the TCB/HW side.

**5. Claim the nanoprocess vision concretely where the mainstream can't reach.** Shared-nothing linking is the ecosystem's *unshipped* security promise. Deliver it: each verified component keeps its own linear memory + minimal capabilities, fused statically with proofs that isolation holds, on an 8KB-RAM Cortex-M3 (gust). That is a demonstrable thing the mainstream only aspires to. *Tied to: §2 nanoprocess gap, §1 verification-unoccupied finding.*

**6. Sharpen the two-line distinction against the two incumbents that will be invoked.** Against WAMR/esp-wdf: "they keep a runtime + TCB on the device; gale dissolves to native with no runtime and a machine-checked TCB." Against seL4: "they verify a runtime microkernel on device; gale verifies the *component logic*, removes the runtime, and runs the *same artifact* in browser/host/MCU." Bake these into every pitch so gale is never miscategorized. *Tied to: §5 WAMR + seL4 threats.*

---

*Confidence: High on the landscape and on the three central findings — (a) import-boundary record / cross-engine replay is already published (Wasm-R3), (b) "no runtime" dissolve is established prior art (wasm2c/w2c2/Frank Denis), (c) no mainstream player ships verified + dissolved wasm on MCUs. Medium on whether stock Wasmtime+verified-Cranelift+TTD could subsume gale's non-dissolve story (research, not one shipped product). Findings on synth#167 status and gust energy numbers are drawn from memory notes, not re-verified in-repo this session — marked unverified.*
