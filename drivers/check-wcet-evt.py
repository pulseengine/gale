#!/usr/bin/env python3
"""check-wcet-evt.py — VER-OS-WCET-001's kill-criterion, made mechanical.

T4 / v0.6.0 workstream D1. `REQ-OS-WCET-001` gets sound per-function STATIC cycle
bounds from synth itself (`--emit-wcet`, schema `synth-wcet-v1`; emitted + gated by
`drivers/emit-wcet.sh`). `VER-OS-WCET-001` says those bounds only count as evidence
if something can falsify them:

    "The static bound SHALL dominate both the statistical bound and every observed
     raw DWT high-water-mark for that function.  Kill-criterion: a function's static
     bound is exceeded by its statistical bound or by an observed DWT sample [...]"

This script is that kill-criterion as a build gate. It takes the semihosting capture
of `src/bin/gust_wcet_evt.rs` (real STM32F100 silicon, DWT CYCCNT) plus the committed
sidecar, and exits 1 if either half of the domination claim is violated.

DIRECTION OF EVIDENCE — read this before quoting any number below. DWT measurement
may only ever FALSIFY the static model. Nothing here licenses sizing a partition
budget from a measured value; that is the standing rule in `drivers/emit-wcet.sh` and
the second half of the artifact's kill-criterion.

WHAT THE "STATISTICAL BOUND" IS, HONESTLY. The estimator is block maxima: split the
N samples into >=5 blocks, take each block's max, report max(block maxima) — the
classical EVT block-maxima statistic. For a straight-line, branch-free function like
`gust:os/time#deadline` / `#elapsed` the per-sample distribution is DEGENERATE (the
same instruction sequence every time on an in-order, zero-wait-state core), so the
block maxima collapse onto a single value and the statistic is tight BY CONSTRUCTION
and numerically equal to the observed max. That is a real property of the code under
test, not a defect of the estimator — but it does mean this is a CROSS-CHECK of the
static model against silicon, NOT an independent statistical bound. A genuine
Gumbel/GEV tail fit needs a non-degenerate sample distribution (input-dependent
paths, caches, contention); none of those exist on this target for these functions.
The dispersion of the block maxima is reported so a reader can see this for
themselves rather than take it on trust.

USAGE
    check-wcet-evt.py CAPTURE [--sidecar PATH]     gate a capture (exit 0/1)
    check-wcet-evt.py -       [--sidecar PATH]     ... from stdin
    check-wcet-evt.py --emit-sample-capture pass   print a synthetic PASSING capture
    check-wcet-evt.py --emit-sample-capture fail   print a synthetic OVER-BOUND capture
    check-wcet-evt.py --self-test                  demonstrate the red state; EXITS 1

HOW TO PRODUCE A CAPTURE
    Build `src/bin/gust_wcet_evt.rs` for the F100 (see its module doc), flash the
    STM32VLDISCOVERY and stream semihosting exactly as `silicon/run-adc.sh` does for
    the ADC anchor, then:  ... | check-wcet-evt.py -
    qemu is NOT a substitute: it models no DWT, and the firmware refuses to emit
    timings there rather than reporting emulated numbers.

INPUT CONTRACT (printed by gust_wcet_evt.rs)
    WCET-EVT CAL read_read=<c> call_shim=<c> path_variants=<cold+warm|warm-only>
    WCET-EVT-SAMPLE <fn> i=<idx> cyc=<c> path=<cold|warm>      (optional, repeated)
    WCET-EVT <fn> n=<N> min=<c> max=<c> mean=<c> overhead=<c>
    WCET-EVT DONE

Stdlib only, integer arithmetic only, no third-party dependencies.
"""

import argparse
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT_SIDECAR = os.path.join(HERE, "os-node", "repro-757", "os-tl.wcet.json")

MIN_SAMPLES = 30  # SWREQ-MEAS-MS02: N >= 30
MIN_BLOCKS = 5  # block-maxima needs enough blocks to be a maxima series at all

