I have received the following bug report. Can you please confirm if
it's real and interesting?

Report:
---
{{report}}
---

You are a fresh validator. Gale is ASIL-D kernel code — a false
positive here costs serious engineering time, and a false negative
ships a kernel primitive bug.

Procedure:
1. Read the cited file and function BEFORE the hypothesis. For
   proof-related claims, locate the Verus/Rocq/Lean proof and check
   what it ACTUALLY proves. Proof ≠ correctness; it is correctness
   relative to the proof's stated preconditions and postconditions.
2. Run the provided oracle (Kani / Verus / Rocq / Lean). If no
   counterexample appears on the unfixed code, reply
   `VERDICT: not-confirmed`. Stop.
3. Run the PoC. For concurrency bugs, use a deterministic scheduler
   or `loom` to establish reproducibility. If the PoC passes on the
   unfixed code, reply `VERDICT: not-confirmed`. Stop.
4. If both confirm, ask: is this *interesting*?
   A finding is NOT interesting if any of the following hold:
     - it violates a spec precondition the kernel explicitly
       documents as the caller's responsibility (e.g.,
       "must be called with IRQs disabled")
     - the scenario requires a hardware configuration outside
       gale's declared target list
     - the race requires a core count or ordering model the port
       layer does not support
     - it is a known limitation in `artifacts/` or safety/
5. If real and interesting, map to a `UCA-N`. Prefer grouping under
   existing UCAs. If no existing UCA fits, reply
   `VERDICT: confirmed-but-no-uca` with a description of what new
   UCA would be needed; do not emit a scenario.

Output:
- `VERDICT: confirmed | not-confirmed | confirmed-but-no-uca`
- `UCA: UCA-N` (only on confirmed)
- `REASON:` one paragraph
