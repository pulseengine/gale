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

The rule here is the same one `check-driver-components.py` uses for the object
axis: expected_undefined(wasm) == actual_undefined(object). It is correct for
drivers that import nothing (`hm-thin` -> empty set, and an empty set is a
PASS, not a failure) and for drivers whose seam is not `gust:hal`
(`mpu-thin` -> `mpu-write`, `switch-thin` -> the three ctx symbols).

Legs (see CROSS-ARCH.md for the policy of each):
  ARM cortex-m3      GATED. Every driver, no ledger.
  RISC-V esp32c3     GATED, with a capped, expiring known-defect ledger. Every
                     emitted RV32 object is also scanned, from its section
                     BYTES, for A-extension (atomic) instructions: esp32c3 is
                     RV32IMC and has no A extension.
  ARMv6-M            ADVISORY. synth has no ARMv6-M target; the leg is reported
                     on every run and never changes the exit code.

  python3 drivers/check-cross-arch.py
  python3 drivers/check-cross-arch.py --self-test
  python3 drivers/check-cross-arch.py --armv6m-advisory

Exit codes:
  0  gate held
  1  self-test failed: a planted bad input was not caught by the gate code
  2  usage / tooling error (wasm-tools, synth, nm, assembler missing; no drivers)
  3  a driver's wasm failed to build (cargo exit != 0, or no fresh wasm reported)
  4  a driver's ARM object does not match its imports
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
# Ledger rules (all enforced by ledger_findings(), all planted in --self-test):
#   R1 stale       a ledgered driver that now crosses must leave the ledger
#   R2 regression  a driver NOT on the ledger that does not cross fails the leg
#   R3 entry       every entry names the upstream blocker that would clear it
#                  (a synth#N tracker) and an ISO expiry date
#   R4 expiry      an entry past its expiry fails: it must be re-measured and
#                  re-dated, or the driver fixed. Expiry may be at most
#                  LEDGER_MAX_HORIZON_DAYS ahead, so "2099-01-01" is not a way out
#   R5 ratchet     len(ledger) must EQUAL RISCV_LEDGER_CAP. Growing the ledger
#                  needs an explicit cap bump in the same diff; shrinking it
#                  must lower the cap, so freed slack cannot be silently reused
#   R6 majority    the ledger must leave a strict majority of subjects held
#                  (2 * len(ledger) < subjects). No cap bump can get past this:
#                  past half, the leg is a list of exceptions, not a gate, and
#                  the honest move is to demote it to advisory in review.
RISCV_LEDGER = {
    # Measured on synth 0.89.0 (layer covalent 2026.11.1), 2026-11-18.
    "mpu-thin": {
        "blocker": "RV32 selector folds 0x100000+k into the 12-bit offset (func_4); "
                   "I32Popcnt and MemoryFill unsupported; 2 of 8 exports skipped",
        "tracker": "synth#1254",
        "expires": "2027-02-28",
    },
    "switch-thin": {
        "blocker": "RV32 selector folds 0x100000+k into the 12-bit offset (4 functions); "
                   "I64Load32U unsupported; 2 of 11 exports skipped",
        "tracker": "synth#1254",
        "expires": "2027-02-28",
    },
    "i2c-v2-thin": {
        "blocker": "a call inside a block with an operand beneath its arguments (Call(0))",
        "tracker": "synth#1724",
        "expires": "2027-02-28",
    },
}
RISCV_LEDGER_CAP = 3
LEDGER_MAX_HORIZON_DAYS = 120

# Drivers whose emitted ARM code READS the globals table, so the
# --embedder-global-init declaration would be a real promise rather than a
# vacuous one. These must go through --native-pointer-abi instead;
# build-iso-core.sh does that, with meld --pack-rebase ahead of it.
NEEDS_NATIVE_PTR = {"mpu-thin", "switch-thin"}

