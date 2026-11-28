//! gust-target-gen compiled to a WASI p2 component must write the same files,
//! with the same exit code and the same stderr, as the native binary.
//!
//! The second of gale's own tools moved onto a wasm runtime (verus-strip was
//! the first, gale#556). For every board `targets/regen.sh` regenerates, this
//! gate runs the native binary and the component (under `--runtime`, e.g.
//! wasmtime) with regen.sh's arguments, each into its own empty `out`
//! directory, and compares the exit code, stdout, stderr, the set of files
//! written and every file byte for byte.
//!
//! Verdicts: 0 every board ran on both sides, at least one file was compared
//! and nothing differed; 1 at least one difference (board, file or stream, and
//! the first differing byte offset, or the two exit codes); 3 refused -- fewer
//! boards than the floor, a spawn failure or death by signal, or no file
//! compared and no difference seen (an all-failing run would otherwise pass
//! with nothing shown identical). A refusal is never a pass. A difference wins
//! over the zero-file refusal: "not identical" cannot be vacuous.
//!
//! Both sides see the same relative paths. The native binary runs in a scratch
//! directory holding symlinks `tools` and `targets` to the input root plus an
//! empty `out`; the component gets `--dir <root>::.` and `--dir <scratch>/out::out`.
//! Every path the tool prints (`--reference`, `out/<file>`) is therefore the
//! same string on both sides, and stderr is compared raw, without
//! normalisation. Where stderr cannot match it is reported, not hidden: on an
//! unreadable input both sides exit 3 (I/O error), but the native message
//! carries the host errno (os error 2) where the component prints the WASI
//! errno (os error 44). That is a real difference of the two builds.

use super::wasi_component::{exec, first_diff, opts, wasmtime, Opts, Out};
use crate::verdict::Verdict;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

pub const NAME: &str = "gust-target-gen-wasm";

/// The regen.sh board list is five boards today. The floor is a constant so a
/// regen.sh that lost boards (or a parser that lost them) refuses instead of
/// comparing fewer.
const MIN_BOARDS: usize = 5;

/// The spar-items fixture regen.sh feeds the generator, repo-relative.
const ITEMS: &str = "tools/gust-target-gen/tests/golden/f100.items.json";

/// The regen.sh lines that fix the argument shape this gate reproduces. If
/// regen.sh changes how it calls the generator, the gate refuses rather than
/// comparing a different invocation.
const REGEN_SHAPE: &[&str] = &[
    "FIXTURE=\"$REPO/tools/gust-target-gen/tests/golden/f100.items.json\"",
    "STEM=\"$(printf '%s' \"${BOARD%%::*}\" | tr '[:upper:]' '[:lower:]')\"",
    "--items \"$FIXTURE\" --board \"$BOARD\" --out \"$GEN\"",
    "--reference \"$REPO/targets/reference/$STEM.peripherals.tsv\"",
    "--board-reference \"$REPO/targets/reference/$STEM.board.tsv\"",
];

/// The quoted boards of regen.sh's `for BOARD in ... ; do` loop, in order.
fn parse_regen(text: &str) -> Result<Vec<String>, String> {
    for s in REGEN_SHAPE {
        if !text.contains(s) {
            return Err(format!(
                "targets/regen.sh no longer contains `{s}`; the gate would not reproduce its invocation"
            ));
        }
    }
    let start = text
        .find("for BOARD in")
        .ok_or("targets/regen.sh has no `for BOARD in` loop")?;
    let body = &text[start + "for BOARD in".len()..];
    let end = body
        .find("; do")
        .ok_or("targets/regen.sh `for BOARD in` loop has no `; do`")?;
    let quoted: Vec<&str> = body[..end].split('"').collect();
    // split on '"': the odd-indexed pieces are inside quotes.
    if quoted.len() % 2 == 0 {
        return Err("targets/regen.sh board list has an unbalanced quote".into());
    }
    let boards: Vec<String> = quoted
        .iter()
        .skip(1)
        .step_by(2)
        .map(|s| s.to_string())
        .collect();
    if let Some(b) = boards.iter().find(|b| !b.contains("::") || b.is_empty()) {
        return Err(format!(
            "targets/regen.sh board `{b}` is not `Pkg::Type.impl`"
        ));
    }
    Ok(boards)
}

fn regen_boards(repo: &Path) -> Result<Vec<String>, String> {
    let p = repo.join("targets/regen.sh");
    let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    parse_regen(&t)
}

