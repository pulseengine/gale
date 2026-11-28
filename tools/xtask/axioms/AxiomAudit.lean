/-
The trusted Lean half of `cargo xtask check axioms-audit`
(docs/design/axioms-trusted-audit.md, REQ-HONESTY-AXIOMS-001).

It imports ONLY `Lean`, and the gate elaborates it with no `LEAN_*`
variable in the environment, so that `import Lean` resolves in the pinned
toolchain's own `lib/lean` and nowhere else. The audited libraries are not on
the search path while this file is elaborated: they cannot change how it
parses or what it prints (L11 forged every in-file `#print axioms`; an
audited module named `Lean` on LEAN_PATH shadowed this import, measured).

Only at run time does `main` install the audit search path, toolchain first:
`<sysroot>/lib/lean`, then each `--path=` directory in order. For THE module
named on the command line as `<Module>=<olean>` it resolves the module with
`findOLean` and refuses unless that is the given `.olean` (so a module that
shares a root with the toolchain or Mathlib is never audited in another's
place), reads it, imports that module's own imports, REPLAYS every constant
of the module through the kernel (`Lean.Environment.replay`), and then runs
`collectAxioms` on each constant in the replayed environment.

Usage: `lean --run AxiomAudit.lean <nonce> --path=<dir>... <Module>=<olean>`
with no LEAN_PATH: exactly ONE module per process. `importModules` maps or
reads the module's imports (all of Mathlib) afresh on every call and never
frees them, so N modules in one process held N copies of Mathlib (measured on
darwin: 2.0 GB peak footprint for one module, 18.5 GB for the 8 in one
process; the hosted Linux runner was OOM-killed at 9 GiB RSS). The gate runs
one process per module, sequentially, each with its own nonce. The sysroot is THIS binary's own prefix (`getBuildDir`),
not a `lean` on PATH, so the oleans are read by the toolchain that wrote them.

Output, one tab-separated record per line; every name and message is
hex-encoded UTF-8, so no name (`«#x] [»`, U+00A0, a newline) can break a
record:
  begin   <nonce>
  module  <hex module> <isModule> <constants> <imports>
  const   <hex module> <hex name> <kind> <unsafe> <partial> <hex axiom>,<hex axiom>,...
  end     <nonce> <const records>
or, instead of `end`, one of
  refuse  <nonce> <hex message>   (exit 3: an olean is missing, unreadable, or not the one given)
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

/-- One module: resolve and match, read, import its imports, replay, report. -/
def auditModule (m : String) (want : System.FilePath) : ExceptT (String × String) IO Nat := do
  let mod := m.toName
  let fname ← (findOLean mod).toBaseIO >>= fun
    | .ok f => pure f
    | .error e => throw ("refuse", "findOLean " ++ m ++ ": " ++ toString e)
  let same ← (do pure ((← IO.FS.realPath fname) == (← IO.FS.realPath want)) : IO Bool).toBaseIO >>= fun
    | .ok b => pure b
    | .error e => throw ("refuse", "realPath for " ++ m ++ ": " ++ toString e)
  if !same then
    throw ("refuse", m ++ " resolves to " ++ fname.toString ++ ", not the manifest's " ++ want.toString ++
      " (its root name is shadowed by, or shadows, another search-path entry)")
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
  | nonce :: rest =>
    IO.println (tab ["begin", nonce])
    let paths := rest.filterMap (fun a => if a.startsWith "--path=" then some (System.FilePath.mk (a.drop 7).toString) else none)
    let mods := rest.filter (fun a => !a.startsWith "--path=")
    -- The toolchain's own library first: `Lean`, `Init`, `Std`, `Lake` can
    -- only ever resolve there. Nothing reads LEAN_PATH: the stock helper that
    -- prepends it to the search path is deliberately not called.
    searchPathRef.set ((← getBuildDir) / "lib" / "lean" :: paths)
    -- One module per process (see the header): a second `importModules` in
    -- the same process would hold a second copy of Mathlib.
    if mods.length != 1 then
      IO.println (tab ["refuse", nonce, hex ("exactly one <Module>=<olean> per run, got " ++ toString mods.length)])
      return 3
    let mut total := 0
    for a in mods do
      match a.splitOn "=" with
      | [m, olean] =>
        match ← (auditModule m olean).run with
        | .ok n => total := total + n
        | .error (k, msg) =>
          IO.println (tab [k, nonce, hex msg])
          return (if k == "reject" then 1 else 3)
      | _ =>
        IO.println (tab ["refuse", nonce, hex ("not <Module>=<olean>: " ++ a)])
        return 3
    IO.println (tab ["end", nonce, toString total])
    return 0
