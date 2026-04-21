You are emitting a new entry to gale's safety artifact store.
Consult `artifacts/` for the existing shape; gale does not follow
the `safety/stpa/loss-scenarios.yaml` convention that sibling repos
use. Identify the right target file and shape by reading existing
artifacts first.

Input:
- Confirmed bug report (below)
- Chosen `UCA-N` from the validator
---
{{confirmed_report}}
UCA: {{uca_id}}
---

Rules:
1. Grouping invariant: entries group under UCAs. New entries are
   siblings of existing ones under the same UCA, not new UCAs.
2. New id follows the existing scheme in the target file.
3. Required fields match existing entries exactly. Do not invent
   fields.
4. In the prose description, reference the oracle by path (Kani
   harness, Verus proof file, Rocq .v file, or Lean .lean file) AND
   cite the Zephyr kernel specification or `docs/` proof document
   section that the bug violates. Include the concrete thread
   interleaving / input that triggers it.
5. If the bug is proof-code drift (proof correct, Rust code
   diverged from the proof's assumed specification), say so
   explicitly. Drift needs different remediation than a primitive
   bug: either re-verify the current code or revert the code to
   match the proof.
6. Optional: `related-cve:` with the Zephyr CVE class this mirrors
   (e.g., `CVE-2023-5564` for double-free-under-concurrency,
   `CVE-2022-3806` for cbprintf overflow).
7. Add `status: draft`. Gale ships kernel code — no draft →
   deployed path without human review.

Emit ONLY the artifact block, nothing else.
