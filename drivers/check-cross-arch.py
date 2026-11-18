#!/usr/bin/env python3
"""Cross-architecture seam gate for the thin-seam drivers (REQ-DRV-XARCH-001).

Supersedes the verdict half of `build-cross-arch.sh`, which was wrong in two
ways that cancelled each other out into a green light:

  1. It enumerated EIGHT drivers by hand while more `*-thin` directories
     existed, so can/hm/mpu/spi/switch were never gated at all.
  2. Its rule was `undefined_count > 0`. That is not the property. A driver
     crosses only if the lowered object's undefined set is EXACTLY the set of
     names the wasm imports -- no more, no less. `> 0` is satisfied by a
     LEAKED symbol just as well as by a seam symbol.

A lowered object passes a leg only if ALL of these hold:
  seam      its undefined set equals the wasm's import names (an empty set is
            correct for a driver importing nothing, e.g. `hm-thin`)
  exports   every exported wasm function is defined in the object under at
            least one of its export names (see exports_ok() on aliases)
  code      it has a non-empty executable section
  RV32 only no A-extension instruction (esp32c3 is RV32IMC)
The seam rule alone cannot fail an import-free driver whose object is empty or
missing: the empty set equals the empty set. The exports and code rules can.

Legs (see CROSS-ARCH.md for the policy of each):
  ARM cortex-m3      GATED. Every driver, no ledger.
  RISC-V esp32c3     GATED, with a capped, expiring known-defect ledger.
  ARMv6-M            ADVISORY. synth has no ARMv6-M target; the leg is reported
                     on every run and never changes the exit code.

  python3 drivers/check-cross-arch.py
  RV_CC=<clang with the RISC-V backend> python3 drivers/check-cross-arch.py --self-test
  python3 drivers/check-cross-arch.py --armv6m-advisory

Exit codes:
  0  gate held
  1  self-test failed: a planted bad input was not caught by the gate code
  2  usage / tooling error (wasm-tools, synth, nm, assembler missing; no drivers)
  3  a driver's wasm failed to build (cargo exit != 0, or cargo reported no wasm)
  4  a driver's ARM object fails the leg
  5  the RISC-V leg failed: a held driver does not cross, or the ledger broke a rule
  6  an RV32 object contains an A-extension (atomic) instruction

Every intermediate file (wasm copy, ARM and RV32 objects, self-test fixtures)
lives in a per-run tempfile.TemporaryDirectory, so parallel runs cannot collide
and a previous run's object can never be read as this run's.
"""

import argparse, datetime, json, os, pathlib, re, shutil, struct, subprocess, sys, tempfile

HERE = pathlib.Path(__file__).resolve().parent

EXIT_OK, EXIT_SELFTEST, EXIT_TOOL, EXIT_BUILD, EXIT_ARM, EXIT_RV, EXIT_AEXT = 0, 1, 2, 3, 4, 5, 6

# ---------------------------------------------------------------------------
# RISC-V known-defect ledger.
#
# History (synth 0.52 .. 0.89): the RV32 leg started at 0 of 13 crossing (a
# dangling `synth_func_N` at exit 0, synth#1102; then a loud refusal from 0.61.0
# on, and the 0x100000+k immediate fold, synth#1254). gale#516 removed
# wit-bindgen's run_ctors_once guard -- the only access at 0x100000+k -- and 11
# of 13 crossed on synth 0.89.0. CROSS-ARCH.md keeps the full record.
#
# What the rules below DO: they make every exception visible and dated, so a
# reviewer sees it. What they do NOT do: stop a bad ledger edit. Swapping one
# entry for another at the same cap, with a made-up tracker such as synth#0,
# passes every rule. Review of this dict is the control for that.
#
# Rules (ledger_findings(); each clause has a planted violation in --self-test):
#   R1 stale       a ledgered driver that now crosses must leave the ledger
#   R2 regression  a driver NOT on the ledger that does not cross fails the leg
#   R3 entry       each entry names an upstream tracker (synth#N), an owner
#                  issue here (gale#N) and an ISO expiry, and names a driver
#                  that exists
#   R4 expiry      an entry past its expiry fails (expiry day itself passes).
#                  Expiry may be at most LEDGER_MAX_HORIZON_DAYS ahead. Within
#                  LEDGER_WARN_DAYS of expiry the gate emits a ::warning::
#   R5 cap         len(ledger) must EQUAL RISCV_LEDGER_CAP, so adding or
#                  removing an entry shows up as a second edit (the cap) in the
#                  same diff
#   R6 majority    the ledger must leave a strict majority of subjects held
#                  (2 * ledgered < subjects). Past half, the leg is a list of
#                  exceptions rather than a gate; the response is to demote it
#                  to advisory in review, not to grow the list
RISCV_LEDGER = {
    # Measured on synth 0.89.0 (layer covalent 2026.11.1), 2026-11-18.
    "mpu-thin": {
        "blocker": "3 functions: RV32 selector folds 0x100000+k into the 12-bit offset "
                   "(synth#1254). Also declines 3 functions on I32Popcnt and 1 on "
                   "MemoryFill, which have NO upstream tracker yet. 2 of 8 exports skipped",
        "tracker": "synth#1254",
        "owner": "gale#506",
        "expires": "2027-02-28",
    },
    "switch-thin": {
        "blocker": "9 functions: RV32 selector folds 0x100000+k into the 12-bit offset; "
                   "1 more on I64Load32U; 2 of 11 exports skipped",
        "tracker": "synth#1254",
        "owner": "gale#506",
        "expires": "2027-02-28",
    },
    "i2c-v2-thin": {
        "blocker": "1 function: a call inside a block with an operand beneath its "
                   "arguments (Call(0)); a retained export relocates against it (#1102 refusal)",
        "tracker": "synth#1724",
        "owner": "gale#506",
        "expires": "2027-02-28",
    },
}
RISCV_LEDGER_CAP = 3
LEDGER_MAX_HORIZON_DAYS = 120
LEDGER_WARN_DAYS = 30