# ARMv6-M (Cortex-M0/M0+) is ADVISORY. synth's `--target` list has no ARMv6-M
# profile, so there is nothing to lower to. The leg is printed on every gate run
# and as a `::warning::` annotation from its own continue-on-error CI step; it
# never changes this script's exit code and is not part of any required context.
ARMV6M_TARGET_RE = re.compile(r"cortex-m0\+?|cortex-m0plus|cortex-m1\b|armv6-?m", re.I)


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
    is the one cargo's own `compiler-artifact` message reports for this
    invocation -- not a glob of target/ -- and it must be at least as new as
    every source file under the crate, or it is refused as stale.
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
    srcs = [crate_dir / "Cargo.toml"] + [f for f in (crate_dir / "src").rglob("*") if f.is_file()]
    newest = max(f.stat().st_mtime for f in srcs)
    if w.stat().st_mtime < newest:
        return None, f"stale wasm: {w.name} is older than the crate's newest source"
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


def seam_ok(imp, und):
    """THE rule: the object's undefined set equals the wasm's import names."""
    return imp is not None and und is not None and und == imp


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


def elf32_exec_sections(path):
    """[(name, bytes)] for every SHF_EXECINSTR section of a little-endian ELF32."""
    data = pathlib.Path(path).read_bytes()
    if data[:4] != b"\x7fELF" or data[4] != 1 or data[5] != 1:
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


def a_ext_scan(obj):
    """Every RV32 A-extension instruction in `obj`, decoded from the section BYTES.

    Not from a disassembler's text: objdump without +a prints an AMO as `.word`
    (and does so for any bytes under a `$d` mapping symbol), which a mnemonic
    grep cannot see. Linear sweep over each executable section, 16-bit parcels
    when the low two bits are not 0b11 (C extension), 32-bit otherwise. A 32-bit
    word whose major opcode is AMO (0b0101111) is an A-extension instruction
    (LR/SC/AMO*; funct3 010 = .W, 011 = .D). Returns [(section, offset, word)].
    Blind spot: a 16-bit data parcel inside .text could desynchronise the sweep;
    synth emits no data in RV32 .text.
    """
    hits = []
    for sec, b in elf32_exec_sections(obj):
        i = 0
        while i + 2 <= len(b):
            lo, = struct.unpack_from("<H", b, i)
            if lo & 0b11 != 0b11:
                i += 2
                continue
            if i + 4 > len(b):
                break
            w, = struct.unpack_from("<I", b, i)
            if w & 0x7F == 0b0101111:
                hits.append((sec, i, w))
            i += 4
    return hits


def assess(wasm, arm_obj, rv_obj, nm):
    """The per-driver verdict. main() and --self-test both go through here."""
    imp = imports(wasm)
    arm = undefined(arm_obj, nm) if arm_obj else None
    rv = undefined(rv_obj, nm) if rv_obj else None
    aext = a_ext_scan(rv_obj) if rv_obj else []
    good = seam_ok(imp, arm)
    rv_good = seam_ok(imp, rv) and not aext
    return {"imp": imp, "arm": arm, "rv": rv, "aext": aext, "good": good, "rv_good": rv_good}


# --------------------------------------------------------------------------- ledger

