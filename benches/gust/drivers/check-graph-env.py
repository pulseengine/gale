#!/usr/bin/env python3
"""VER-DRV-GRAPH-001 — no raw `env` import survives in the composed graph.

REQ-DRV-COMPONENT-001 used to carry this clause alongside a per-driver one, and the
two disagreed about dma-own (in the graph, but an ownership FSM rather than a
thin-seam driver). The per-driver clause is gated by check-driver-components.py.
This is the graph clause.

The rule, deliberately narrow: no core module of a committed composed/fused wasm
may import from module `env`. A WIT-lowered import carries a namespaced module name
(`gust:hal/mmio@0.1.0`); a raw `extern "C"` seam lowers to `env`. That is the
distinction the requirement is about.

What this does NOT do, and why: it does not classify by SYMBOL SHAPE. An earlier
sketch of this gate keyed on the underscore-vs-hyphen split (`poll_task` raw,
`poll-task` WIT-lowered). A hyphen does prove WIT-lowering -- it cannot occur in a C
identifier -- but its ABSENCE proves nothing: `read32`, `write32` and `poll` are all
WIT field names with no hyphen. A one-way signal is not a rule, so the check reads
the import's MODULE name, which is unambiguous.

Non-WIT-shaped imports that are not `env` are REPORTED, not failed. `fused.wasm`
imports `("cabi-arena-realloc", "")`, a canonical-ABI shim -- outside this
requirement's literal rule. Listing it keeps a later reader from assuming the sweep
looked at it and approved.

Census: every committed *.wasm under the gust bench must be either swept or on the
NOT_COMPOSED ledger. A new artifact silently escaping coverage is exactly how
dma-own escaped the gate this one is a sibling to (gale#316).

Exit codes:
  0  clean
  2  usage / tooling error (wasm-tools missing)
  3  a composed artifact imports from `env`
  5  a committed .wasm is neither swept nor ledgered
  6  the NEGATIVE CONTROL passed when it must fail  <- the gate is not biting
"""
import argparse, pathlib, re, subprocess, sys, tempfile

# Composed / fused artifacts: the graph this requirement is about.
# Paths are relative to the GUST BENCH ROOT (benches/gust), not to this directory,
# so the sweep and the census cover the same tree. An earlier draft rooted both at
# drivers/ and swept ../wasm-kernel/* from outside the censused area -- those two
# were checked but a NEW artifact beside them would not have been noticed, which is
# precisely how dma-own escaped coverage (gale#316).
COMPOSED = [
    "drivers/measurements/wcet-fixture/gustos.loom.wasm",
    "drivers/measurements/wcet-fixture/gustos.loom.named.wasm",
    "drivers/os-node/repro-757/loom.wasm",
    "wasm-kernel/fused.wasm",
    "wasm-kernel/gust_kernel.wasm",
]

# Committed .wasm that are NOT composed graphs, with the reason. Ledgered so the
# census cannot go vacuous, the same shape as check-driver-components.py's UNGATED.
NOT_COMPOSED: dict[str, str] = {
    # path -> why it is not a composed graph. Empty today; kept as a dict so a
    # future entry must carry its reason rather than appearing as a bare path.
}

IMPORT_RE = re.compile(r'\(import "([^"]*)" "([^"]*)"')


def imports_of(wasm: pathlib.Path, wasm_tools: str) -> list[tuple[str, str]]:
    r = subprocess.run([wasm_tools, "print", str(wasm)], capture_output=True, text=True)
    if r.returncode != 0:
        sys.exit(f"FATAL: {wasm_tools} print failed on {wasm} (exit 2)\n{r.stderr[:400]}")
    return sorted(set(IMPORT_RE.findall(r.stdout)))


def wit_shaped(module: str) -> bool:
    """A WIT-lowered import names its interface: `ns:pkg/iface@version`."""
    return ":" in module and "/" in module