# Drivers whose emitted ARM code READS the globals table, so the
# --embedder-global-init declaration would be a real promise rather than a
# vacuous one. These must go through --native-pointer-abi instead;
# build-iso-core.sh does that, with meld --pack-rebase ahead of it.
NEEDS_NATIVE_PTR = {"mpu-thin", "switch-thin"}

# ARMv6-M (Cortex-M0/M0+) is ADVISORY. synth's `--target` list has no ARMv6-M
# profile, so there is nothing to lower to. The leg is printed on every gate run
# and as a `::warning::` annotation from its own continue-on-error CI step; it
# never changes this script's exit code and is not part of any required context.
ARMV6M_TARGET_RE = re.compile(r"cortex-m0(?:\+|plus)?\b|cortex-m1\b|armv6-?m|thumbv6m", re.I)

# RV32 A extension: major opcode AMO, funct3 010 (.W), and one of these funct5.
A_FUNCT5 = {0b00010: "lr.w", 0b00011: "sc.w", 0b00001: "amoswap.w", 0b00000: "amoadd.w",
            0b00100: "amoxor.w", 0b01100: "amoand.w", 0b01000: "amoor.w", 0b10000: "amomin.w",
            0b10100: "amomax.w", 0b11000: "amominu.w", 0b11100: "amomaxu.w"}


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def need(tool, probe):
    try:
        ok = run(probe).returncode == 0
    except (FileNotFoundError, OSError):
        ok = False
    if not ok:
        print(f"FATAL: {tool} not usable (exit 2)", file=sys.stderr)
        sys.exit(EXIT_TOOL)


def synth_bin():
    """Resolve synth through the varve pin; fall back to $SYNTH or PATH."""
    if os.environ.get("SYNTH"):
        return os.environ["SYNTH"]
    try:
        p = run(["varve", "which", "synth"])
    except (FileNotFoundError, OSError):
        return "synth"   # no varve here (CI installs a pinned synth and sets $SYNTH)
    if p.returncode == 0 and p.stdout.strip():
        cand = p.stdout.splitlines()[0].strip()
        if os.access(cand, os.X_OK):
            return cand
    return "synth"


# --------------------------------------------------------------------------- build

def build_wasm(crate_dir, out_dir, env=None):
    """Build `crate_dir` for wasm32 and copy THIS build's wasm into out_dir.

    Returns (path, None) or (None, reason). A cargo failure is a gate failure:
    there is no fallback to whatever wasm a previous build left behind. The path
    is the one cargo's own `compiler-artifact` message names for this
    invocation, not a glob of target/. Freshness is cargo's call: it only
    reports an artifact it has just built or has verified up to date.
    """
    crate_dir = pathlib.Path(crate_dir)
    p = run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown",
             "--message-format=json"], cwd=crate_dir, env=env)
    if p.returncode != 0:
        errs = [ln for ln in p.stderr.splitlines() if ln.startswith("error")]
        return None, f"cargo exit {p.returncode}: {(errs or ['(no error line)'])[0]}"
    manifest = str((crate_dir / "Cargo.toml").resolve())
    wasms = []
    for ln in p.stdout.splitlines():
        try:
            m = json.loads(ln)
        except ValueError:
            continue
        if m.get("reason") != "compiler-artifact":
            continue
        if str(pathlib.Path(m.get("manifest_path", "")).resolve()) != manifest:
            continue
        wasms += [f for f in m.get("filenames", []) if f.endswith(".wasm")]
    if len(wasms) != 1:
        return None, f"cargo reported {len(wasms)} wasm artifacts for this crate (expected 1)"
    w = pathlib.Path(wasms[0])
    if not w.is_file():
        return None, f"cargo reported {w}, which does not exist"
    dst = pathlib.Path(out_dir) / f"{crate_dir.name}.wasm"
    shutil.copyfile(w, dst)
    return dst, None


def lower(synth, wasm, out_dir, name):
    """Lower to cortex-m3 and to esp32c3 (RV32IMC). Returns (arm_o|None, rv_o|None, rv_err)."""
    out_dir = pathlib.Path(out_dir)
    arm, rv = out_dir / f"{name}.cm3.o", out_dir / f"{name}.rv32.o"
    # synth refuses the plain relocatable path unless the caller accepts the
    # data-segment (#1041) and global-initializer (#1052) obligations. We accept
    # them and then CHECK them -- see globals_or_data_refs() and NEEDS_NATIVE_PTR.
    pa = run([synth, "compile", str(wasm), "--target", "cortex-m3", "--all-exports",
              "--relocatable", "--embedder-data-init", "--embedder-global-init", "-o", str(arm)])
    pr = run([synth, "compile", str(wasm), "-b", "riscv", "--target", "esp32c3",
              "--all-exports", "--relocatable", "-o", str(rv)])
    rv_err = None
    if pr.returncode != 0:
        lines = [ln for ln in (pr.stderr + pr.stdout).splitlines() if "rror" in ln]
        rv_err = (lines or [f"exit {pr.returncode}"])[0].strip()
    return (arm if pa.returncode == 0 and arm.exists() else None,
            rv if pr.returncode == 0 and rv.exists() else None,
            rv_err)


# --------------------------------------------------------------------------- read

def imports(wasm):
    """The field names the wasm imports -- exactly what must stay undefined."""
    p = run(["wasm-tools", "print", str(wasm)])
    if p.returncode != 0:
        print(f"FATAL: wasm-tools print failed for {wasm} (exit 2)", file=sys.stderr)
        sys.exit(EXIT_TOOL)
    return {m.group(2) for m in re.finditer(r'\(import\s+"([^"]+)"\s+"([^"]+)"', p.stdout)}


def _leb(b, i):
    r = s = 0
    while True:
        x = b[i]; i += 1
        r |= (x & 0x7F) << s; s += 7
        if not x & 0x80:
            return r, i