RE_CAL = re.compile(
    r"^WCET-EVT\s+CAL\s+read_read=(\d+)\s+call_shim=(\d+)\s+path_variants=(\S+)\s*$"
)
RE_SAMPLE = re.compile(
    r"^WCET-EVT-SAMPLE\s+(\S+)\s+i=(\d+)\s+cyc=(\d+)\s+path=(\S+)\s*$"
)
RE_SUMMARY = re.compile(
    r"^WCET-EVT\s+(?!CAL\b|DONE\b)(\S+)\s+n=(\d+)\s+min=(\d+)\s+max=(\d+)"
    r"\s+mean=(\d+)\s+overhead=(\d+)\s*$"
)
RE_DONE = re.compile(r"^WCET-EVT\s+DONE\s*$")


# --------------------------------------------------------------------------
# sidecar
# --------------------------------------------------------------------------
def load_sidecar(path):
    """Read synth's static bounds. Never hardcode a cycle count anywhere."""
    with open(path) as fh:
        doc = json.load(fh)
    if doc.get("schema") != "synth-wcet-v1":
        raise SystemExit(
            f"check-wcet-evt: {path}: schema {doc.get('schema')!r}, expected 'synth-wcet-v1'"
        )
    return doc


def static_bound(doc, short_name, sidecar_path):
    """Resolve a printed short name (e.g. 'deadline') to its sidecar entry.

    Matched by WIT-export suffix ('#<short>'), which must be UNAMBIGUOUS: two
    functions ending in the same name would make the bound lookup a coin flip, so
    that is an error rather than a pick-the-first.
    """
    hits = [f for f in doc["functions"] if f["name"].endswith("#" + short_name)]
    if not hits:
        hits = [f for f in doc["functions"] if f["name"] == short_name]
    if len(hits) != 1:
        raise SystemExit(
            f"check-wcet-evt: {len(hits)} sidecar entries match '#{short_name}' in "
            f"{sidecar_path} (need exactly 1): {[f['name'] for f in hits]}"
        )
    fn = hits[0]
    if fn.get("status") != "bounded":
        raise SystemExit(
            f"check-wcet-evt: sidecar entry {fn['name']} has status "
            f"{fn.get('status')!r} (reason: {fn.get('reason')!r}) — there is no static "
            f"bound to cross-check. Measuring it is fine; GATING it is not."
        )
    return fn["name"], int(fn["cycles"]), int(fn.get("instr_count", 0))


# --------------------------------------------------------------------------
# EVT block maxima (integer, dependency-free)
# --------------------------------------------------------------------------
def block_maxima(samples, min_blocks=MIN_BLOCKS):
    """Split into as many equal blocks as possible (>= min_blocks) and return the
    per-block maxima. Any remainder samples are appended to the last block so no
    observation is silently dropped."""
    n = len(samples)
    nblocks = min_blocks
    # prefer ~sqrt(n) blocks (a common block-maxima heuristic), floored at min_blocks
    k = max(min_blocks, int(n ** 0.5))
    while k > min_blocks and n // k < 2:
        k -= 1
    nblocks = k if n // k >= 2 else min_blocks
    size = n // nblocks
    maxima = []
    for b in range(nblocks):
        lo = b * size
        hi = n if b == nblocks - 1 else (b + 1) * size
        maxima.append(max(samples[lo:hi]))
    return maxima


# --------------------------------------------------------------------------
# capture parsing
# --------------------------------------------------------------------------
def parse_capture(text):
    cal = None
    samples = {}
    summaries = []
    done = False
    for line in text.splitlines():
        line = line.strip()
        m = RE_CAL.match(line)
        if m:
            cal = {
                "read_read": int(m.group(1)),
                "call_shim": int(m.group(2)),
                "path_variants": m.group(3),
            }
            continue
        m = RE_SAMPLE.match(line)
        if m:
            samples.setdefault(m.group(1), []).append(
                {"i": int(m.group(2)), "cyc": int(m.group(3)), "path": m.group(4)}
            )
            continue
        m = RE_SUMMARY.match(line)
        if m:
            summaries.append(
                {
                    "fn": m.group(1),
                    "n": int(m.group(2)),
                    "min": int(m.group(3)),
                    "max": int(m.group(4)),
                    "mean": int(m.group(5)),
                    "overhead": int(m.group(6)),
                }
            )
            continue
        if RE_DONE.match(line):
            done = True
    return cal, samples, summaries, done