def sweep(root: pathlib.Path, wasm_tools: str) -> int:
    worst, swept, noted = 0, [], []
    for rel in COMPOSED:
        w = (root / rel).resolve()
        if not w.exists():
            print(f"  SKIP {rel} (not present)")
            continue
        imps = imports_of(w, wasm_tools)
        env = [(m, f) for m, f in imps if m == "env"]
        odd = [(m, f) for m, f in imps if m != "env" and not wit_shaped(m)]
        if env:
            print(f"  FAIL {rel}: {len(env)} raw env import(s): "
                  f"{', '.join(f or '<anon>' for _, f in env)}")
            worst = max(worst, 3)
        else:
            shown = ", ".join(f"{m} {f}" for m, f in imps) or "(none)"
            print(f"  ok   {rel}: 0 env, {len(imps)} import(s): {shown}")
        for m, f in odd:
            noted.append(f"{rel}: ({m!r}, {f!r})")
        swept.append(rel)

    if noted:
        print("\n  noted — imports that are neither `env` nor WIT-shaped. Outside this")
        print("  requirement's literal rule, listed so nobody assumes they were approved:")
        for n in noted:
            print(f"    {n}")

    # Census over COMMITTED files only -- `git ls-files`, not a filesystem glob.
    # A glob would sweep local build output into the ledger and fail on a clean
    # checkout that happens to have built, which is a gate that cries wolf.
    g = subprocess.run(["git", "ls-files", "*.wasm"], cwd=str(root),
                       capture_output=True, text=True)
    if g.returncode != 0:
        sys.exit("FATAL: git ls-files failed (exit 2)")
    found = {l.strip() for l in g.stdout.splitlines() if l.strip()}
    covered = set(COMPOSED) | set(NOT_COMPOSED)
    stray = sorted(found - covered)
    if stray:
        print(f"\n  FAIL: {len(stray)} committed .wasm neither swept nor ledgered:")
        for s in stray:
            print(f"    {s}")
        print("  Add it to COMPOSED if it is a composed graph, or to NOT_COMPOSED with a reason.")
        worst = max(worst, 5)

    print(f"\n  swept {len(swept)} composed artifact(s)")
    return worst


def self_test(wasm_tools: str) -> int:
    """The gate must be OBSERVED to fail on a raw env import."""
    ok = True
    with tempfile.TemporaryDirectory() as td:
        td = pathlib.Path(td)
        bad = td / "bad.wat"
        bad.write_text('(module (import "env" "mmio_read32" (func (param i32) (result i32))))')
        good = td / "good.wat"
        good.write_text('(module (import "gust:hal/mmio@0.1.0" "read32" (func (param i32) (result i32))))')
        for name, src, must_fail in (("raw env", bad, True), ("WIT-typed", good, False)):
            out = td / (src.stem + ".wasm")
            subprocess.run([wasm_tools, "parse", str(src), "-o", str(out)], check=True,
                           capture_output=True)
            imps = imports_of(out, wasm_tools)
            hit = any(m == "env" for m, _ in imps)
            if must_fail and not hit:
                print(f"  MISSED: {name} control was not caught"); ok = False
            elif not must_fail and hit:
                print(f"  BROKEN: {name} control was flagged"); ok = False
            else:
                print(f"  ok   {name} control behaves: env-detected={hit}")
    print("  self-test PASS" if ok else "  self-test FAIL")
    return 0 if ok else 6


def main() -> int:
    ap = argparse.ArgumentParser(description="VER-DRV-GRAPH-001 gate")
    ap.add_argument("--wasm-tools", default="wasm-tools")
    ap.add_argument("--self-test", action="store_true")
    a = ap.parse_args()
    if subprocess.run([a.wasm_tools, "--version"], capture_output=True).returncode != 0:
        sys.exit(f"FATAL: {a.wasm_tools} not usable (exit 2)")
    if a.self_test:
        return self_test(a.wasm_tools)
    root = pathlib.Path(__file__).resolve().parent.parent   # benches/gust
    print(f"VER-DRV-GRAPH-001 — no raw env import in the composed graph")
    rc = sweep(root, a.wasm_tools)
    print("VER-DRV-GRAPH-001: " + ("PASS" if rc == 0 else f"FAIL (exit {rc})"))
    return rc


if __name__ == "__main__":
    sys.exit(main())