def ledger_findings(ledger, cap, subjects, rv_clean, today, horizon=LEDGER_MAX_HORIZON_DAYS):
    """[(rule, message)] -- empty means the RISC-V leg holds. See R1..R6 above."""
    out = []
    subjects, rv_clean, led = set(subjects), set(rv_clean), set(ledger)
    for d in sorted(led & rv_clean):
        out.append(("R1", f"{d} now crosses RISC-V; remove it from RISCV_LEDGER and lower the cap"))
    for d in sorted(subjects - led - rv_clean):
        out.append(("R2", f"{d} is not on the ledger and does not cross RISC-V"))
    for d in sorted(led):
        e = ledger[d] or {}
        if not re.fullmatch(r"synth#\d+", str(e.get("tracker", ""))):
            out.append(("R3", f"{d}: entry names no upstream tracker (synth#N)"))
        try:
            exp = datetime.date.fromisoformat(str(e.get("expires", "")))
        except ValueError:
            out.append(("R3", f"{d}: entry has no ISO expiry date")); continue
        if exp < today:
            out.append(("R4", f"{d}: ledger entry expired {exp}; re-measure and re-date it, or fix it"))
        elif (exp - today).days > horizon:
            out.append(("R4", f"{d}: expiry {exp} is more than {horizon} days ahead"))
    for d in sorted(led - subjects):
        out.append(("R3", f"{d}: on the ledger but no such driver exists"))
    if len(led) != cap:
        out.append(("R5", f"ledger holds {len(led)} entries but RISCV_LEDGER_CAP is {cap}; "
                          "growth needs an explicit cap bump, shrinkage must lower the cap"))
    if 2 * len(led & subjects) >= len(subjects):
        out.append(("R6", f"ledger covers {len(led & subjects)} of {len(subjects)} subjects; "
                          "the leg must hold a strict majority"))
    return out


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
# The same A-extension instruction three ways: a mnemonic, and a `.word` that a
# disassembler without +a (or any disassembler, under the `$d` mapping symbol the
# assembler emits for it) prints only as data.
RV_AMO_MNEMONIC = RV_SEAM.replace("    ret", "    amoadd.w a0, a1, (a2)\n    ret")
RV_AMO_WORD = RV_SEAM.replace("    ret", "    .word 0x00b6252f\n    ret")     # amoadd.w a0, a1, (a2)
RV_LR_WORD = RV_SEAM.replace("    ret", "    .word 0x100525af\n    ret")      # lr.w a1, (a0)
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
        r_seam, r_leak = P.rv(RV_SEAM, "r_seam"), P.rv(RV_LEAK, "r_leak")
        r_amo, r_word, r_lr = P.rv(RV_AMO_MNEMONIC, "r_amo"), P.rv(RV_AMO_WORD, "r_word"), P.rv(RV_LR_WORD, "r_lr")

        # Seam rule, through imports() + undefined() + seam_ok() via assess().
        v = assess(w_seam, a_seam, r_seam, nm)
        check("planted seam-exact objects pass on ARM and RV32", v["good"] and v["rv_good"])
        check("imports() reads exactly {read32, write32} from the planted wasm",
              v["imp"] == {"read32", "write32"})
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

        # A-extension scan, from the bytes, on planted RV32 objects.
        check("A-scan: clean RV32 object has 0 A-extension instructions", a_ext_scan(r_seam) == [])
        for o, label in ((r_amo, "amoadd.w as a mnemonic"), (r_word, "amoadd.w as .word"),
                         (r_lr, "lr.w as .word")):
            v = assess(w_seam, a_seam, o, nm)
            check(f"A-scan fires on {label}, and fails the RV32 leg", len(v["aext"]) == 1 and not v["rv_good"])

        # ARM linear-memory / globals check (gale#516).
        r9, mem = globals_or_data_refs(P.arm(PLANTED_BAD_REFS, "bad_refs"))
        check(f"planted [fp, ip] load + r9 read is flagged (r9 x{r9}, [fp, ...] x{mem})", bool(r9 and mem))
        r9, mem = globals_or_data_refs(a_none)
        check("planted sp/reg-only object is not flagged", r9 == 0 and mem == 0)

        # Build: a cargo failure is a gate failure and never yields the old wasm.
        crate = pathlib.Path(td) / "fixture"
        (crate / "src").mkdir(parents=True)
        (crate / "Cargo.toml").write_text(FIXTURE_TOML)
        (crate / "src/lib.rs").write_text(FIXTURE_OK)
        env = dict(os.environ, CARGO_TARGET_DIR=str(pathlib.Path(td) / "target"))
        outd = pathlib.Path(td) / "out"
        outd.mkdir()
        w, err = build_wasm(crate, outd, env=env)
        check(f"fixture crate builds to a wasm ({err or 'ok'})", w is not None and w.is_file())
        (crate / "src/lib.rs").write_text(FIXTURE_BROKEN)
        w2, err2 = build_wasm(crate, outd, env=env)
        check(f"syntax error -> build failure, no stale wasm returned ({err2})", w2 is None and err2)

        # Ledger rules, on planted ledgers (today pinned so the test is stable).
        today = datetime.date(2026, 11, 18)
        subj = [f"d{i:02}" for i in range(15)]
        e = lambda: {"blocker": "x", "tracker": "synth#1", "expires": "2027-01-31"}
        ok3 = {d: e() for d in subj[:3]}
        clean12 = subj[3:]
        check("valid 3-of-15 ledger at cap 3 has no findings",
              ledger_findings(ok3, 3, subj, clean12, today) == [])
        rules = lambda f: {r for r, _ in f}
        full = {d: e() for d in subj}
        check("R6: a ledger covering 15 of 15 fails even with the cap bumped",
              "R6" in rules(ledger_findings(full, 15, subj, [], today)))
        l14 = {d: e() for d in subj[:14]}
        check("R6: a ledger covering 14 of 15 fails even with the cap bumped",
              "R6" in rules(ledger_findings(l14, 14, subj, subj[14:], today)))
        l8 = {d: e() for d in subj[:8]}
        check("R6: 8 of 15 (no strict majority held) fails",
              "R6" in rules(ledger_findings(l8, 8, subj, subj[8:], today)))
        l4 = dict(ok3, **{subj[3]: e()})
        check("R5: growing the ledger without a cap bump fails",
              "R5" in rules(ledger_findings(l4, 3, subj, subj[4:], today)))
        l2 = {d: e() for d in subj[:2]}
        check("R5: shrinking the ledger without lowering the cap fails",
              "R5" in rules(ledger_findings(l2, 3, subj, subj[2:], today)))
        exp = dict(ok3); exp[subj[0]] = dict(e(), expires="2026-11-01")
        check("R4: an expired entry fails", "R4" in rules(ledger_findings(exp, 3, subj, clean12, today)))
        far = dict(ok3); far[subj[0]] = dict(e(), expires="2099-01-01")
        check("R4: an expiry past the horizon fails", "R4" in rules(ledger_findings(far, 3, subj, clean12, today)))
        bare = dict(ok3); bare[subj[0]] = {"blocker": "x"}
        check("R3: an entry with no tracker and no expiry fails",
              "R3" in rules(ledger_findings(bare, 3, subj, clean12, today)))
        check("R1: a ledgered driver that now crosses fails",
              "R1" in rules(ledger_findings(ok3, 3, subj, subj, today)))
        check("R2: an unledgered driver that stops crossing fails",
              "R2" in rules(ledger_findings(ok3, 3, subj, clean12[1:], today)))
        check("the shipped RISCV_LEDGER satisfies R3/R4/R5 today",
              not [f for f in ledger_findings(RISCV_LEDGER, RISCV_LEDGER_CAP, list(RISCV_LEDGER) + subj,
                                              subj, datetime.date.today()) if f[0] in ("R3", "R4", "R5")])

        # ARMv6-M advisory probe.
        check("ARMv6-M probe: today's target list reads as absent",
              armv6m_status("ARM: cortex-m3, cortex-m4, cortex-m4f, cortex-m7, cortex-m7dp") is None)
        check("ARMv6-M probe: a list with cortex-m0plus reads as present",
              armv6m_status("ARM: cortex-m0plus, cortex-m3") is not None)

    print(f"\n  self-test: {len(fails)} control(s) failed" if fails else "\n  self-test: every control fired")
    return EXIT_SELFTEST if fails else EXIT_OK


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

    build_fail, arm_fail, aext_hits, rv_clean = [], [], {}, []
    rows = []
    with tempfile.TemporaryDirectory(prefix="xarch-") as td:
        for d in drivers:
            wasm, err = build_wasm(HERE / d, td)
            if wasm is None:
                build_fail.append(d)
                rows.append((d, "BUILD FAILED", err, "", "", ""))
                continue
            arm_o, rv_o, rv_err = lower(synth, wasm, td, d)
            v = assess(wasm, arm_o, rv_o, nm)
            if not v["good"]:
                arm_fail.append(d)
            if arm_o is not None:
                r9, dref = globals_or_data_refs(arm_o)
                if r9 is None:
                    pass                      # objdump unavailable; not a verdict
                elif d in NEEDS_NATIVE_PTR:
                    if r9 == 0:
                        print(f"  {d}: NO r9 refs, but it is on NEEDS_NATIVE_PTR -- drop it from the set")
                        arm_fail.append(d)
                elif r9 or dref:
                    print(f"  {d}: reads globals (r9 x{r9}) or linear memory ([fp, ...] x{dref}),")
                    print("     so --embedder-*-init is a real promise here. Move it to NEEDS_NATIVE_PTR.")
                    arm_fail.append(d)
            if v["aext"]:
                aext_hits[d] = v["aext"]
            if v["rv_good"]:
                rv_clean.append(d)
            imp = " ".join(sorted(v["imp"])) or "(none)"
            armv = "ok" if v["good"] else ("no object" if v["arm"] is None else
                                           "MISMATCH " + " ".join(sorted(v["arm"] ^ v["imp"])))
            if v["rv_good"]:
                rvv = "ok"
            elif v["aext"] and seam_ok(v["imp"], v["rv"]):
                rvv = "A-EXTENSION"
            elif v["rv"] is None:
                rvv = "declined" + (" (ledger)" if d in RISCV_LEDGER else "")
            else:
                rvv = "MISMATCH " + " ".join(sorted(v["rv"] ^ v["imp"]))
            rows.append((d, imp, armv, rvv, str(len(v["aext"])) if rv_o else "-",
                         str(wasm_atomics(wasm))))
            if rv_err and d in RISCV_LEDGER:
                print(f"  {d}: rv32 decline: {rv_err[:150]}")

    print(f"\n{'driver':<13} {'imports':<34} {'ARM cm3':<10} {'RV32 esp32c3':<20} {'A-ext':<6} wasm-atomics")
    for d, imp, armv, rvv, ae, wa in rows:
        print(f"{d:<13} {imp:<34} {armv:<10} {rvv:<20} {ae:<6} {wa}")

    n = len(drivers)
    rv_objs = sum(1 for r in rows if r[4] not in ("-", ""))
    print(f"\nARM leg: {n - len(set(arm_fail)) - len(build_fail)} of {n} hold")
    print(f"RISC-V leg: {len(rv_clean)} of {n} cross; ledger {len(RISCV_LEDGER)} (cap {RISCV_LEDGER_CAP})")
    print(f"A-extension scan: {sum(len(h) for h in aext_hits.values())} instruction(s) in "
          f"{rv_objs} RV32 object(s) scanned. Denominator note: "
          f"{sum(int(r[5]) for r in rows if r[5])} wasm atomic op(s) across the inputs, so on this corpus "
          "the scan can only fire if the backend invents atomics; its potency is the self-test's planted objects.")
    _, a6 = armv6m_line(synth)
    print(a6)

    rc = EXIT_OK
    if build_fail:
        print("\nBUILD FAILED -- a driver that does not build is a gate failure, never a stale wasm:")
        for d, _, err, *_ in (r for r in rows if r[1] == "BUILD FAILED"):
            print(f"  {d}: {err}")
        rc = rc or EXIT_BUILD
    if arm_fail:
        print("\nARM leg FAILED:")
        for d in sorted(set(arm_fail)):
            print(f"  {d}")
        rc = rc or EXIT_ARM
    if aext_hits:
        print("\nA-EXTENSION in an RV32IMC object (esp32c3 has no A extension):")
        for d, hits in aext_hits.items():
            for sec, off, w in hits:
                print(f"  {d}: {sec}+{off:#x}: {w:#010x}")
        rc = rc or EXIT_AEXT
    subjects = [d for d in drivers if d not in build_fail]
    findings = ledger_findings(RISCV_LEDGER, RISCV_LEDGER_CAP, subjects, rv_clean, datetime.date.today())
    if build_fail:
        findings = [f for f in findings if not f[1].startswith(tuple(build_fail))]
    if findings:
        print("\nRISC-V leg FAILED:")
        for r, msg in findings:
            print(f"  [{r}] {msg}")
        rc = rc or EXIT_RV
    else:
        print("RISC-V ledger: R1-R6 hold.")
    print(f"\nexit {rc}")
    return rc


if __name__ == "__main__":
    sys.exit(main())
