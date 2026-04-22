#!/usr/bin/env python3
"""Compare two engine_control CSV outputs (baseline vs gale).

Usage:
  ./compare.py <baseline.csv> <gale.csv>

Reads the custom CSV emitted by src/main.c (H,<tag>,<bucket>,<lo>,<hi>,<count>
rows + scalar summary lines) and prints a markdown table showing deltas
for mean / min / max and the histogram overlap.
"""

from __future__ import annotations

import sys
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class Report:
    build: str = "?"
    hz: int = 0
    samples: int = 0
    drops: int = 0
    algo_min: int = 0
    algo_max: int = 0
    algo_mean: int = 0
    handoff_min: int = 0
    handoff_max: int = 0
    handoff_mean: int = 0
    # tag -> bucket -> count
    hist: dict[str, dict[int, int]] = field(default_factory=dict)


def parse(path: Path) -> Report:
    r = Report()
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or line.startswith("==="):
            continue
        parts = line.split(",")
        head = parts[0]
        if head == "build":
            r.build = parts[1]
        elif head == "cycles_per_sec":
            r.hz = int(parts[1])
        elif head == "samples":
            r.samples = int(parts[1])
        elif head == "drops":
            r.drops = int(parts[1])
        elif head == "algo" and len(parts) >= 3:
            setattr(r, f"algo_{parts[1]}", int(parts[2]))
        elif head == "handoff" and len(parts) >= 3:
            setattr(r, f"handoff_{parts[1]}", int(parts[2]))
        elif head == "H" and len(parts) >= 6:
            _, tag, bucket, _lo, _hi, count = parts[:6]
            r.hist.setdefault(tag, {})[int(bucket)] = int(count)
    return r


def cycles_to_ns(cycles: int, hz: int) -> float:
    return cycles * 1e9 / hz if hz else 0.0


def delta_pct(baseline: int, gale: int) -> str:
    if baseline == 0:
        return "—"
    pct = (gale - baseline) * 100.0 / baseline
    arrow = "↓" if pct < 0 else ("↑" if pct > 0 else "=")
    return f"{pct:+.1f}% {arrow}"


def render(baseline: Report, gale: Report) -> str:
    out: list[str] = []
    out.append("# Engine-control benchmark — baseline vs Gale\n")
    out.append(f"- Baseline build: `{baseline.build}` "
               f"({baseline.samples} samples, {baseline.drops} drops)")
    out.append(f"- Gale build: `{gale.build}` "
               f"({gale.samples} samples, {gale.drops} drops)")
    out.append(f"- Cycle counter: {baseline.hz:,} Hz "
               f"(1 cycle ≈ {1e9/baseline.hz:.1f} ns)\n")

    out.append("## Algorithm-only (pure C, should be identical)\n")
    out.append("| Metric | Baseline (cyc/ns) | Gale (cyc/ns) | Δ |")
    out.append("|---|---|---|---|")
    for metric, b, g in [
        ("min",  baseline.algo_min,  gale.algo_min),
        ("mean", baseline.algo_mean, gale.algo_mean),
        ("max",  baseline.algo_max,  gale.algo_max),
    ]:
        bn = cycles_to_ns(b, baseline.hz)
        gn = cycles_to_ns(g, gale.hz)
        out.append(f"| {metric} | {b} / {bn:.0f}ns | {g} / {gn:.0f}ns | "
                   f"{delta_pct(b, g)} |")

    out.append("\n## Primitive handoff — `ring_buf_put` + `k_sem_give`\n")
    out.append("The interesting comparison. This is the Zephyr primitive chain "
               "Gale replaces with formally verified Rust.\n")
    out.append("| Metric | Baseline (cyc/ns) | Gale (cyc/ns) | Δ |")
    out.append("|---|---|---|---|")
    for metric, b, g in [
        ("min",  baseline.handoff_min,  gale.handoff_min),
        ("mean", baseline.handoff_mean, gale.handoff_mean),
        ("max",  baseline.handoff_max,  gale.handoff_max),
    ]:
        bn = cycles_to_ns(b, baseline.hz)
        gn = cycles_to_ns(g, gale.hz)
        out.append(f"| {metric} | {b} / {bn:.0f}ns | {g} / {gn:.0f}ns | "
                   f"{delta_pct(b, g)} |")

    out.append("\n## Histogram overlap (handoff tag)\n")
    out.append("Bucket counts. Different distributions = meaningfully "
               "different latency profiles.\n")
    b_h = baseline.hist.get("handoff", {})
    g_h = gale.hist.get("handoff", {})
    keys = sorted(set(b_h) | set(g_h))
    out.append("| Bucket | Cycle range | Baseline | Gale |")
    out.append("|---|---|---|---|")
    for k in keys:
        lo = 0 if k == 0 else (1 << k)
        hi = (1 << (k + 1)) - 1
        out.append(f"| {k} | {lo}–{hi} | {b_h.get(k, 0)} | {g_h.get(k, 0)} |")

    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    baseline = parse(Path(argv[1]))
    gale = parse(Path(argv[2]))
    sys.stdout.write(render(baseline, gale))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