/// regen.sh's `STEM`: the package before `::`, lowercased.
fn stem(board: &str) -> String {
    board.split("::").next().unwrap_or(board).to_lowercase()
}

/// regen.sh's arguments for one board, with repo-relative inputs and `out`.
fn args(board: &str) -> Vec<String> {
    let s = stem(board);
    vec![
        "--items".into(),
        ITEMS.into(),
        "--board".into(),
        board.into(),
        "--out".into(),
        "out".into(),
        "--reference".into(),
        format!("targets/reference/{s}.peripherals.tsv"),
        "--board-reference".into(),
        format!("targets/reference/{s}.board.tsv"),
    ]
}

/// One side's run: its streams and exit code, and the files it wrote.
struct Side {
    out: Out,
    files: BTreeMap<String, Vec<u8>>,
}

fn read_out(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut m = BTreeMap::new();
    for e in rd {
        let e = e.map_err(|e| format!("{}: {e}", dir.display()))?;
        let p = e.path();
        if !p.is_file() {
            return Err(format!("{} is not a regular file", p.display()));
        }
        let b = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        m.insert(e.file_name().to_string_lossy().into_owned(), b);
    }
    Ok(m)
}

#[cfg(unix)]
fn link(src: &Path, dst: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(src, dst)
        .map_err(|e| format!("symlink {} -> {}: {e}", dst.display(), src.display()))
}

#[cfg(not(unix))]
fn link(_src: &Path, dst: &Path) -> Result<(), String> {
    Err(format!(
        "{}: the native side needs symlinks (unix only)",
        dst.display()
    ))
}

fn mkdir(p: &Path) -> Result<(), String> {
    std::fs::create_dir_all(p).map_err(|e| format!("{}: {e}", p.display()))
}

fn native(bin: &Path, root: &Path, scratch: &Path, a: &[String]) -> Result<Side, String> {
    mkdir(&scratch.join("out"))?;
    link(&root.join("tools"), &scratch.join("tools"))?;
    link(&root.join("targets"), &scratch.join("targets"))?;
    let mut c = Command::new(bin);
    // The component sees no host environment; keep backtrace knobs out of the
    // native run too, so a caller's RUST_BACKTRACE cannot change its stderr.
    c.current_dir(scratch)
        .env_remove("RUST_BACKTRACE")
        .env_remove("RUST_LIB_BACKTRACE")
        .args(a);
    let out = exec(c, &format!("native {}", bin.display()))?;
    let files = read_out(&scratch.join("out"))?;
    Ok(Side { out, files })
}

fn wasm(rt: &Path, comp: &Path, root: &Path, scratch: &Path, a: &[String]) -> Result<Side, String> {
    let o = scratch.join("out");
    mkdir(&o)?;
    let mut c = wasmtime(rt, comp, scratch, &[(root, "."), (&o, "out")]);
    c.args(a);
    let out = exec(c, &format!("runtime {}", rt.display()))?;
    let files = read_out(&o)?;
    Ok(Side { out, files })
}

