#!/usr/bin/env python3
"""Extract board-level pin facts from the pinned Zephyr fork's board devicetrees.

Writes targets/reference/<soc>.board.tsv: user LEDs and buttons (port, pin, active
level) and the console UART with its TX/RX pins. Facts only -- the devicetree files
are not vendored. Reference data: targets/*.aadl stays authoritative and
gust-target-gen --board-reference cross-checks it.

usage: import_boards.py <dir with <board>.dts>   (run with python3 -I)
"""
import hashlib, pathlib, re, sys

REPO = "pulseengine/zephyr"
COMMIT = "9550d256a582f4a19bc5bee650310b29a606c64b"  # .github/zephyr-fork-pin
BOARDS = {  # gale target model -> board directory (each file's sha256 is recorded in its output)
    "stm32f100": "stm32vl_disco",
    "stm32g031": "nucleo_g031k8",
    "stm32g474": "nucleo_g474re",
    "stm32wb55": "nucleo_wb55rg",
    "stm32wl55": "nucleo_wl55jc",
}

def block(text: str, start: int) -> str:
    depth, i = 0, text.index("{", start)
    for j in range(i, len(text)):
        depth += {"{": 1, "}": -1}.get(text[j], 0)
        if depth == 0:
            return text[i : j + 1]
    raise ValueError("unbalanced braces")

def children(text: str, compatible: str):
    m = re.search(r'compatible = "' + re.escape(compatible) + r'";', text)
    if not m:
        return []
    parent = block(text, text.rfind("{", 0, m.start()))
    out = []
    for c in re.finditer(r"(\w[\w-]*)\s*:\s*([\w-]+)\s*\{([^{}]*)\}", parent):
        g = re.search(r"gpios = <&gpio([a-z]) (\d+) \(?([^>]*?)\)?>;", c.group(3))
        if g:
            out.append((c.group(2), g.group(1).upper(), int(g.group(2)), "ACTIVE_LOW" in g.group(3)))
    return out

def console(text: str):
    uart = re.search(r"zephyr,console = &(\w+);", text).group(1)
    node = text[text.index(f"&{uart} {{") :]
    pins = re.search(r"pinctrl-0 = <([^>]*)>", block(node, 0)).group(1)
    tx = re.search(r"_tx_p([a-z])(\d+)", pins)
    rx = re.search(r"_rx_p([a-z])(\d+)", pins)
    return uart.upper(), tx.group(1).upper(), int(tx.group(2)), rx.group(1).upper(), int(rx.group(2))

def main(src: str) -> int:
    out = pathlib.Path(__file__).resolve().parent
    for soc, board in BOARDS.items():
        raw = (pathlib.Path(src) / f"{board}.dts").read_bytes()
        sha = hashlib.sha256(raw).hexdigest()
        text = raw.decode()
        rows = [f"led\t{n}\t{p}\t{pin}\t{'low' if al else 'high'}" for n, p, pin, al in children(text, "gpio-leds")]
        rows += [f"button\t{n}\t{p}\t{pin}\t{'low' if al else 'high'}" for n, p, pin, al in children(text, "gpio-keys")]
        u, tp, tn, rp, rn = console(text)
        rows.append(f"console\t{u}\t{tp}{tn}\t{rp}{rn}\t-")
        head = [
            f"# {board} board pins, from {REPO}@{COMMIT} boards/st/{board}/{board}.dts (sha256 {sha}).",
            "# Facts only (the devicetree is not vendored). Reference: targets/*.aadl is authoritative;",
            "# gust-target-gen --board-reference cross-checks it. Regenerate with import_boards.py.",
            "# kind\tname\tport|tx\tpin|rx\tactive",
        ]
        (out / f"{soc}.board.tsv").write_text("\n".join(head + rows) + "\n")
        print(f"{soc} ({board}):", "; ".join(rows))
    return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1]))
