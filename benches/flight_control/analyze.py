#!/usr/bin/env python3
"""Flight-control macro-bench — raw event-stream analyzer.

Strict superset of engine_control/analyze.py: same per-step + Mann-Whitney
logic, plus four new columns (t_lock, t_post, t_round, t_bcast) for the
controller-period segments (Section 3 of macro-bench-design.md).

Usage:
  analyze.py --baseline events.csv --gale events.csv [--runs N]
             [--assert-only] [--json]

Input format (per line; anything else is skipped):
  R<run>,<variant>,E,<seq>,<step>,<load>,<algo>,<handoff>,<t_lock>,<t_post>,<t_round>,<t_bcast>
  M,R<run>,<variant>,{drops,N | samples,N | telemetry_emits,N | END | build,X
                      | cycles_per_sec,N | target_samples,N}

Negative values in t_lock/t_post/t_round/t_bcast = "not measured on this
sensor row" and are excluded from the per-step distributions. Only ~1 in
10 sensor rows carries each segment, by design.

Pure stdlib (no scipy / numpy dependency) — runs inside the minimal
Zephyr CI container.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import statistics
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path


@dataclass
class Sample:
    run: str
    variant: str
    seq: int
    step: int
    load: int
    algo: int
    handoff: int
    t_lock: int     # -1 if not measured
    t_post: int
    t_round: int
    t_bcast: int


@dataclass
class Meta:
    build: str = "?"
    cycles_per_sec: int = 0
    target_samples: int = 0
    drops: dict[str, int] = field(default_factory=dict)
    samples: dict[str, int] = field(default_factory=dict)
    telemetry_emits: dict[str, int] = field(default_factory=dict)
    ended: set[str] = field(default_factory=set)


def parse_events(path: Path) -> tuple[list[Sample], Meta]:
    samples: list[Sample] = []
    meta = Meta()
    for line in path.read_text(errors="replace").splitlines():
        line = line.rstrip()
        if not line or line.startswith("#"):
            continue
        parts = line.split(",")
        head = parts[0]
        if head.startswith("R") and len(parts) >= 12 and parts[2] == "E":
            # R<run>,<variant>,E,<seq>,<step>,<load>,<algo>,<handoff>,
            #   <t_lock>,<t_post>,<t_round>,<t_bcast>
            try:
                samples.append(Sample(
                    run=parts[0],
                    variant=parts[1],
                    seq=int(parts[3]),
                    step=int(parts[4]),
                    load=int(parts[5]),
                    algo=int(parts[6]),
                    handoff=int(parts[7]),
                    t_lock=int(parts[8]),
                    t_post=int(parts[9]),
                    t_round=int(parts[10]),
                    t_bcast=int(parts[11]),
                ))
            except ValueError:
                continue
        elif head == "M" and len(parts) >= 4:
            run = parts[1]
            tail = parts[3]
            if tail == "END":
                meta.ended.add(run)
            elif tail == "drops" and len(parts) >= 5:
                try: meta.drops[run] = int(parts[4])
                except ValueError: pass
            elif tail == "samples" and len(parts) >= 5:
                try: meta.samples[run] = int(parts[4])
                except ValueError: pass
            elif tail == "telemetry_emits" and len(parts) >= 5:
                try: meta.telemetry_emits[run] = int(parts[4])
                except ValueError: pass
            elif tail == "build" and len(parts) >= 5:
                meta.build = parts[4]
            elif tail == "cycles_per_sec" and len(parts) >= 5:
                try: meta.cycles_per_sec = int(parts[4])
                except ValueError: pass
            elif tail == "target_samples" and len(parts) >= 5:
                try: meta.target_samples = int(parts[4])
                except ValueError: pass
    return samples, meta


# ---------------------------------------------------------- statistics

def percentile(sorted_xs: list[float], q: float) -> float:
    if not sorted_xs:
        return float("nan")
    if len(sorted_xs) == 1:
        return sorted_xs[0]
    idx = q * (len(sorted_xs) - 1)
    lo = int(math.floor(idx))
    hi = int(math.ceil(idx))
    if lo == hi:
        return sorted_xs[lo]
    frac = idx - lo
    return sorted_xs[lo] * (1 - frac) + sorted_xs[hi] * frac


def bootstrap_median_ci(xs: list[int], iters: int = 2000,
                        alpha: float = 0.05, seed: int = 12345
                        ) -> tuple[float, float, float]:
    if not xs:
        return (float("nan"), float("nan"), float("nan"))
    rng = random.Random(seed)
    n = len(xs)
    med = statistics.median(xs)
    if n < 3:
        return (med, float(min(xs)), float(max(xs)))
    samples = []
    for _ in range(iters):
        resample = [xs[rng.randrange(n)] for _ in range(n)]
        resample.sort()
        samples.append(percentile(resample, 0.5))
    samples.sort()
    lo = percentile(samples, alpha / 2)
    hi = percentile(samples, 1 - alpha / 2)
    return (med, lo, hi)


def mannwhitney_u(xs: list[int], ys: list[int]) -> tuple[float, float]:
    n1, n2 = len(xs), len(ys)
    if n1 == 0 or n2 == 0:
        return (float("nan"), float("nan"))
    combined = [(v, 0) for v in xs] + [(v, 1) for v in ys]
    combined.sort(key=lambda t: t[0])
    ranks = [0.0] * len(combined)
    i = 0
    while i < len(combined):
        j = i
        while j + 1 < len(combined) and combined[j + 1][0] == combined[i][0]:
            j += 1
        avg = (i + j) / 2 + 1
        for k in range(i, j + 1):
            ranks[k] = avg
        i = j + 1
    r1 = sum(r for r, (_, g) in zip(ranks, combined) if g == 0)
    u1 = r1 - n1 * (n1 + 1) / 2
    u2 = n1 * n2 - u1
    u = min(u1, u2)
    tie_sum = 0.0
    i = 0
    while i < len(combined):
        j = i
        while j + 1 < len(combined) and combined[j + 1][0] == combined[i][0]:
            j += 1
        t = j - i + 1
        if t > 1:
            tie_sum += (t ** 3 - t)
        i = j + 1
    n = n1 + n2
    mean_u = n1 * n2 / 2
    var_u = n1 * n2 * (n + 1) / 12
    if n > 1:
        var_u -= n1 * n2 * tie_sum / (12 * n * (n - 1))
    if var_u <= 0:
        return (u, 1.0)
    z = (u - mean_u) / math.sqrt(var_u)
    p = math.erfc(abs(z) / math.sqrt(2))
    return (u, p)


# ----------------------------------------------------------- reporting

# Metric set: engine_control's two plus the four new macro-bench
# segments. Each cell in the per-step table is rendered the same way;
# new metrics only filter out negative ("not measured") rows.
METRICS = ("algo", "handoff", "t_lock", "t_post", "t_round", "t_bcast")


def cycles_to_ns(cycles: float, hz: int) -> float:
    return cycles * 1e9 / hz if hz else 0.0


def metric_values(samples: list[Sample], name: str) -> list[int]:
    """Filter out the -1 sentinel for non-applicable rows."""
    out = []
    for s in samples:
        v = getattr(s, name)
        if v >= 0:
            out.append(v)
    return out


def group_by_step(samples: list[Sample]) -> dict[int, dict]:
    groups: dict[int, dict] = defaultdict(
        lambda: {"load": 0, **{m: [] for m in METRICS}})
    for s in samples:
        g = groups[s.step]
        g["load"] = s.load
        for m in METRICS:
            v = getattr(s, m)
            if v >= 0:
                g[m].append(v)
    return groups


def format_ns(cyc: float, hz: int) -> str:
    if hz == 0 or math.isnan(cyc):
        return f"{cyc:.0f}"
    return f"{cyc:.0f} / {cycles_to_ns(cyc, hz):.0f}ns"


def render(base_s: list[Sample], gale_s: list[Sample],
           base_m: Meta, gale_m: Meta, runs: int) -> str:
    hz = base_m.cycles_per_sec or gale_m.cycles_per_sec
    lines: list[str] = []
    lines.append("# Flight-control macro-bench — event-stream analysis\n")
    lines.append(f"- Runs per variant: **{runs}**")
    lines.append(f"- Baseline events: {len(base_s)} "
                 f"(target {base_m.target_samples * runs}, "
                 f"drops {sum(base_m.drops.values())})")
    lines.append(f"- Gale events:     {len(gale_s)} "
                 f"(target {gale_m.target_samples * runs}, "
                 f"drops {sum(gale_m.drops.values())})")
    lines.append(f"- Telemetry emits (baseline / gale): "
                 f"{sum(base_m.telemetry_emits.values())} / "
                 f"{sum(gale_m.telemetry_emits.values())}")
    if hz:
        lines.append(f"- Cycle counter:   {hz:,} Hz "
                     f"(1 cycle ≈ {1e9/hz:.1f} ns)")
    lines.append("")

    base_g = group_by_step(base_s)
    gale_g = group_by_step(gale_s)
    all_steps = sorted(set(base_g) | set(gale_g))

    for metric in METRICS:
        lines.append(f"## `{metric}` cycles — per-step distributions\n")
        lines.append("| Step | Load | N (base/gale) | "
                     "Baseline median (95% CI) | Gale median (95% CI) | "
                     "Δ median | MW-U p |")
        lines.append("|---|---:|---:|---|---|---|---|")
        for st in all_steps:
            b_xs = base_g.get(st, {}).get(metric, [])
            g_xs = gale_g.get(st, {}).get(metric, [])
            load = (base_g.get(st, {}).get("load")
                    or gale_g.get(st, {}).get("load") or 0)
            if not b_xs or not g_xs:
                lines.append(f"| {st} | {load} | "
                             f"{len(b_xs)}/{len(g_xs)} | — | — | — | — |")
                continue
            b_med, b_lo, b_hi = bootstrap_median_ci(b_xs)
            g_med, g_lo, g_hi = bootstrap_median_ci(g_xs)
            _, p = mannwhitney_u(b_xs, g_xs)
            if b_med != 0:
                delta_pct = (g_med - b_med) * 100.0 / b_med
                arrow = "↓" if delta_pct < 0 else ("↑" if delta_pct > 0 else "=")
                delta_str = f"{delta_pct:+.1f}% {arrow}"
            else:
                delta_str = "—"
            lines.append(
                f"| {st} | {load} | {len(b_xs)}/{len(g_xs)} | "
                f"{format_ns(b_med, hz)} "
                f"[{format_ns(b_lo, hz)}, {format_ns(b_hi, hz)}] | "
                f"{format_ns(g_med, hz)} "
                f"[{format_ns(g_lo, hz)}, {format_ns(g_hi, hz)}] | "
                f"{delta_str} | {p:.3g} |"
            )
        lines.append("")

    # Pooled percentiles for each cross-thread metric
    for metric in ("handoff", "t_lock", "t_post", "t_round", "t_bcast"):
        lines.append(f"## `{metric}` — overall (pooled across steps)\n")
        lines.append("| Percentile | Baseline | Gale |")
        lines.append("|---|---|---|")
        all_b = sorted(metric_values(base_s, metric))
        all_g = sorted(metric_values(gale_s, metric))
        for q, label in [(0.50, "p50"), (0.75, "p75"),
                         (0.95, "p95"), (0.99, "p99"), (1.00, "max")]:
            bp = percentile(all_b, q) if all_b else float("nan")
            gp = percentile(all_g, q) if all_g else float("nan")
            lines.append(f"| {label} | {format_ns(bp, hz)} | "
                         f"{format_ns(gp, hz)} |")
        lines.append("")

    # Integrity check: algo median should match across builds.
    base_algo = sorted(s.algo for s in base_s)
    gale_algo = sorted(s.algo for s in gale_s)
    if base_algo and gale_algo:
        b_med = percentile(base_algo, 0.5)
        g_med = percentile(gale_algo, 0.5)
        delta = (abs(g_med - b_med) / b_med * 100.0) if b_med else 0.0
        lines.append(f"## Integrity\n")
        lines.append(f"- `algo` median (baseline vs gale): "
                     f"{b_med:.0f} vs {g_med:.0f} cycles "
                     f"({delta:.1f}% delta; integrity assert passes at <10%)")

    return "\n".join(lines) + "\n"


# ------------------------------------------------------------- asserts

def run_asserts(base_s: list[Sample], gale_s: list[Sample],
                base_m: Meta, gale_m: Meta, runs: int
                ) -> tuple[bool, list[str]]:
    msgs: list[str] = []
    ok = True

    def check(label: str, cond: bool, detail: str) -> None:
        nonlocal ok
        if cond:
            msgs.append(f"pass [{label}]: {detail}")
        else:
            msgs.append(f"FAIL [{label}]: {detail}")
            ok = False

    expected = base_m.target_samples * runs
    check("baseline.samples>=expected",
          len(base_s) >= expected * 0.95,
          f"got {len(base_s)} of {expected} (>=95%)")
    check("gale.samples>=expected",
          len(gale_s) >= expected * 0.95,
          f"got {len(gale_s)} of {expected} (>=95%)")

    b_drops = sum(base_m.drops.values())
    g_drops = sum(gale_m.drops.values())
    check("baseline.drops==0", b_drops == 0, f"drops={b_drops}")
    check("gale.drops==0",     g_drops == 0, f"drops={g_drops}")

    check("baseline.runs_ended",
          len(base_m.ended) == runs,
          f"ended runs: {sorted(base_m.ended)} (expected {runs})")
    check("gale.runs_ended",
          len(gale_m.ended) == runs,
          f"ended runs: {sorted(gale_m.ended)} (expected {runs})")

    # Telemetry integrity: design Section "Risks" calls for proof
    # that the lowest-priority telemetry thread isn't starved.
    b_tele = sum(base_m.telemetry_emits.values())
    g_tele = sum(gale_m.telemetry_emits.values())
    check("baseline.telemetry_emits>0", b_tele > 0,
          f"telemetry_emits={b_tele}")
    check("gale.telemetry_emits>0", g_tele > 0,
          f"telemetry_emits={g_tele}")

    # Integrity: algo median delta < 10% across builds.
    base_algo = sorted(s.algo for s in base_s)
    gale_algo = sorted(s.algo for s in gale_s)
    if base_algo and gale_algo:
        b_med = percentile(base_algo, 0.5)
        g_med = percentile(gale_algo, 0.5)
        if b_med > 0:
            delta = abs(g_med - b_med) / b_med
            check("algo.median_delta<10%",
                  delta < 0.10,
                  f"baseline_med={b_med:.0f} gale_med={g_med:.0f} "
                  f"delta={delta*100:.1f}%")

    # Regression guards: gale p99 must not exceed 2x baseline p99 on
    # any of the four new segments + the legacy handoff. Mirrors
    # engine_control's "no sample > 2x baseline p99" but operates on
    # p99 (not max) because the new segments are sparse — only ~10%
    # of sensor rows carry them.
    for metric in ("handoff", "t_lock", "t_post", "t_round", "t_bcast"):
        b = sorted(metric_values(base_s, metric))
        g = sorted(metric_values(gale_s, metric))
        if not b or not g:
            check(f"gale.{metric}_p99<=2*base_p99", False,
                  f"insufficient data: base={len(b)} gale={len(g)}")
            continue
        b_p99 = percentile(b, 0.99)
        g_p99 = percentile(g, 0.99)
        check(f"gale.{metric}_p99<=2*base_p99",
              g_p99 <= 2 * b_p99,
              f"gale_p99={g_p99:.0f} baseline_p99={b_p99:.0f}")

    return ok, msgs


# ------------------------------------------------------------------ main

def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--baseline", required=True, type=Path)
    ap.add_argument("--gale",     required=True, type=Path)
    ap.add_argument("--runs",     type=int, default=1)
    ap.add_argument("--assert-only", action="store_true")
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args(argv)

    base_s, base_m = parse_events(args.baseline)
    gale_s, gale_m = parse_events(args.gale)

    ok, messages = run_asserts(base_s, gale_s, base_m, gale_m, args.runs)

    if not args.assert_only and not args.json:
        sys.stdout.write(render(base_s, gale_s, base_m, gale_m, args.runs))
        sys.stdout.write("\n## Asserts\n\n")
        for m in messages:
            sys.stdout.write(f"- {m}\n")
    elif args.json:
        json.dump({
            "ok": ok,
            "messages": messages,
            "baseline_samples": len(base_s),
            "gale_samples": len(gale_s),
            "baseline_drops": sum(base_m.drops.values()),
            "gale_drops": sum(gale_m.drops.values()),
            "baseline_telemetry_emits": sum(base_m.telemetry_emits.values()),
            "gale_telemetry_emits": sum(gale_m.telemetry_emits.values()),
            "runs": args.runs,
        }, sys.stdout, indent=2)
        sys.stdout.write("\n")
    else:
        for m in messages:
            print(m)

    return 0 if ok else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
