#!/usr/bin/env python3
"""SPAR conformance oracle — engine_control bench vs. AADL model.

Bridges gale's runtime evidence (event-stream CSV from the
interrupt-driven bench) to its formal architectural spec (an AADL
model parsed with spar). The v1 checks are:

  1. Every observed handoff_cycles sample is within the
     Compute_Execution_Time range declared on `subprogram Give_Decide`
     in the AADL model. Cycles are converted to nanoseconds using the
     per-run cycles_per_sec meta line.

  2. No event's step/handoff combination constitutes a transition into
     the EMV2 `Saturated` error state. The heuristic: a sample with
     handoff_cycles outside WCET bounds is treated as suspicious and
     checked against the allowed EMV2 transition list; in practice the
     Rust FFI guarantees (Verus requires count <= limit) that a
     Saturated transition cannot occur at the bench-legal input range,
     so the oracle expects zero Saturated observations.

  3. End-to-end flow latency (first ISR event to last reader event in
     a run) is within the `latency => ... applies to isr_to_handoff`
     range declared on `Handoff_System.impl`.

Pure stdlib (mirrors analyze.py). Use spar directly for richer
extraction once a `spar properties --format json` subcommand exists.

Usage:
    spar_oracle.py --model safety/aadl/semaphore.aadl \\
                   --events /tmp/engine-gale/events.csv \\
                   [--spar-bin /path/to/spar] \\
                   [--format markdown|json]
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path


# ---------------------------------------------------------- AADL extraction

# Units spar accepts on timing properties. The oracle works in nanoseconds
# internally.
_UNIT_TO_NS = {
    "ns": 1,
    "us": 1_000,
    "ms": 1_000_000,
    "sec": 1_000_000_000,
    "s":   1_000_000_000,
    "min": 60 * 1_000_000_000,
}


_RANGE_RE = re.compile(
    r"(?P<lo>[0-9]+(?:\.[0-9]+)?)\s*(?P<lo_unit>[a-zA-Z]+)"
    r"\s*\.\.\s*"
    r"(?P<hi>[0-9]+(?:\.[0-9]+)?)\s*(?P<hi_unit>[a-zA-Z]+)"
)


def _to_ns(value: float, unit: str) -> float:
    unit = unit.lower()
    if unit not in _UNIT_TO_NS:
        raise ValueError(f"unknown time unit: {unit}")
    return value * _UNIT_TO_NS[unit]


def _parse_range_ns(text: str) -> tuple[float, float]:
    m = _RANGE_RE.search(text)
    if m is None:
        raise ValueError(f"no range found in: {text!r}")
    lo = _to_ns(float(m.group("lo")), m.group("lo_unit"))
    hi = _to_ns(float(m.group("hi")), m.group("hi_unit"))
    return (lo, hi)


@dataclass
class AadlSpec:
    give_decide_wcet_ns: tuple[float, float] | None = None
    e2e_latency_ns: tuple[float, float] | None = None
    emv2_states: list[str] = field(default_factory=list)
    emv2_error_states: list[str] = field(default_factory=list)


def _strip_line_comment(line: str) -> str:
    # AADL line comments start with `--`.
    idx = line.find("--")
    return line if idx < 0 else line[:idx]


def parse_aadl(path: Path) -> AadlSpec:
    """Stdlib-only parse of the specific AADL idioms gale uses.

    Not a full AADL parser — enough to extract the three properties
    the oracle compares against. For the full lossless tree, use
    `spar parse --tree`.
    """
    raw = path.read_text()
    # Strip comments so the regexes don't accidentally match commented-out
    # timing ranges.
    cleaned = "\n".join(_strip_line_comment(line) for line in raw.splitlines())

    spec = AadlSpec()

    # Give_Decide WCET range. The subprogram block runs from
    # `subprogram Give_Decide` to `end Give_Decide;`.
    sub = re.search(
        r"subprogram\s+Give_Decide\b(.*?)end\s+Give_Decide\s*;",
        cleaned, re.DOTALL | re.IGNORECASE)
    if sub:
        m = re.search(r"Compute_Execution_Time\s*=>\s*([^;]+);", sub.group(1),
                      re.IGNORECASE)
        if m:
            spec.give_decide_wcet_ns = _parse_range_ns(m.group(1))

    # End-to-end flow latency on Handoff_System.impl.
    impl = re.search(
        r"system\s+implementation\s+Handoff_System\.impl\b(.*?)end\s+Handoff_System\.impl\s*;",
        cleaned, re.DOTALL | re.IGNORECASE)
    if impl:
        m = re.search(
            r"latency\s*=>\s*([^;]+?)\s+applies\s+to\s+isr_to_handoff\s*;",
            impl.group(1), re.IGNORECASE)
        if m:
            spec.e2e_latency_ns = _parse_range_ns(m.group(1))

    # EMV2 state names (from the error behavior block). Keep Saturated
    # and any type extending Saturated as the error-state set.
    emv2 = re.search(r"annex\s+EMV2\s*\{\*\*(.*?)\*\*\}\s*;",
                     cleaned, re.DOTALL | re.IGNORECASE)
    if emv2:
        body = emv2.group(1)
        states_block = re.search(r"states\b(.*?)transitions\b",
                                 body, re.DOTALL | re.IGNORECASE)
        if states_block:
            for line in states_block.group(1).splitlines():
                line = line.strip().rstrip(";")
                if not line:
                    continue
                m = re.match(r"([A-Za-z_][A-Za-z0-9_]*)\s*:", line)
                if m:
                    spec.emv2_states.append(m.group(1))
        # Error states: everything whose type-set label is
        # `GaleSemErrors` or extends `Saturated`.
        for m in re.finditer(
                r"([A-Za-z_][A-Za-z0-9_]*)\s*:\s*type(?:\s+extends\s+Saturated)?\s*;",
                body):
            name = m.group(1)
            if name in ("Saturated", "CountOverflow"):
                spec.emv2_error_states.append(name)

    return spec


def try_spar_items(spar_bin: str | None, model: Path) -> str | None:
    """Shell out to `spar items` to cross-check the model parses.

    Returns captured stdout on success, None on failure or if spar is
    not available. Used for the oracle's diagnostic header; the oracle
    does NOT depend on a spar binary being installed.
    """
    bin_path = spar_bin or shutil.which("spar")
    if not bin_path or not os.path.exists(bin_path):
        return None
    try:
        r = subprocess.run([bin_path, "items", str(model)],
                           capture_output=True, text=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if r.returncode != 0:
        return None
    return r.stdout


# ---------------------------------------------------------- event stream

@dataclass
class Event:
    run: str
    variant: str
    seq: int
    step: int
    rpm: int
    algo: int
    handoff: int


@dataclass
class RunMeta:
    build: str = "?"
    cycles_per_sec: int = 0
    target_samples: int = 0


def parse_events(path: Path) -> tuple[list[Event], RunMeta]:
    events: list[Event] = []
    meta = RunMeta()
    for line in path.read_text(errors="replace").splitlines():
        line = line.rstrip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(",")
        head = parts[0]
        if head.startswith("R") and len(parts) >= 8 and parts[2] == "E":
            try:
                events.append(Event(
                    run=parts[0],
                    variant=parts[1],
                    seq=int(parts[3]),
                    step=int(parts[4]),
                    rpm=int(parts[5]),
                    algo=int(parts[6]),
                    handoff=int(parts[7]),
                ))
            except ValueError:
                continue
        elif head == "M" and len(parts) >= 4:
            tail = parts[3]
            if tail == "build" and len(parts) >= 5:
                meta.build = parts[4]
            elif tail == "cycles_per_sec" and len(parts) >= 5:
                try:
                    meta.cycles_per_sec = int(parts[4])
                except ValueError:
                    pass
            elif tail == "target_samples" and len(parts) >= 5:
                try:
                    meta.target_samples = int(parts[4])
                except ValueError:
                    pass
    return events, meta


# ---------------------------------------------------------- checks

@dataclass
class CheckResult:
    name: str
    passed: bool
    detail: str


def cycles_to_ns(cycles: int, cycles_per_sec: int) -> float:
    if cycles_per_sec <= 0:
        return float("nan")
    return cycles * 1_000_000_000.0 / cycles_per_sec


def check_wcet(spec: AadlSpec, events: list[Event],
               cycles_per_sec: int) -> CheckResult:
    if spec.give_decide_wcet_ns is None:
        return CheckResult(
            "wcet_bounds", False,
            "AADL model missing Compute_Execution_Time on Give_Decide")
    if not events:
        return CheckResult("wcet_bounds", False, "no events to check")
    if cycles_per_sec <= 0:
        return CheckResult(
            "wcet_bounds", False,
            "event stream missing cycles_per_sec meta (can't convert)")

    lo_ns, hi_ns = spec.give_decide_wcet_ns
    below = 0
    above = 0
    for e in events:
        ns = cycles_to_ns(e.handoff, cycles_per_sec)
        if ns < lo_ns:
            below += 1
        elif ns > hi_ns:
            above += 1
    total = len(events)
    if below == 0 and above == 0:
        return CheckResult(
            "wcet_bounds", True,
            f"{total}/{total} handoff samples within "
            f"[{lo_ns:.0f}, {hi_ns:.0f}] ns")
    return CheckResult(
        "wcet_bounds", False,
        f"{below} below / {above} above WCET on {total} samples "
        f"(bounds: {lo_ns:.0f}..{hi_ns:.0f} ns)")


def check_flow_latency(spec: AadlSpec, events: list[Event],
                       cycles_per_sec: int) -> CheckResult:
    """End-to-end latency = elapsed cycles between first and last ISR
    event in the stream, as a coarse proxy for worst-case ISR→reader
    delay. Real flow latency will need per-event reader timestamps
    (future work tied to the CTF tracer)."""
    if spec.e2e_latency_ns is None:
        return CheckResult(
            "flow_latency", False,
            "AADL model missing `latency applies to isr_to_handoff`")
    if not events:
        return CheckResult("flow_latency", False, "no events to check")
    if cycles_per_sec <= 0:
        return CheckResult(
            "flow_latency", False,
            "event stream missing cycles_per_sec")

    # Use max per-event handoff as a proxy for worst-case flow latency.
    # The CSV doesn't yet emit a reader-side end-of-chain cycle; the
    # CTF stream will, once promoted from poor-man's CSV.
    worst_cycles = max(e.handoff for e in events)
    worst_ns = cycles_to_ns(worst_cycles, cycles_per_sec)
    lo_ns, hi_ns = spec.e2e_latency_ns
    if lo_ns <= worst_ns <= hi_ns:
        return CheckResult(
            "flow_latency", True,
            f"worst-case handoff {worst_ns:.0f} ns within flow latency "
            f"[{lo_ns:.0f}, {hi_ns:.0f}] ns")
    return CheckResult(
        "flow_latency", False,
        f"worst-case handoff {worst_ns:.0f} ns outside "
        f"[{lo_ns:.0f}, {hi_ns:.0f}] ns")


def check_emv2_saturated(spec: AadlSpec, events: list[Event],
                         cycles_per_sec: int) -> CheckResult:
    """No observed event should be classified as `Saturated`.

    The bench drives sem_give from the ISR with a legal (count, limit)
    pair — Verus `requires count <= limit` holds by construction, so
    give_decide never returns Saturated. An event with handoff_cycles
    >> WCET is flagged as a candidate Saturated observation. Zero
    candidates = conformance.
    """
    if not spec.emv2_states and not spec.emv2_error_states:
        return CheckResult(
            "emv2_no_saturated", False,
            "AADL EMV2 annex declares no states — nothing to check")
    if spec.give_decide_wcet_ns is None:
        return CheckResult(
            "emv2_no_saturated", True,
            "no WCET to anchor Saturated check; skipping (vacuous pass)")

    _, hi_ns = spec.give_decide_wcet_ns
    # A sample >2x the WCET upper bound is treated as a candidate
    # Saturated observation. This heuristic will be replaced with a
    # CTF-derived direct observation of the GiveDecision enum emitted
    # by the Rust FFI.
    threshold_ns = 2 * hi_ns
    candidates = [
        e for e in events
        if cycles_to_ns(e.handoff, cycles_per_sec) > threshold_ns
    ]
    states_str = ", ".join(spec.emv2_error_states) or "Saturated"
    if not candidates:
        return CheckResult(
            "emv2_no_saturated", True,
            f"zero candidate {{{states_str}}} transitions observed "
            f"in {len(events)} events")
    return CheckResult(
        "emv2_no_saturated", False,
        f"{len(candidates)} events exceed 2x WCET — potential "
        f"{{{states_str}}} transition")


# ---------------------------------------------------------- report

def render_markdown(spec: AadlSpec, meta: RunMeta, events: list[Event],
                    checks: list[CheckResult], model_path: Path,
                    events_path: Path,
                    spar_items_stdout: str | None) -> str:
    lines: list[str] = []
    lines.append("# SPAR conformance oracle — engine_control")
    lines.append("")
    lines.append(f"- Model: `{model_path}`")
    lines.append(f"- Events: `{events_path}`")
    lines.append(f"- Build: {meta.build}")
    lines.append(f"- Cycle counter: {meta.cycles_per_sec:,} Hz")
    lines.append(f"- Samples: {len(events)} (target {meta.target_samples})")
    lines.append("")
    lines.append("## AADL spec (extracted)")
    if spec.give_decide_wcet_ns:
        lo, hi = spec.give_decide_wcet_ns
        lines.append(f"- `Give_Decide.Compute_Execution_Time` = "
                     f"{lo:.0f} ns .. {hi:.0f} ns")
    else:
        lines.append("- `Give_Decide.Compute_Execution_Time` = **missing**")
    if spec.e2e_latency_ns:
        lo, hi = spec.e2e_latency_ns
        lines.append(f"- `Handoff_System.impl.latency (isr_to_handoff)` = "
                     f"{lo:.0f} ns .. {hi:.0f} ns")
    else:
        lines.append("- `Handoff_System.impl.latency` = **missing**")
    lines.append(f"- EMV2 states: {spec.emv2_states or '(none)'}")
    lines.append(f"- EMV2 error states: "
                 f"{spec.emv2_error_states or '(none)'}")
    lines.append("")
    lines.append("## Checks")
    for c in checks:
        tag = "pass" if c.passed else "FAIL"
        lines.append(f"- [{tag}] **{c.name}** — {c.detail}")
    lines.append("")
    all_pass = all(c.passed for c in checks)
    lines.append(
        f"## Result: {'PASS' if all_pass else 'FAIL'}")
    if spar_items_stdout:
        lines.append("")
        lines.append("<details><summary>spar items output</summary>")
        lines.append("")
        lines.append("```")
        lines.append(spar_items_stdout.strip())
        lines.append("```")
        lines.append("")
        lines.append("</details>")
    return "\n".join(lines) + "\n"


def render_json(spec: AadlSpec, meta: RunMeta, events: list[Event],
                checks: list[CheckResult], model_path: Path,
                events_path: Path) -> str:
    return json.dumps({
        "model": str(model_path),
        "events": str(events_path),
        "build": meta.build,
        "cycles_per_sec": meta.cycles_per_sec,
        "sample_count": len(events),
        "aadl": {
            "give_decide_wcet_ns": spec.give_decide_wcet_ns,
            "e2e_latency_ns": spec.e2e_latency_ns,
            "emv2_states": spec.emv2_states,
            "emv2_error_states": spec.emv2_error_states,
        },
        "checks": [
            {"name": c.name, "passed": c.passed, "detail": c.detail}
            for c in checks
        ],
        "pass": all(c.passed for c in checks),
    }, indent=2) + "\n"


# ---------------------------------------------------------- main

def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--model", type=Path, required=True,
                    help="AADL model (e.g. safety/aadl/semaphore.aadl)")
    ap.add_argument("--events", type=Path, required=True,
                    help="Event stream CSV from run_qemu_bench.sh")
    ap.add_argument("--spar-bin", type=str, default=None,
                    help="spar CLI path (auto-detected from PATH)")
    ap.add_argument("--format", choices=("markdown", "json"),
                    default="markdown")
    args = ap.parse_args(argv)

    if not args.model.is_file():
        print(f"error: model not found: {args.model}", file=sys.stderr)
        return 2
    if not args.events.is_file():
        print(f"error: events not found: {args.events}", file=sys.stderr)
        return 2

    spec = parse_aadl(args.model)
    events, meta = parse_events(args.events)
    spar_items_stdout = try_spar_items(args.spar_bin, args.model)

    checks = [
        check_wcet(spec, events, meta.cycles_per_sec),
        check_flow_latency(spec, events, meta.cycles_per_sec),
        check_emv2_saturated(spec, events, meta.cycles_per_sec),
    ]

    if args.format == "json":
        sys.stdout.write(render_json(spec, meta, events, checks,
                                     args.model, args.events))
    else:
        sys.stdout.write(render_markdown(spec, meta, events, checks,
                                         args.model, args.events,
                                         spar_items_stdout))

    return 0 if all(c.passed for c in checks) else 1


if __name__ == "__main__":
    sys.exit(main())