def func_exports(wasm):
    """{function index: {export names}} from the wasm export section (binary parse).

    A function can be exported under several names (wit-bindgen aliases
    identical bodies, e.g. hm-thin's `fresh`/`budget-ok`/`innovation-ok`)."""
    b = pathlib.Path(wasm).read_bytes()
    if b[:4] != b"\0asm":
        raise ValueError(f"{wasm}: not a wasm module")
    out, i = {}, 8
    while i < len(b):
        sid = b[i]; size, i = _leb(b, i + 1); end = i + size
        if sid == 7:
            n, i = _leb(b, i)
            for _ in range(n):
                ln, i = _leb(b, i); name = b[i:i + ln].decode(); i += ln
                kind = b[i]; idx, i = _leb(b, i + 1)
                if kind == 0:
                    out.setdefault(idx, set()).add(name)
        i = end
    return out


def wasm_atomics(wasm):
    """Count of wasm threads-proposal atomic instructions (denominator for the A-scan)."""
    p = run(["wasm-tools", "print", str(wasm)])
    return len(re.findall(r"\batomic\.(?:fence|notify|wait\d+)\b|\.atomic\.", p.stdout))


def undefined(obj, nm):
    p = run([nm, str(obj)])
    if p.returncode != 0:
        return None
    return {ln.split()[-1] for ln in p.stdout.splitlines()
            if re.match(r"^\s*U\s", ln) or re.search(r"\sU\s", ln)}


def defined(obj, nm):
    """Global symbols the object defines (nm type letter upper-case, not U)."""
    p = run([nm, str(obj)])
    if p.returncode != 0:
        return None
    out = set()
    for ln in p.stdout.splitlines():
        f = ln.split()
        if len(f) >= 3 and len(f[1]) == 1 and f[1].isupper() and f[1] != "U":
            out.add(f[2])
    return out


def seam_ok(imp, und):
    """THE seam rule: the object's undefined set equals the wasm's import names."""
    return imp is not None and und is not None and und == imp


def exports_ok(fexp, dfn):
    """Every exported function is defined under at least one of its export names.

    Alias-tolerant on purpose: synth 0.89.0 emits ONE symbol per function, so an
    export that aliases another's body (same function index) has no symbol of
    its own. That gap is reported per driver (missing_aliases) but not gated."""
    return dfn is not None and all(names & dfn for names in fexp.values())


def missing_aliases(fexp, dfn):
    if dfn is None:
        return set()
    return {n for names in fexp.values() if names & dfn for n in names - dfn}


