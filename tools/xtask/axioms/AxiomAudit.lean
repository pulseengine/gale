/-
The trusted Lean half of `cargo xtask check axioms-audit`
(docs/design/axioms-trusted-audit.md, REQ-HONESTY-AXIOMS-001).

It imports ONLY `Lean`: nothing in the audited files can change how it parses
or what it prints (L11 forged every in-file `#print axioms`). For each module
named on the command line it reads the module's compiled `.olean`, imports
that module's own imports, REPLAYS every constant of the module through the
kernel (`Lean.Environment.replay`), and then runs `collectAxioms` on each
constant in the replayed environment.

Usage: `lean --run AxiomAudit.lean <nonce> <Module> ...` with LEAN_PATH set.
The search path is taken from THIS binary's own prefix (`getBuildDir`), not
from a `lean` on PATH, so the oleans are read by the toolchain that wrote them.

Output, one tab-separated record per line; every name and message is
hex-encoded UTF-8, so no name (`«#x] [»`, U+00A0, a newline) can break a
record:
  begin   <nonce>
  module  <hex module> <isModule> <constants> <imports>
  const   <hex module> <hex name> <kind> <unsafe> <partial> <hex axiom>,<hex axiom>,...
  end     <nonce> <const records>
or, instead of `end`, one of
  refuse  <nonce> <hex message>   (exit 3: an olean is missing or unreadable)
  reject  <nonce> <hex message>   (exit 1: the kernel rejected a replayed constant)

What it cannot see: `example` declarations (they leave no constant); unsafe
and partial constants are listed but NOT replayed (`replay` skips them);
the imported modules (Mathlib, Lean itself) are loaded, not replayed, so
their constants are trusted as compiled; the kernel itself.
-/
import Lean
open Lean

def hexDigit (n : Nat) : Char :=
  if n < 10 then Char.ofNat (48 + n) else Char.ofNat (87 + n)

def hex (s : String) : String :=
  s.toUTF8.foldl (fun acc b => (acc.push (hexDigit (b.toNat / 16))).push (hexDigit (b.toNat % 16))) ""

def kindOf : ConstantInfo → String
  | .axiomInfo _ => "axiom"
  | .defnInfo _ => "def"
  | .thmInfo _ => "theorem"
  | .opaqueInfo _ => "opaque"
  | .quotInfo _ => "quot"
  | .inductInfo _ => "inductive"
  | .ctorInfo _ => "ctor"
  | .recInfo _ => "rec"

def axiomsOf (env : Environment) (c : Name) : Array Name :=
  let (_, s) := ((CollectAxioms.collect c).run env).run {}
  s.axioms

def tab (xs : List String) : String := "\t".intercalate xs

/-- One module: read, import its imports, replay its constants, report. -/
def auditModule (m : String) : ExceptT (String × String) IO Nat := do
  let mod := m.toName
  let fname ← (findOLean mod).toBaseIO >>= fun
    | .ok f => pure f
    | .error e => throw ("refuse", "findOLean " ++ m ++ ": " ++ toString e)
  let (md, _region) ← (readModuleData fname).toBaseIO >>= fun
    | .ok r => pure r
    | .error e => throw ("refuse", "readModuleData " ++ m ++ ": " ++ toString e)
  IO.println (tab ["module", hex m, toString md.isModule, toString md.constants.size, toString md.imports.size])
  let env ← (importModules md.imports {} 0 (loadExts := false)).toBaseIO >>= fun
    | .ok e => pure e
    | .error e => throw ("refuse", "importModules for " ++ m ++ ": " ++ toString e)
  let mut newConsts : Std.HashMap Name ConstantInfo := {}
  for c in md.constants do
    newConsts := newConsts.insert c.name c
  let env' ← (env.replay newConsts).toBaseIO >>= fun
    | .ok e => pure e
    | .error e => throw ("reject", "replay " ++ m ++ ": " ++ toString e)
  let mut n := 0
  for c in md.constants do
    let axs := ",".intercalate ((axiomsOf env' c.name).toList.map (fun a => hex a.toString))
    IO.println (tab ["const", hex m, hex c.name.toString, kindOf c, toString c.isUnsafe, toString c.isPartial, axs])
    n := n + 1
  return n

def main (args : List String) : IO UInt32 := do
  match args with
  | [] => IO.println "refuse\t-\t6e6f206e6f6e6365"; return 3
  | nonce :: mods =>
    initSearchPath (← getBuildDir)
    IO.println (tab ["begin", nonce])
    if mods.isEmpty then
      IO.println (tab ["refuse", nonce, hex "no modules named"])
      return 3
    let mut total := 0
    for m in mods do
      match ← (auditModule m).run with
      | .ok n => total := total + n
      | .error (k, msg) =>
        IO.println (tab [k, nonce, hex msg])
        return (if k == "reject" then 1 else 3)
    IO.println (tab ["end", nonce, toString total])
    return 0