# --------------------------------------------------------------------------
# the gate
# --------------------------------------------------------------------------
def gate(text, sidecar_path, out=sys.stdout):
    """Return 0 (pass) / 1 (fail). Every failure prints WHY and WHICH artifact
    clause it violates."""
    doc = load_sidecar(sidecar_path)
    cal, samples, summaries, done = parse_capture(text)
    fails = []
    notes = []

    print(
        f"check-wcet-evt: VER-OS-WCET-001 cross-check\n"
        f"  sidecar  : {sidecar_path}\n"
        f"  schema   : {doc['schema']}  core_class: {doc.get('core_class')}  "
        f"wait_states: {doc.get('wait_states')}\n"
        f"  model    : {doc.get('memory_assumption')}",
        file=out,
    )

    if not summaries:
        print(
            "  FAIL: no 'WCET-EVT <fn> n=.. min=.. max=.. mean=..' line in the capture "
            "— nothing to gate (truncated capture, or the firmware never ran).",
            file=out,
        )
        return 1
    if not done:
        fails.append(
            "capture has no 'WCET-EVT DONE' terminator — it is TRUNCATED, so an "
            "over-bound sample could be sitting in the part that was lost"
        )
    if cal is None:
        notes.append(
            "no 'WCET-EVT CAL' line: measurement overhead is unknown, so the "
            "call-overhead attribution below is unavailable"
        )
    else:
        print(
            f"  overhead : read_read={cal['read_read']} cyc (SUBTRACTED from every "
            f"sample)  call_shim={cal['call_shim']} cyc (bl + bx lr; NOT subtracted "
            f"— samples stay conservative)\n"
            f"  paths    : {cal['path_variants']}",
            file=out,
        )
        if cal["path_variants"] != "cold+warm":
            notes.append(
                f"path_variants={cal['path_variants']}: the firmware could NOT force "
                "the once-init store path, so the longest path the static bound "
                "covers was not exercised — the cross-check is weaker than intended"
            )

    print(file=out)
    for s in summaries:
        short = s["fn"]
        full, static, instr = static_bound(doc, short, sidecar_path)
        raw = [x["cyc"] for x in sorted(samples.get(short, []), key=lambda d: d["i"])]

        # ---- sample-count / integrity checks -----------------------------
        if s["n"] < MIN_SAMPLES:
            fails.append(
                f"{short}: n={s['n']} < {MIN_SAMPLES} (SWREQ-MEAS-MS02 requires N>=30)"
            )
        if raw and len(raw) != s["n"]:
            fails.append(
                f"{short}: summary says n={s['n']} but {len(raw)} WCET-EVT-SAMPLE lines "
                f"were captured — the capture is inconsistent"
            )
        if raw and max(raw) != s["max"]:
            fails.append(
                f"{short}: summary max={s['max']} disagrees with the raw samples "
                f"(max={max(raw)}) — the capture is inconsistent"
            )

        # ---- the statistical (EVT block-maxima) bound ---------------------
        if raw and len(raw) >= MIN_SAMPLES:
            maxima = block_maxima(raw)
            stat = max(maxima)
            disp = max(maxima) - min(maxima)
            stat_src = (
                f"block maxima over {len(maxima)} blocks of ~{len(raw)//len(maxima)}, "
                f"dispersion {disp} cyc"
            )
            if disp == 0:
                stat_src += " (DEGENERATE: every block max identical — tight by "
                stat_src += "construction, see the header)"
            projected = stat + disp  # informational tail headroom only; NOT gated
        else:
            maxima, disp = [], None
            stat = s["max"]
            stat_src = (
                "no raw WCET-EVT-SAMPLE lines (or n<30): falling back to the reported "
                "max as a DEGENERATE stand-in — this is NOT a block-maxima estimate"
            )
            projected = None
            notes.append(
                f"{short}: statistical bound could not be computed from raw samples; "
                f"used the reported max instead"
            )

        # ---- the artifact's two domination clauses ------------------------
        obs_bad = s["max"] > static
        stat_bad = stat > static
        verdict = "FAIL" if (obs_bad or stat_bad) else "PASS"
        print(
            f"  [{verdict}] {short}  ->  {full}\n"
            f"        static (synth --emit-wcet) : {static:>6} cyc "
            f"({instr} instr, model-sound)\n"
            f"        statistical (EVT blk-max)  : {stat:>6} cyc   [{stat_src}]\n"
            f"        observed max (raw DWT)     : {s['max']:>6} cyc   "
            f"(n={s['n']}, min={s['min']}, mean={s['mean']})\n"
            f"        margin static-observed     : {static - s['max']:>6} cyc",
            file=out,
        )
        if projected is not None and disp:
            flag = " (ABOVE the static bound)" if projected > static else ""
            print(
                f"        [info, NOT gated] tail-headroom projection "
                f"max(blk)+dispersion = {projected} cyc{flag}",
                file=out,
            )

        if obs_bad:
            msg = (
                f"{short}: observed DWT max {s['max']} > static bound {static} "
                f"(+{s['max'] - static}) — VER-OS-WCET-001 kill-criterion: "
                f"'a function's static bound is exceeded by [...] an observed DWT sample'"
            )
            if cal and 0 < s["max"] - static <= cal["call_shim"]:
                msg += (
                    f"\n        ATTRIBUTION: the exceedance ({s['max'] - static}) is "
                    f"<= the calibrated call_shim overhead ({cal['call_shim']} cyc). "
                    f"Reported samples still contain the caller's `bl`, which is NOT "
                    f"part of the callee's static bound. This may be measurement "
                    f"attribution rather than an unsound bound — it is reported, not "
                    f"absorbed: the gate stays RED for a human to adjudicate."
                )
            fails.append(msg)
        if stat_bad and not obs_bad:
            fails.append(
                f"{short}: statistical bound {stat} > static bound {static} — "
                f"VER-OS-WCET-001 kill-criterion: 'a function's static bound is "
                f"exceeded by its statistical bound'"
            )
        print(file=out)

    for n in notes:
        print(f"  note: {n}", file=out)
    if notes:
        print(file=out)

    if fails:
        print("check-wcet-evt: GATE RED — VER-OS-WCET-001 kill-criterion met:", file=out)
        for f in fails:
            print(f"  * {f}", file=out)
        return 1

    print(
        "check-wcet-evt: GATE GREEN — the static bound dominates both the statistical "
        "bound and every observed raw DWT sample, for every gated function.\n"
        "  Reminder: this cross-checks the static model; it does NOT license sizing "
        "any partition budget from a measured value.",
        file=out,
    )
    return 0


