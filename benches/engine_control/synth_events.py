"""Synthesize a realistic event stream for spar_oracle smoke testing.

Emits /tmp/gale-oracle-smoke.csv — a 150-event stream in the same
format as the real bench's output.csv, with handoff cycle counts
well inside the AADL-declared Give_Decide WCET bound.

Optional --violation flag injects a few out-of-range samples so the
oracle's failure path can be exercised in tests.
"""
import argparse
import random


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--violation", action="store_true",
                    help="Inject out-of-WCET samples (for negative tests)")
    ap.add_argument("--out", default="/tmp/gale-oracle-smoke.csv")
    args = ap.parse_args()

    r = random.Random(7)
    lines = [
        "# synthetic event stream for spar_oracle smoke test",
        "M,R1,gale,build,qemu_cortex_m3",
        "M,R1,gale,cycles_per_sec,12000000",
        "M,R1,gale,target_samples,150",
    ]
    seq = 0
    for step_idx, rpm in enumerate([1000, 3000, 5000, 7000, 9000]):
        for i in range(30):
            seq += 1
            algo = r.randint(80, 120)
            if args.violation and step_idx == 2 and i == 5:
                # 200 cycles @ 12 MHz = ~16700 ns — above the 6500 ns
                # WCET. Also above 2x WCET (13000 ns) so the Saturated
                # heuristic should catch it.
                handoff = 200
            else:
                handoff = r.randint(12, 60)
            lines.append(
                f"R1,gale,E,{seq},{step_idx},{rpm},{algo},{handoff}")
    lines += [
        "M,R1,gale,samples,150",
        "M,R1,gale,drops,0",
        "M,R1,gale,END",
    ]
    with open(args.out, "w") as f:
        f.write("\n".join(lines) + "\n")
    print(f"wrote {args.out}, total events: {seq}")


if __name__ == "__main__":
    main()