fn scratch() -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    std::env::temp_dir().join(format!(
        "gale-xtask-tgwasm-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// The comparison. `nroot` is what the native binary reads, `wroot` what the
/// component reads; they are the same tree except in the planted-input tests.
fn compare(o: &Opts, nroot: &Path, wroot: &Path, boards: &[String], floor: usize) -> Verdict {
    if boards.len() < floor.max(1) {
        return Verdict::Refused(format!(
            "{} board(s) to run, need at least {}",
            boards.len(),
            floor.max(1)
        ));
    }
    let tmp = scratch();
    let _ = std::fs::remove_dir_all(&tmp);
    let r = compare_in(o, nroot, wroot, boards, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    r
}

fn compare_in(o: &Opts, nroot: &Path, wroot: &Path, boards: &[String], tmp: &Path) -> Verdict {
    let mut lines = Vec::new();
    let mut diffs = Vec::new();
    let (mut files, mut same_files, mut same_code, mut same_err) = (0usize, 0usize, 0usize, 0usize);
    for (i, b) in boards.iter().enumerate() {
        let a = args(b);
        let n = match native(&o.native, nroot, &tmp.join(format!("{i}-native")), &a) {
            Ok(s) => s,
            Err(e) => return Verdict::Refused(e),
        };
        let w = match wasm(
            &o.runtime,
            &o.wasm,
            wroot,
            &tmp.join(format!("{i}-wasm")),
            &a,
        ) {
            Ok(s) => s,
            Err(e) => return Verdict::Refused(e),
        };
        if n.out.code == w.out.code {
            same_code += 1;
        } else {
            diffs.push(format!(
                "DIFF {b}: exit code native={} wasm={}",
                n.out.code, w.out.code
            ));
        }
        match first_diff(&n.out.stderr, &w.out.stderr) {
            None => same_err += 1,
            Some(at) => diffs.push(format!(
                "DIFF {b}: stderr differs at byte {at} (native {} B, wasm {} B)",
                n.out.stderr.len(),
                w.out.stderr.len()
            )),
        }
        if let Some(at) = first_diff(&n.out.stdout, &w.out.stdout) {
            diffs.push(format!(
                "DIFF {b}: stdout differs at byte {at} (native {} B, wasm {} B)",
                n.out.stdout.len(),
                w.out.stdout.len()
            ));
        }
        let mut names: Vec<&String> = n.files.keys().chain(w.files.keys()).collect();
        names.sort();
        names.dedup();
        for f in &names {
            files += 1;
            match (n.files.get(*f), w.files.get(*f)) {
                (Some(x), Some(y)) => match first_diff(x, y) {
                    None => same_files += 1,
                    Some(at) => diffs.push(format!(
                        "DIFF {b}: {f} differs at byte {at} (native {} B, wasm {} B)",
                        x.len(),
                        y.len()
                    )),
                },
                (Some(_), None) => diffs.push(format!("DIFF {b}: {f} written only by native")),
                (None, _) => diffs.push(format!("DIFF {b}: {f} written only by wasm")),
            }
        }
        lines.push(format!(
            "{b}: exit native={} wasm={}, {} file(s)",
            n.out.code,
            w.out.code,
            names.len()
        ));
    }
    let nb = boards.len();
    let differ = files - same_files;
    lines.extend(diffs.iter().cloned());
    lines.push(format!(
        "{nb} boards, {files} files compared: {same_files} identical, {differ} differ; \
         exit codes equal on {same_code}/{nb}, stderr equal on {same_err}/{nb}"
    ));
    if !diffs.is_empty() {
        return Verdict::Fail(lines);
    }
    if files == 0 {
        return Verdict::Refused(format!(
            "no output file was compared on {nb} board(s) (every run wrote nothing on both sides); \
             nothing was shown identical"
        ));
    }
    Verdict::Pass(lines)
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let o = match opts() {
        Ok(o) => o,
        Err(e) => return Verdict::Refused(e),
    };
    let boards = match regen_boards(repo) {
        Ok(b) => b,
        Err(e) => return Verdict::Refused(e),
    };
    compare(o, repo, repo, &boards, MIN_BOARDS)
}

/// Copy the generator's inputs (the items fixture and targets/reference/*.tsv)
/// of `repo` into `dst`, at the same relative paths.
fn stage(repo: &Path, dst: &Path) -> Result<(), String> {
    let io = |e: std::io::Error| format!("staging {}: {e}", dst.display());
    let items = dst.join(ITEMS);
    mkdir(items.parent().unwrap_or(dst))?;
    std::fs::copy(repo.join(ITEMS), &items).map_err(io)?;
    let rref = repo.join("targets/reference");
    mkdir(&dst.join("targets/reference"))?;
    let rd = std::fs::read_dir(&rref).map_err(io)?;
    for e in rd.filter_map(|e| e.ok()) {
        let n = e.file_name();
        if n.to_string_lossy().ends_with(".tsv") {
            std::fs::copy(e.path(), dst.join("targets/reference").join(&n)).map_err(io)?;
        }
    }
    Ok(())
}

/// Change the integer value of the first `Gust_Target_Props::Vrefint_Ch` in
/// the staged items fixture. It is emitted (`VREFINT_CH`) and not
/// cross-checked against the reference, so the run still succeeds and writes
/// a different file.
fn perturb_items(root: &Path) -> Result<(), String> {
    let p = root.join(ITEMS);
    let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let at = t
        .find("\"name\": \"Gust_Target_Props::Vrefint_Ch\"")
        .ok_or("items fixture has no Vrefint_Ch property")?;
    let k = at
        + t[at..]
            .find("\"Integer\": [")
            .ok_or("Vrefint_Ch has no Integer value")?
        + "\"Integer\": [".len();
    let ds = k + t[k..]
        .find(|c: char| c.is_ascii_digit())
        .ok_or("no digits")?;
    let de = ds + t[ds..].find(|c: char| !c.is_ascii_digit()).unwrap_or(0);
    let v: u64 = t[ds..de]
        .parse()
        .map_err(|e| format!("Vrefint_Ch value: {e}"))?;
    let t = format!("{}{}{}", &t[..ds], v + 1000, &t[de..]);
    std::fs::write(&p, t).map_err(|e| format!("{}: {e}", p.display()))
}

/// Set the base of the first data row of a staged `<stem>.peripherals.tsv`
/// to 0x00000004, which no modelled device has: a REFERENCE MISMATCH.
fn perturb_reference(root: &Path, stem: &str) -> Result<(), String> {
    let p = root.join(format!("targets/reference/{stem}.peripherals.tsv"));
    let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut done = false;
    let out: Vec<String> = t
        .lines()
        .map(|l| {
            if done || l.starts_with('#') || !l.contains('\t') {
                return l.to_string();
            }
            done = true;
            let name = l.split('\t').next().unwrap_or("");
            format!("{name}\t0x00000004")
        })
        .collect();
    if !done {
        return Err(format!("{} has no data row", p.display()));
    }
    std::fs::write(&p, out.join("\n") + "\n").map_err(|e| format!("{}: {e}", p.display()))
}

pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    let boards = match regen_boards(repo) {
        Ok(b) => b,
        Err(e) => return Verdict::Refused(e),
    };
    ck(
        &format!(
            "regen.sh board list parses ({} boards, floor {MIN_BOARDS})",
            boards.len()
        ),
        boards.len() >= MIN_BOARDS,
    );
    let regen = std::fs::read_to_string(repo.join("targets/regen.sh")).unwrap_or_default();
    ck(
        "regen.sh without the --board-reference argument refuses",
        parse_regen(&regen.replace("--board-reference", "--board-ref")).is_err(),
    );

    let o = match opts() {
        Ok(o) => o,
        Err(e) => return Verdict::Refused(format!("self-test plants run the real binaries: {e}")),
    };
    let tmp = std::env::temp_dir().join(format!("gale-xtask-tgwasm-st-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let (good, items, refw, onebad, allbad, panic) = (
        tmp.join("good"),
        tmp.join("items"),
        tmp.join("refw"),
        tmp.join("onebad"),
        tmp.join("allbad"),
        tmp.join("panic"),
    );
    let first = stem(&boards[0]);
    let staged = stage(repo, &good)
        .and_then(|_| stage(repo, &items))
        .and_then(|_| perturb_items(&items))
        .and_then(|_| stage(repo, &refw))
        .and_then(|_| perturb_reference(&refw, &first))
        .and_then(|_| stage(repo, &onebad))
        .and_then(|_| perturb_reference(&onebad, &first))
        .and_then(|_| stage(repo, &allbad))
        .and_then(|_| {
            boards
                .iter()
                .try_for_each(|b| perturb_reference(&allbad, &stem(b)))
        })
        .and_then(|_| stage(repo, &panic))
        .and_then(|_| std::fs::remove_file(panic.join(ITEMS)).map_err(|e| e.to_string()));
    if let Err(e) = staged {
        let _ = std::fs::remove_dir_all(&tmp);
        return Verdict::Refused(e);
    }

    let show = |v: &Verdict| format!("exit {}", v.code());
    let has = |v: &Verdict, pat: &[&str]| match v {
        Verdict::Pass(l) | Verdict::Fail(l) => l.iter().any(|x| pat.iter().all(|p| x.contains(p))),
        Verdict::Refused(_) => false,
    };
    let line = |v: &Verdict, pat: &str| match v {
        Verdict::Pass(l) | Verdict::Fail(l) => l
            .iter()
            .find(|x| x.contains(pat))
            .cloned()
            .unwrap_or_default(),
        Verdict::Refused(r) => r.clone(),
    };

    // Positive control: identical staged inputs pass, with files compared.
    let v = compare(o, &good, &good, &boards, MIN_BOARDS);
    ck(
        &format!(
            "control: identical inputs PASS ({}; {})",
            show(&v),
            line(&v, "files compared")
        ),
        matches!(v, Verdict::Pass(_)),
    );

    // (a) the component reads a perturbed items fixture: a file must differ.
    let v = compare(o, &good, &items, &boards, MIN_BOARDS);
    ck(
        &format!(
            "(a) perturbed items JSON on the wasm side FAILS naming a file and byte ({}; {})",
            show(&v),
            line(&v, "DIFF")
        ),
        matches!(v, Verdict::Fail(_)) && has(&v, &["DIFF", "gust_target_", "differs at byte"]),
    );

    // (a') the component reads a perturbed reference TSV: it hits REFERENCE
    // MISMATCH and exits 1 where native exits 0.
    let v = compare(o, &good, &refw, &boards, MIN_BOARDS);
    ck(
        &format!(
            "(a') perturbed reference TSV on the wasm side FAILS on the exit code ({}; {})",
            show(&v),
            line(&v, "exit code")
        ),
        matches!(v, Verdict::Fail(_)) && has(&v, &["DIFF", "exit code native=0 wasm=1"]),
    );

    // (b) a runtime that does not exist must refuse, not pass or fail.
    let missing = Opts {
        native: o.native.clone(),
        wasm: o.wasm.clone(),
        runtime: tmp.join("no-such-runtime"),
    };
    let v = compare(&missing, &good, &good, &boards, MIN_BOARDS);
    ck(
        &format!("(b) missing runtime REFUSES ({})", show(&v)),
        matches!(v, Verdict::Refused(_)),
    );

    // (c) failure-path control: a reference mismatch on BOTH sides gives the
    // same exit 1 and the same stderr, so the gate reports identical.
    let v = compare(o, &onebad, &onebad, &boards, MIN_BOARDS);
    ck(
        &format!(
            "(c) reference mismatch on both sides PASSES as identical ({}; {})",
            show(&v),
            line(&v, &boards[0])
        ),
        matches!(v, Verdict::Pass(_)) && has(&v, &[&boards[0], "exit native=1 wasm=1, 0 file(s)"]),
    );

    // (d) I/O-error path on both sides: a missing --items file. Both exit 3,
    // but the errno in the message differs (host 2, WASI 44); the gate must
    // report that, not hide it.
    let v = compare(o, &panic, &panic, &boards, MIN_BOARDS);
    let codes = line(&v, "stderr differs");
    ck(
        &format!(
            "(d) missing --items (I/O error) on both sides FAILS on stderr ({}; {codes})",
            show(&v)
        ),
        matches!(v, Verdict::Fail(_)) && has(&v, &["DIFF", "stderr differs"])
            && has(&v, &[&boards[0], "exit native=3 wasm=3, 0 file(s)"]),
    );

    // (e) fewer boards than the floor refuses.
    let v = compare(o, &good, &good, &boards[..MIN_BOARDS - 1], MIN_BOARDS);
    ck(
        &format!(
            "(e) {} boards against a floor of {MIN_BOARDS} REFUSES ({})",
            MIN_BOARDS - 1,
            show(&v)
        ),
        matches!(v, Verdict::Refused(_)),
    );

    // (f) every board fails identically on both sides: zero files compared
    // and no difference seen must refuse, not pass.
    let v = compare(o, &allbad, &allbad, &boards, MIN_BOARDS);
    ck(
        &format!(
            "(f) zero files compared, no difference, REFUSES ({})",
            show(&v)
        ),
        matches!(v, Verdict::Refused(_)),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn regen_board_list_is_the_five_boards() {
        let b = regen_boards(&repo()).expect("regen.sh parses");
        assert_eq!(
            b,
            [
                "STM32F100::Board.vldiscovery",
                "STM32G474::Board.nucleo",
                "STM32WL55::Board.nucleo",
                "STM32WB55::Board.nucleo",
                "STM32G031::Board.nucleo",
            ]
        );
    }

    #[test]
    fn args_follow_regen() {
        let a = args("STM32G474::Board.nucleo");
        assert_eq!(a[7], "targets/reference/stm32g474.peripherals.tsv");
        assert_eq!(a[9], "targets/reference/stm32g474.board.tsv");
        assert_eq!(a[5], "out");
    }

    #[test]
    fn missing_binaries_refuse() {
        let o = Opts {
            native: "/nonexistent/native".into(),
            wasm: "/nonexistent/x.wasm".into(),
            runtime: "/nonexistent/rt".into(),
        };
        let b = regen_boards(&repo()).unwrap();
        assert!(matches!(
            compare(&o, &repo(), &repo(), &b, MIN_BOARDS),
            Verdict::Refused(_)
        ));
    }

    #[test]
    fn too_few_boards_refuse_before_spawning() {
        let o = Opts {
            native: "/x".into(),
            wasm: "/y".into(),
            runtime: "/z".into(),
        };
        assert!(matches!(
            compare(&o, &repo(), &repo(), &[], MIN_BOARDS),
            Verdict::Refused(_)
        ));
    }
}