def elf32_exec_sections(path):
    """[(name, bytes)] for every SHF_EXECINSTR section of a little-endian ELF32."""
    data = pathlib.Path(path).read_bytes()
    if len(data) < 0x34 or data[:4] != b"\x7fELF" or data[4] != 1 or data[5] != 1:
        raise ValueError(f"{path}: not a little-endian ELF32 object")
    shoff, = struct.unpack_from("<I", data, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from("<HHH", data, 0x2E)
    hdrs = [struct.unpack_from("<IIIIIIIIII", data, shoff + i * shentsize) for i in range(shnum)]
    strtab = hdrs[shstrndx]
    def name(off):
        s = data[strtab[4] + off:]
        return s[:s.index(b"\0")].decode()
    SHT_NOBITS, SHF_EXECINSTR = 8, 0x4
    return [(name(h[0]), data[h[4]:h[4] + h[5]]) for h in hdrs
            if h[2] & SHF_EXECINSTR and h[1] != SHT_NOBITS]


def code_bytes(obj):
    """Total bytes of executable sections, or None if the object cannot be parsed."""
    try:
        return sum(len(b) for _, b in elf32_exec_sections(obj))
    except (ValueError, struct.error, IndexError, OSError):
        return None


def a_ext_scan(obj):
    """Every RV32 A-extension instruction in `obj`, decoded from the section BYTES.

    Not from a disassembler's text: objdump prints bytes under a `$d` mapping
    symbol, or an AMO when the A extension is not enabled, as `.word`, which a
    mnemonic grep cannot see. And not a linear sweep: a stray 16-bit parcel
    (`.half 0x0013`) whose low bits read as a 32-bit instruction desynchronises
    the sweep and hides the next instruction. So every 2-byte-aligned offset of
    each executable section is tested: major opcode AMO (0b0101111), funct3 010
    (.W), funct5 one of the eleven RV32A operations (LR.W also needs rs2 = 0).
    Returns [(section, offset, word)]. Over-approximation: a 32-bit window that
    straddles two real instructions could match; 0 such hits on today's 12
    RV32 objects.
    """
    hits = []
    for sec, b in elf32_exec_sections(obj):
        for i in range(0, len(b) - 3, 2):
            w, = struct.unpack_from("<I", b, i)
            if w & 0x7F != 0b0101111 or (w >> 12) & 0x7 != 0b010:
                continue
            f5 = w >> 27
            if f5 in A_FUNCT5 and not (f5 == 0b00010 and (w >> 20) & 0x1F):
                hits.append((sec, i, w))
    return hits


def leg(imp, fexp, obj, nm):
    """One leg's facts for one object."""
    und = undefined(obj, nm) if obj else None
    dfn = defined(obj, nm) if obj else None
    code = code_bytes(obj) if obj else None
    return {"und": und, "dfn": dfn, "code": code,
            "seam": seam_ok(imp, und), "exports": exports_ok(fexp, dfn), "has_code": bool(code)}


def assess(wasm, arm_obj, rv_obj, nm):
    """The per-driver verdict. main() and --self-test both go through here."""
    imp = imports(wasm)
    fexp = func_exports(wasm)
    a, r = leg(imp, fexp, arm_obj, nm), leg(imp, fexp, rv_obj, nm)
    try:
        aext = a_ext_scan(rv_obj) if rv_obj else []
    except (ValueError, struct.error, IndexError):
        aext = []
    good = seam_ok(imp, a["und"]) and a["exports"] and a["has_code"]
    rv_good = r["seam"] and r["exports"] and r["has_code"] and not aext
    return {"imp": imp, "fexp": fexp, "arm": a["und"], "rv": r["und"], "arm_leg": a, "rv_leg": r,
            "aext": aext, "good": good, "rv_good": rv_good}


# --------------------------------------------------------------------------- ledger

def ledger_findings(ledger, cap, subjects, rv_clean, today, horizon=LEDGER_MAX_HORIZON_DAYS):
    """[(rule, message)] -- empty means the RISC-V ledger holds. See R1..R6 above."""
    out = []
    subjects, rv_clean, led = set(subjects), set(rv_clean), set(ledger)
    for d in sorted(led & rv_clean):
        out.append(("R1", f"{d} now crosses RISC-V; remove it from RISCV_LEDGER and lower the cap"))
    for d in sorted(subjects - led - rv_clean):
        out.append(("R2", f"{d} is not on the ledger and does not cross RISC-V"))
    for d in sorted(led):
        e = ledger[d] or {}
        if not re.fullmatch(r"synth#\d+", str(e.get("tracker", ""))):
            out.append(("R3-tracker", f"{d}: entry names no upstream tracker (synth#N)"))
        if not re.fullmatch(r"gale#\d+", str(e.get("owner", ""))):
            out.append(("R3-owner", f"{d}: entry names no owner issue (gale#N)"))
        try:
            exp = datetime.date.fromisoformat(str(e.get("expires", "")))
        except ValueError:
            out.append(("R3-expiry", f"{d}: entry has no ISO expiry date"))
            continue
        if exp < today:
            out.append(("R4", f"{d}: ledger entry expired {exp}; re-measure and re-date it, or fix it"))
        elif (exp - today).days > horizon:
            out.append(("R4-horizon", f"{d}: expiry {exp} is more than {horizon} days ahead"))
    for d in sorted(led - subjects):
        out.append(("R3-driver", f"{d}: on the ledger but no such driver exists"))
    if len(led) != cap:
        out.append(("R5", f"ledger holds {len(led)} entries but RISCV_LEDGER_CAP is {cap}; "
                          "adding or removing an entry must also move the cap"))
    if 2 * len(led & subjects) >= len(subjects):
        out.append(("R6", f"ledger covers {len(led & subjects)} of {len(subjects)} subjects; "
                          "the leg must hold a strict majority"))
    return out


def ledger_warnings(ledger, today, days=LEDGER_WARN_DAYS):
    """Entries expiring within `days` (not yet expired): warn, do not fail."""
    out = []
    for d, e in sorted(ledger.items()):
        try:
            exp = datetime.date.fromisoformat(str((e or {}).get("expires", "")))
        except ValueError:
            continue
        if today <= exp and (exp - today).days <= days:
            out.append(f"{d}: RISC-V ledger entry expires {exp} ({(exp - today).days} days); "
                       f"re-measure on the pinned synth ({(e or {}).get('owner', '?')})")
    return out


def verdict(records, drivers, ledger, cap, today):
    """Rows -> (exit code, report lines). The ONLY place main() decides its exit code.

    records: {driver: {"build_err": str|None, "good": bool, "ptr_fail": bool,
                       "aext": [...], "rv_good": bool}}
    Priority when several legs fail: build (3), ARM (4), A-extension (6), RV (5).
    """
    lines = []
    build_fail = [d for d in drivers if records[d].get("build_err")]
    built = [d for d in drivers if d not in build_fail]
    arm_fail = [d for d in built if not records[d]["good"] or records[d].get("ptr_fail")]
    aext = {d: records[d]["aext"] for d in built if records[d]["aext"]}
    rv_clean = [d for d in built if records[d]["rv_good"]]
    rc = EXIT_OK
    if build_fail:
        lines.append("BUILD FAILED -- a driver that does not build is a gate failure, never a stale wasm:")
        lines += [f"  {d}: {records[d]['build_err']}" for d in build_fail]
        rc = rc or EXIT_BUILD
    if arm_fail:
        lines.append("ARM leg FAILED:")
        lines += [f"  {d}" for d in arm_fail]
        rc = rc or EXIT_ARM
    if aext:
        lines.append("A-EXTENSION in an RV32IMC object (esp32c3 has no A extension):")
        for d, hits in aext.items():
            lines += [f"  {d}: {sec}+{off:#x}: {w:#010x}" for sec, off, w in hits]
        rc = rc or EXIT_AEXT
    findings = [f for f in ledger_findings(ledger, cap, built, rv_clean, today)
                if not (f[0] == "R3-driver" and f[1].split(":")[0] in build_fail)]
    if findings:
        lines.append("RISC-V leg FAILED:")
        lines += [f"  [{r}] {msg}" for r, msg in findings]
        rc = rc or EXIT_RV
    else:
        lines.append("RISC-V ledger: R1-R6 hold.")
    return rc, lines


def armv6m_status(help_text):
    m = ARMV6M_TARGET_RE.search(help_text)
    return m.group(0) if m else None


def armv6m_line(synth):
    p = run([synth, "compile", "--help"])
    tgt = armv6m_status(p.stdout + p.stderr)
    if tgt:
        return tgt, (f"ARMv6-M leg: ADVISORY, but synth now offers `{tgt}` -- "
                     "promote this leg to a gated one (CROSS-ARCH.md, ARMv6-M policy)")
    return None, ("ARMv6-M leg: ADVISORY, not lowered -- synth's --target list has no "
                  "ARMv6-M profile (Cortex-M0/M0+). Not part of any required context.")


# --------------------------------------------------------------------------- self-test

ARM_SEAM = """
    .syntax unified
    .thumb
    .text
    .global f
f:
    bl read32
    bl write32
    bx lr
"""
ARM_LEAK = ARM_SEAM.replace("    bx lr", "    bl synth_func_18\n    bx lr")
ARM_NONE = """
    .syntax unified
    .thumb
    .text
    .global f
f:
    adds r0, r0, #1
    bx lr
"""
ARM_EMPTY = "    .text\n"                                   # no code, no symbols
ARM_DATA_ONLY = "    .data\n    .global f\nf:\n    .word 0\n"  # `f` defined, but no code
ARM_WRONG_EXPORT = ARM_NONE.replace("global f\nf:", "global g\ng:")
RV_SEAM = """
    .text
    .globl f
f:
    c.addi a0, 1
    call read32
    call write32
    ret
"""
RV_LEAK = RV_SEAM.replace("    ret", "    call synth_func_18\n    ret")
# The same A-extension instruction several ways: a mnemonic, a `.word` that a
# disassembler prints only as data, and one hidden behind a stray 16-bit parcel
# (`.half 0x0013` reads as the low half of a 32-bit instruction) that throws a
# linear sweep out of step.
RV_AMO_MNEMONIC = RV_SEAM.replace("    ret", "    amoadd.w a0, a1, (a2)\n    ret")
RV_AMO_WORD = RV_SEAM.replace("    ret", "    .word 0x00b6252f\n    ret")     # amoadd.w a0, a1, (a2)
RV_LR_WORD = RV_SEAM.replace("    ret", "    .word 0x100525af\n    ret")      # lr.w a1, (a0)
RV_AMO_AFTER_HALF = RV_SEAM.replace("    ret", "    .half 0x0013\n    amoadd.w a0, a1, (a2)\n    ret")
PLANTED_BAD_REFS = """
    .syntax unified
    .thumb
    .text
    .global bad
bad:
    movw ip, #12
    movt ip, #16
    ldrb.w r1, [fp, ip]
    mov r0, r9
    bx lr
"""
WAT_SEAM = """(module
  (import "gust:hal/mmio@0.1.0" "read32" (func (param i32) (result i32)))
  (import "gust:hal/mmio@0.1.0" "write32" (func (param i32 i32)))
  (func (export "f") (param i32) (result i32) local.get 0))"""
WAT_NONE = """(module (func (export "f") (param i32) (result i32) local.get 0))"""
FIXTURE_TOML = """[workspace]
[package]
name = "xarch-fixture"
version = "0.1.0"
edition = "2021"
[lib]
crate-type = ["cdylib"]
[profile.release]
panic = "abort"
"""
FIXTURE_OK = "#![no_std]\n#[panic_handler]\nfn p(_: &core::panic::PanicInfo) -> ! { loop {} }\n" \
             "#[no_mangle] pub extern \"C\" fn f(x: i32) -> i32 { x.wrapping_add(1) }\n"
FIXTURE_BROKEN = FIXTURE_OK.replace("x.wrapping_add(1) }", "x.wrapping_add(1) ")
# Pinned so the self-test does not change verdict with the calendar.
SELFTEST_TODAY = datetime.date(2026, 11, 18)


class Planter:
    def __init__(self, d):
        self.d = pathlib.Path(d)
        self.arm_as = os.environ.get("AS", "arm-none-eabi-as")
        self.rv_cc = os.environ.get("RV_CC", "clang")

    def arm(self, src, tag):
        s, o = self.d / f"{tag}.s", self.d / f"{tag}.o"
        s.write_text(src)
        if run([self.arm_as, "-mcpu=cortex-m3", "-mthumb", "-o", str(o), str(s)]).returncode:
            print(f"FATAL: {self.arm_as} could not assemble planted {tag} (exit 2)", file=sys.stderr)
            sys.exit(EXIT_TOOL)
        return o

    def rv(self, src, tag):
        s, o = self.d / f"{tag}.s", self.d / f"{tag}.o"
        s.write_text(src)
        p = run([self.rv_cc, "--target=riscv32-unknown-elf", "-march=rv32imac", "-c", "-o", str(o), str(s)])
        if p.returncode:
            print(f"FATAL: {self.rv_cc} could not assemble planted RV32 {tag} (set RV_CC to a "
                  f"clang with the RISC-V backend) (exit 2)\n{p.stderr}", file=sys.stderr)
            sys.exit(EXIT_TOOL)
        return o

    def wasm(self, wat, tag):
        s, o = self.d / f"{tag}.wat", self.d / f"{tag}.wasm"
        s.write_text(wat)
        if run(["wasm-tools", "parse", str(s), "-o", str(o)]).returncode:
            print(f"FATAL: wasm-tools could not parse planted {tag} (exit 2)", file=sys.stderr)
            sys.exit(EXIT_TOOL)
        return o

    def junk(self, tag):
        o = self.d / f"{tag}.o"
        o.write_bytes(b"\x7fELF this is not an object" + bytes(64))
        return o


def self_test():
    """Every control runs the REAL gate code on a planted input and must fire."""
    nm = os.environ.get("NM", "arm-none-eabi-nm")
    asm = os.environ.get("AS", "arm-none-eabi-as")
    for tool, probe in ((asm, [asm, "--version"]), (nm, [nm, "--version"]),
                        ("arm-none-eabi-objdump", ["arm-none-eabi-objdump", "--version"]),
                        ("wasm-tools", ["wasm-tools", "--version"]),
                        ("cargo", ["cargo", "--version"]),
                        (os.environ.get("RV_CC", "clang"), [os.environ.get("RV_CC", "clang"), "--version"])):
        need(tool, probe)
    fails = []

    def check(label, cond):
        print(f"  self-test {'ok  ' if cond else 'FAIL'}: {label}")
        if not cond:
            fails.append(label)

    with tempfile.TemporaryDirectory(prefix="xarch-selftest-") as td:
        P = Planter(td)
        w_seam, w_none = P.wasm(WAT_SEAM, "seam"), P.wasm(WAT_NONE, "none")
        a_seam, a_leak, a_none = P.arm(ARM_SEAM, "a_seam"), P.arm(ARM_LEAK, "a_leak"), P.arm(ARM_NONE, "a_none")
        a_empty, a_data = P.arm(ARM_EMPTY, "a_empty"), P.arm(ARM_DATA_ONLY, "a_data")
        a_wrong, junk = P.arm(ARM_WRONG_EXPORT, "a_wrong"), P.junk("junk")
        r_seam, r_leak = P.rv(RV_SEAM, "r_seam"), P.rv(RV_LEAK, "r_leak")
        r_amo, r_word, r_lr = P.rv(RV_AMO_MNEMONIC, "r_amo"), P.rv(RV_AMO_WORD, "r_word"), P.rv(RV_LR_WORD, "r_lr")
        r_half = P.rv(RV_AMO_AFTER_HALF, "r_half")
        r_empty, r_data = P.rv("    .text\n", "r_empty"), P.rv("    .data\n    .globl f\nf:\n    .word 0\n", "r_data")

        # --- seam / exports / code, through imports() undefined() defined() via assess()
        v = assess(w_seam, a_seam, r_seam, nm)
        check("planted seam-exact objects pass on ARM and RV32", v["good"] and v["rv_good"])
        check("imports() reads exactly {read32, write32} from the planted wasm",
              v["imp"] == {"read32", "write32"})
        check("func_exports() reads the planted export `f`", list(v["fexp"].values()) == [{"f"}])
        v = assess(w_seam, a_leak, r_leak, nm)
        check("a leaked synth_func_18 (ARM) is rejected", not v["good"])
        check("a leaked synth_func_18 (RV32) is rejected", not v["rv_good"])
        check("undefined() reports the leaked symbol itself", v["arm"] is not None and "synth_func_18" in v["arm"])
        v = assess(w_none, a_none, None, nm)
        check("a driver importing nothing passes with 0 undefined (the hm-thin case)", v["good"])
        v = assess(w_seam, a_none, None, nm)
        check("a truncated object (seam vanished, 0 undefined) is rejected", not v["good"])
        v = assess(w_seam, None, None, nm)
        check("no object at all is rejected", not v["good"] and not v["rv_good"])
        # import-free driver: the seam rule alone cannot fail these
        v = assess(w_none, None, None, nm)
        check("imports nothing + no object -> rejected on both legs", not v["good"] and not v["rv_good"])
        v = assess(w_none, junk, junk, nm)
        check("imports nothing + corrupt object -> rejected on both legs", not v["good"] and not v["rv_good"])
        v = assess(w_none, a_empty, None, nm)
        check("imports nothing + empty object -> rejected", not v["good"])
        v = assess(w_none, a_data, None, nm)
        check("imports nothing + export defined but no code -> rejected", not v["good"])
        v = assess(w_none, a_wrong, None, nm)
        check("imports nothing + code present but export missing -> rejected", not v["good"])
        v = assess(w_none, a_none, r_empty, nm)
        check("imports nothing + empty RV32 object -> RV32 leg rejected", not v["rv_good"])
        v = assess(w_none, a_none, r_data, nm)
        check("imports nothing + RV32 export defined but no code -> RV32 leg rejected", not v["rv_good"])
        check("seam_ok(): a missing object is not the empty set", seam_ok(set(), None) is False)
        check("undefined(): nm failing on a corrupt object yields None, not {}", undefined(junk, nm) is None)
        check("defined(): nm failing on a corrupt object yields None, not {}", defined(junk, nm) is None)
        check("exports_ok(): aliases need only one defined name per function",
              exports_ok({0: {"a", "b"}, 1: {"c"}}, {"a", "c"}) and
              not exports_ok({0: {"a", "b"}, 1: {"c"}}, {"a"}))

        # --- A-extension scan, from the bytes, on planted RV32 objects
        check("A-scan: clean RV32 object has 0 A-extension instructions", a_ext_scan(r_seam) == [])
        for o, label in ((r_amo, "amoadd.w as a mnemonic"), (r_word, "amoadd.w as .word"),
                         (r_lr, "lr.w as .word"), (r_half, "amoadd.w after `.half 0x0013`")):
            v = assess(w_seam, a_seam, o, nm)
            check(f"A-scan fires on {label}, and fails the RV32 leg", len(v["aext"]) == 1 and not v["rv_good"])

        # --- ARM linear-memory / globals check (gale#516)
        r9, mem = globals_or_data_refs(P.arm(PLANTED_BAD_REFS, "bad_refs"))
        check(f"planted [fp, ip] load + r9 read is flagged (r9 x{r9}, [fp, ...] x{mem})", bool(r9 and mem))
        r9, mem = globals_or_data_refs(a_none)
        check("planted sp/reg-only object is not flagged", r9 == 0 and mem == 0)

        # --- build: a cargo failure is a gate failure and never yields the old wasm
        crate = pathlib.Path(td) / "fixture"
        (crate / "src").mkdir(parents=True)
        (crate / "Cargo.toml").write_text(FIXTURE_TOML)
        (crate / "src/lib.rs").write_text(FIXTURE_OK)
        env = dict(os.environ, CARGO_TARGET_DIR=str(pathlib.Path(td) / "target"))
        outd = pathlib.Path(td) / "out"
        outd.mkdir()
        w, err = build_wasm(crate, outd, env=env)
        check(f"fixture crate builds to a wasm ({err or 'ok'})", w is not None and w.is_file())
        (crate / "src/.DS_Store").write_bytes(b"x")
        os.utime(crate / "Cargo.toml")
        w1, err1 = build_wasm(crate, outd, env=env)
        check(f"a touched Cargo.toml and a stray src/.DS_Store do not fail the build ({err1 or 'ok'})",
              w1 is not None)
        (crate / "src/bin").mkdir()
        (crate / "src/bin/tool.rs").write_text("fn main( {\n")
        w3, err3 = build_wasm(crate, outd, env=env)
        check(f"lib builds but another target in the crate fails -> build failure ({err3})",
              w3 is None and bool(err3))
        (crate / "src/bin/tool.rs").unlink()
        (crate / "src/lib.rs").write_text(FIXTURE_BROKEN)
        w2, err2 = build_wasm(crate, outd, env=env)
        check(f"syntax error -> build failure, no stale wasm returned ({err2})", w2 is None and bool(err2))

        # --- verdict(): rows -> exit code (what main() returns)
        ok_rec = lambda: {"build_err": None, "good": True, "ptr_fail": False, "aext": [], "rv_good": True}
        drv = [f"d{i:02}" for i in range(7)]
        led1 = {"d00": {"tracker": "synth#1", "owner": "gale#1", "expires": "2026-12-31"}}
        def recs(**over):
            r = {d: ok_rec() for d in drv}
            r["d00"]["rv_good"] = False                     # the one ledgered driver
            for d, kv in over.items():
                r[d].update(kv)
            return r
        rc = lambda r: verdict(r, drv, led1, 1, SELFTEST_TODAY)[0]
        check("verdict: all clean -> exit 0", rc(recs()) == EXIT_OK)
        check("verdict: a build failure -> exit 3", rc(recs(d01={"build_err": "cargo exit 101"})) == EXIT_BUILD)
        check("verdict: an ARM failure -> exit 4", rc(recs(d02={"good": False})) == EXIT_ARM)
        check("verdict: a native-pointer violation -> exit 4", rc(recs(d02={"ptr_fail": True})) == EXIT_ARM)
        check("verdict: an A-extension hit -> exit 6",
              rc(recs(d03={"aext": [(".text", 0, 0x00b6252f)], "rv_good": False})) == EXIT_AEXT)
        check("verdict: an RV32 regression -> exit 5", rc(recs(d04={"rv_good": False})) == EXIT_RV)
        check("verdict: build failure outranks the rest",
              rc(recs(d01={"build_err": "x"}, d02={"good": False}, d04={"rv_good": False})) == EXIT_BUILD)

        # --- ledger rules, on planted ledgers (date pinned)
        today = SELFTEST_TODAY
        subj = [f"d{i:02}" for i in range(15)]
        e = lambda **kw: dict({"blocker": "x", "tracker": "synth#1", "owner": "gale#1",
                               "expires": "2027-01-31"}, **kw)
        ok3 = {d: e() for d in subj[:3]}
        clean12 = subj[3:]
        rules = lambda f: [r for r, _ in f]
        LF = lambda led, cap=3, s=subj, c=clean12, t=today: rules(ledger_findings(led, cap, s, c, t))
        check("valid 3-of-15 ledger at cap 3 has no findings", LF(ok3) == [])
        check("R6: 15 of 15 fails even with the cap bumped", "R6" in LF({d: e() for d in subj}, 15, c=[]))
        check("R6: 14 of 15 fails even with the cap bumped", "R6" in LF({d: e() for d in subj[:14]}, 14, c=subj[14:]))
        s8 = subj[:8]
        check("R6 boundary: 4 of 8 (exactly half) fails",
              LF({d: e() for d in s8[:4]}, 4, s=s8, c=s8[4:]) == ["R6"])
        s7 = subj[:7]
        check("R6 boundary: 3 of 7 (strict majority held) passes",
              LF({d: e() for d in s7[:3]}, 3, s=s7, c=s7[3:]) == [])
        check("R5: growing the ledger without a cap bump fails",
              LF(dict(ok3, **{subj[3]: e()}), 3, c=subj[4:]) == ["R5"])
        check("R5: shrinking the ledger without lowering the cap fails",
              LF({d: e() for d in subj[:2]}, 3, c=subj[2:]) == ["R5"])
        one = lambda **kw: dict(ok3, **{subj[0]: e(**kw)})
        check("R4 boundary: expired yesterday fails", LF(one(expires="2026-11-17")) == ["R4"])
        check("R4 boundary: expiring today passes", LF(one(expires="2026-11-18")) == [])
        check(f"R4 boundary: expiry exactly {LEDGER_MAX_HORIZON_DAYS} days ahead passes",
              LF(one(expires=str(today + datetime.timedelta(days=LEDGER_MAX_HORIZON_DAYS)))) == [])
        check(f"R4 boundary: expiry {LEDGER_MAX_HORIZON_DAYS + 1} days ahead fails",
              LF(one(expires=str(today + datetime.timedelta(days=LEDGER_MAX_HORIZON_DAYS + 1)))) == ["R4-horizon"])
        no_tracker = one(); del no_tracker[subj[0]]["tracker"]
        check("R3: an entry with no tracker fails (only that clause)", LF(no_tracker) == ["R3-tracker"])
        no_owner = one(); del no_owner[subj[0]]["owner"]
        check("R3: an entry with no owner issue fails (only that clause)", LF(no_owner) == ["R3-owner"])
        no_exp = one(); del no_exp[subj[0]]["expires"]
        check("R3: an entry with no expiry fails (only that clause)", LF(no_exp) == ["R3-expiry"])
        ghost = {d: e() for d in subj[:2]}; ghost["no-such-thin"] = e()
        check("R3: an entry naming a driver that does not exist fails",
              LF(ghost, 3, c=subj[2:]) == ["R3-driver"])
        check("R1: a ledgered driver that now crosses fails", "R1" in LF(ok3, c=subj))
        check("R2: an unledgered driver that stops crossing fails", LF(ok3, c=clean12[1:]) == ["R2"])
        check("R3/R5 hold for the shipped RISCV_LEDGER (date-independent rules only)",
              not [f for f in ledger_findings(RISCV_LEDGER, RISCV_LEDGER_CAP, list(RISCV_LEDGER) + subj,
                                              subj, today) if f[0].startswith("R3") or f[0] == "R5"])
        warn = lambda exp: ledger_warnings({"d": e(expires=exp)}, today)
        check(f"warning: {LEDGER_WARN_DAYS} days before expiry warns", len(warn("2026-12-18")) == 1)
        check(f"warning: {LEDGER_WARN_DAYS + 1} days before expiry is quiet", warn("2026-12-19") == [])
        check("warning: an already expired entry is R4's job, not a warning", warn("2026-11-17") == [])

        # --- ARMv6-M advisory probe
        check("ARMv6-M probe: today's target list reads as absent",
              armv6m_status("ARM: cortex-m3, cortex-m4, cortex-m4f, cortex-m7, cortex-m7dp") is None)
        for t in ("cortex-m0plus", "cortex-m0", "thumbv6m"):
            check(f"ARMv6-M probe: a list with {t} reads as present",
                  armv6m_status(f"ARM: {t}, cortex-m3") is not None)

    print(f"\n  self-test: {len(fails)} control(s) failed" if fails else "\n  self-test: every control fired")
    return EXIT_SELFTEST if fails else EXIT_OK


def globals_or_data_refs(obj, objdump="arm-none-eabi-objdump"):
    """(r9_refs, linear_memory_refs) in the emitted ARM code.

    r9 is the globals-table base; fp (r11) is the linear-memory base, so every
    load or store into wasm linear memory is an `[fp, ...]` operand. Zero of
    both means nothing can read an unmaterialized region, so the two
    --embedder-*-init flags are checked rather than trusted (gale#516).
    """
    p = run([objdump, "-d", str(obj)])
    if p.returncode != 0:
        return None, None
    return (len(re.findall(r"\br9\b", p.stdout)),
            len(re.findall(r"\[fp, ", p.stdout)))


# --------------------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--armv6m-advisory", action="store_true",
                    help="print the ARMv6-M leg as a CI warning annotation; always exit 0")
    args = ap.parse_args()
    if args.self_test:
        return self_test()

    synth = synth_bin()
    need("synth", [synth, "--version"])
    if args.armv6m_advisory:
        _, line = armv6m_line(synth)
        print(f"::warning title=ARMv6-M (advisory)::{line}")
        return EXIT_OK

    need("wasm-tools", ["wasm-tools", "--version"])
    nm = os.environ.get("NM", "arm-none-eabi-nm")
    need(nm, [nm, "--version"])
    need("cargo", ["cargo", "--version"])
    print(f"  synth: {run([synth, '--version']).stdout.strip()}  ({synth})")

    drivers = sorted(p.name for p in HERE.glob("*-thin") if p.is_dir())
    if not drivers:
        print("FATAL: no *-thin drivers found (exit 2)", file=sys.stderr)
        return EXIT_TOOL
    print(f"  discovered {len(drivers)} thin-seam drivers\n")

    records, rows, alias_gaps, atomics, rv_objs = {}, [], {}, 0, 0
    with tempfile.TemporaryDirectory(prefix="xarch-") as td:
        for d in drivers:
            wasm, err = build_wasm(HERE / d, td)
            if wasm is None:
                records[d] = {"build_err": err, "good": False, "ptr_fail": False, "aext": [], "rv_good": False}
                rows.append((d, "BUILD FAILED", "-", "-", "-"))
                continue
            arm_o, rv_o, rv_err = lower(synth, wasm, td, d)
            v = assess(wasm, arm_o, rv_o, nm)
            ptr_fail = False
            if arm_o is not None:
                r9, dref = globals_or_data_refs(arm_o)
                if r9 is None:
                    pass                      # objdump unavailable; not a verdict
                elif d in NEEDS_NATIVE_PTR:
                    if r9 == 0:
                        print(f"  {d}: NO r9 refs, but it is on NEEDS_NATIVE_PTR -- drop it from the set")
                        ptr_fail = True
                elif r9 or dref:
                    print(f"  {d}: reads globals (r9 x{r9}) or linear memory ([fp, ...] x{dref}),")
                    print("     so --embedder-*-init is a real promise here. Move it to NEEDS_NATIVE_PTR.")
                    ptr_fail = True
            records[d] = {"build_err": None, "good": v["good"], "ptr_fail": ptr_fail,
                          "aext": v["aext"], "rv_good": v["rv_good"]}
            atomics += wasm_atomics(wasm)
            rv_objs += rv_o is not None
            for name, lg in (("ARM", v["arm_leg"]), ("RV32", v["rv_leg"])):
                gap = missing_aliases(v["fexp"], lg["dfn"])
                if gap:
                    alias_gaps.setdefault(d, {})[name] = gap

            def show(lg, extra_bad=False):
                if lg["und"] is None and lg["dfn"] is None:
                    return "declined" + (" (ledger)" if d in RISCV_LEDGER else "")
                why = [k for k in ("seam", "exports", "has_code") if not lg[k]]
                if extra_bad:
                    why.append("A-EXT")
                return "ok" if not why else "FAIL " + ",".join(why)
            rows.append((d, " ".join(sorted(v["imp"])) or "(none)", show(v["arm_leg"]),
                         show(v["rv_leg"], bool(v["aext"])), str(len(v["aext"])) if rv_o else "-"))
            if rv_err and d in RISCV_LEDGER:
                print(f"  {d}: rv32 decline: {rv_err[:150]}")

    print(f"\n{'driver':<13} {'imports':<34} {'ARM cm3':<14} {'RV32 esp32c3':<20} A-ext")
    for row in rows:
        print(f"{row[0]:<13} {row[1]:<34} {row[2]:<14} {row[3]:<20} {row[4]}")

    n = len(drivers)
    print(f"\nARM leg: {sum(1 for d in drivers if records[d]['good'] and not records[d]['ptr_fail'])} of {n} hold "
          "(seam exact, every exported function defined, non-empty code)")
    print(f"RISC-V leg: {sum(1 for d in drivers if records[d]['rv_good'])} of {n} cross; "
          f"ledger {len(RISCV_LEDGER)} (cap {RISCV_LEDGER_CAP})")
    print(f"A-extension scan: {sum(len(r['aext']) for r in records.values())} hit(s) in {rv_objs} RV32 "
          f"object(s). The wasm inputs hold {atomics} atomic op(s), so on this corpus the scan can only "
          "fire if the backend invents one; its potency is the self-test's planted objects.")
    if alias_gaps:
        print("Export aliases with no symbol of their own (reported, not gated; the function is "
              "defined under another of its names):")
        for d, legs in alias_gaps.items():
            for name, gap in legs.items():
                print(f"  {d} [{name}]: {' '.join(sorted(gap))}")
    _, a6 = armv6m_line(synth)
    print(a6)
    today = datetime.date.today()
    for w in ledger_warnings(RISCV_LEDGER, today):
        print(f"::warning title=RISC-V ledger expiry::{w}")

    rc, lines = verdict(records, drivers, RISCV_LEDGER, RISCV_LEDGER_CAP, today)
    print()
    for ln in lines:
        print(ln)
    print(f"\nexit {rc}")
    return rc


if __name__ == "__main__":
    sys.exit(main())