# --------------------------------------------------------------------------
# synthetic captures (fixtures for the oracles, and for --self-test)
# --------------------------------------------------------------------------
def synth_capture(mode, sidecar_path):
    """Build a synthetic capture from the REAL sidecar bounds.

    'pass' sits a fixed margin under each static bound; 'fail' pushes exactly one
    sample of one function one cycle over it. Neither hardcodes a cycle count —
    both are derived from the sidecar, so the fixtures stay correct if synth's
    model changes.
    """
    doc = load_sidecar(sidecar_path)
    lines = ["gust-wcet-evt: SYNTHETIC capture (%s) — NOT a silicon measurement" % mode]
    lines.append("WCET-EVT CAL read_read=2 call_shim=7 path_variants=cold+warm")
    n = 40
    for short in ("deadline", "elapsed"):
        _full, static, _instr = static_bound(doc, short, sidecar_path)
        base = max(1, static - 5)
        vals = []
        for i in range(n):
            # A small deterministic ripple so min/mean/max are distinct. NOTE the
            # block maxima still come out degenerate — which is exactly what real
            # silicon looks like for these functions, so the fixture is faithful.
            vals.append(base + (i % 3))
        if mode == "fail" and short == "deadline":
            vals[17] = static + 1  # one sample, one cycle over: the kill-criterion
        for i, v in enumerate(vals):
            lines.append(
                "WCET-EVT-SAMPLE %s i=%d cyc=%d path=%s"
                % (short, i, v, "cold" if i < n // 2 else "warm")
            )
        lines.append(
            "WCET-EVT %s n=%d min=%d max=%d mean=%d overhead=2"
            % (short, n, min(vals), max(vals), sum(vals) // len(vals))
        )
    lines.append("WCET-EVT DONE")
    return "\n".join(lines) + "\n"


def self_test(sidecar_path):
    """Prove the gate can go RED without hardware.

    Runs the PASSING fixture (must be green) and then the OVER-BOUND fixture (must
    be red), and EXITS 1 BY DESIGN so the red state is directly observable in the
    oracle transcript. Exit 2 means the self-test itself is broken.
    """
    print("=" * 78)
    print("check-wcet-evt --self-test: case 1/2 — synthetic PASSING capture")
    print("=" * 78)
    rc_pass = gate(synth_capture("pass", sidecar_path), sidecar_path)
    print()
    if rc_pass != 0:
        print(
            "check-wcet-evt SELF-TEST BROKEN: the passing fixture did not go green "
            f"(rc={rc_pass}); the gate cannot be trusted in either direction."
        )
        return 2

    print("=" * 78)
    print(
        "check-wcet-evt --self-test: case 2/2 — synthetic OVER-BOUND capture\n"
        "(one deadline sample = static bound + 1; this MUST go red)"
    )
    print("=" * 78)
    rc_fail = gate(synth_capture("fail", sidecar_path), sidecar_path)
    print()
    if rc_fail != 1:
        print(
            "check-wcet-evt SELF-TEST BROKEN: the over-bound fixture did NOT go red "
            f"(rc={rc_fail}) — the kill-criterion is not wired up."
        )
        return 2

    print(
        "check-wcet-evt --self-test: OK — passing fixture green (exit 0), over-bound "
        "fixture red (exit 1).\n"
        "Exiting 1 BY DESIGN: --self-test reports the demonstrated RED verdict, so a "
        "nonzero exit here is the expected, correct result."
    )
    return 1


def main(argv=None):
    ap = argparse.ArgumentParser(
        description=(
            "VER-OS-WCET-001 cross-check: fail if a measured or EVT-statistical "
            "bound exceeds synth's static bound."
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    ap.add_argument(
        "capture",
        nargs="?",
        help="semihosting capture from gust_wcet_evt ('-' for stdin)",
    )
    ap.add_argument("--sidecar", default=DEFAULT_SIDECAR, help="synth-wcet-v1 JSON")
    ap.add_argument(
        "--self-test",
        action="store_true",
        help="demonstrate the gate's red state without hardware; EXITS 1 BY DESIGN",
    )
    ap.add_argument(
        "--emit-sample-capture",
        choices=("pass", "fail"),
        help="print a synthetic capture (derived from the sidecar) and exit 0",
    )
    args = ap.parse_args(argv)

    if args.emit_sample_capture:
        sys.stdout.write(synth_capture(args.emit_sample_capture, args.sidecar))
        return 0
    if args.self_test:
        return self_test(args.sidecar)
    if not args.capture:
        ap.error("a CAPTURE path (or '-') is required unless --self-test/--emit-sample-capture")

    text = sys.stdin.read() if args.capture == "-" else open(args.capture).read()
    return gate(text, args.sidecar)


if __name__ == "__main__":
    sys.exit(main())
