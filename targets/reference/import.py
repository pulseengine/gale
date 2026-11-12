#!/usr/bin/env python3
"""Regenerate targets/reference/<soc>.peripherals.tsv from a pinned stm32-data snapshot.

Reference data only: the AADL models stay authoritative, and
`cargo xtask check target-reference` cross-checks them against these tables.
Source: embassy-rs/stm32-data-generated (derived from ST's CubeMX database and
SVD files), pinned by commit; each chip file is verified against the sha256
recorded below before use, so a changed upstream file cannot slip in.

usage: import.py <dir-with-downloaded-chip-json>   (run with python3 -I)
"""
import hashlib, json, pathlib, sys

REPO = "embassy-rs/stm32-data-generated"
COMMIT = "7e8466ba48f6ff5a0910eb08d1a7ba623d02e96d"
CHIPS = {  # gale target model -> (chip file, sha256 of that file at COMMIT)
    "stm32f100": ("STM32F100RB", "2cfca0a223cd62289392aa92a4a9a07864e091bb1c4416740b6373eb448e70dd"),
    "stm32g031": ("STM32G031K8", "99d97b1a97a9c54647c7c85b65b908121a765bb34d2b209c055b852435e51eba"),
    "stm32g474": ("STM32G474RE", "da0d1bd8af857a6186f5212769e9392ba84400c5b6533f48eb15c9971179116b"),
    "stm32wb55": ("STM32WB55RG", "e09dda2b6013a6e6983717b631727c4ba92c701ea259d28ce514919c28f99d37"),
    "stm32wl55": ("STM32WL55JC", "60958292b69eb25e3c34de762c472e836b2b68f86ba2aa9bf54c1114c2b219db"),
}

def main(src: str) -> int:
    out = pathlib.Path(__file__).resolve().parent
    for soc, (chip, want) in CHIPS.items():
        raw = (pathlib.Path(src) / f"{chip}.json").read_bytes()
        got = hashlib.sha256(raw).hexdigest()
        if got != want:
            print(f"{chip}.json: sha256 {got[:16]} != pinned {want[:16]}", file=sys.stderr)
            return 1
        core = json.loads(raw)["cores"][0]
        rows = sorted((p["name"], p["address"]) for p in core["peripherals"] if p.get("address"))
        lines = [
            f"# {chip} peripherals, from {REPO}@{COMMIT} data/chips/{chip}.json (sha256 {got}).",
            "# Reference only: targets/*.aadl is authoritative; `cargo xtask check target-reference`",
            "# cross-checks it. Regenerate with targets/reference/import.py, never by hand.",
            "# name\tbase",
        ] + [f"{n}\t0x{a:08X}" for n, a in rows]
        (out / f"{soc}.peripherals.tsv").write_text("\n".join(lines) + "\n")
        print(f"{soc}: {len(rows)} peripherals")
    return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
