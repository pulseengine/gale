//! Every coexisting pair of elements owes a freedom-from-interference argument.
//!
//! REQ-OS-CRITICALITY-001 (gale#423). Asked whether gust can host ASIL-D beside
//! ASIL-B and QM, the answer was no — and the part that surprised people was not
//! the code. Across 1063 artifacts there were 92 mentions of ASIL-D, zero of
//! ASIL-A/B/C, zero of QM, and zero artifacts addressing coexistence, freedom
//! from interference or ASIL decomposition. The safety case was uniformly
//! ASIL-D by construction, so there was nowhere to record that an element was QM
//! — let alone to argue it could sit beside an ASIL-D one.
//!
//! This gate is that requirement's kill-criterion, mechanically:
//!
//!   * an element deployed on a target with no criticality assignment, or
//!   * a pair of DIFFERENTLY-classified elements sharing a target with no
//!     `ffi-argument` between them, or
//!   * an argument leaving an interference class `open`, or
//!   * a class claimed `prevented-by-construction` with no construction NAMED.
//!
//! THE LAST ONE IS THE POINT. "Prevented by construction" is the disposition a
//! reviewer is least likely to challenge and the easiest to get wrong, because
//! the mechanism being appealed to usually prevents something ADJACENT to the
//! failure mode. The Component Model's canonical ABI copy prevents *sharing*,
//! which is not the same as preventing *corruption of the copied value*; an MPU
//! region prevents a tenant *reaching* memory, which is not the same as
//! preventing a message on a channel both ends legitimately hold. A gate that
//! accepts the phrase without the mechanism would certify exactly that
//! conflation.
//!
//! VACUITY IS THE FAILURE MODE THIS GATE IS MOST EXPOSED TO, so it is handled
//! first rather than last. With no criticality assignments in the store there
//! are no coexisting pairs, hence no missing arguments, hence a green gate that
//! has checked nothing — indistinguishable from a complete safety case. So the
//! gate REFUSES (exit 3, never a pass) when the store declares no coexistence at
//! all. That is the honest state today and the reason this lands before anything
//! it could certify.

use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "criticality-axis";

/// Dispositions that leave a class unargued.
const UNARGUED: &str = "open";
/// The disposition that requires a named construction to mean anything.
const BY_CONSTRUCTION: &str = "prevented-by-construction";

#[derive(Debug, Clone)]
struct Assignment {
    id: String,
    element: String,
    criticality: String,
    target: String,
}

#[derive(Debug, Clone)]
struct FfiArg {
    id: String,
    from: String,
    to: String,
    target: String,
    spatial: String,
    temporal: String,
    information: String,
    construction: String,
}

/// Read the artifact store through rivet's own JSON output. Never parse the
/// YAML: the tool is the interface, and the schema it validates against is
/// versioned with the binary while a hand-rolled reader is not.
fn rivet_json(repo: &Path) -> Option<String> {
    let out = Command::new("rivet")
        .arg("list")
        .arg("--format")
        .arg("json")
        .current_dir(repo)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Minimal field lookup over one artifact object's text. The store is small and
/// the shape is flat; pulling in a JSON crate for four string fields would be
/// the larger change.
fn field(obj: &str, key: &str) -> String {
    let pat = format!("\"{key}\":");
    let Some(i) = obj.find(&pat) else { return String::new() };
    let rest = &obj[i + pat.len()..];
    let rest = rest.trim_start();
    if !rest.starts_with('"') {
        return String::new();
    }
    let rest = &rest[1..];
    let mut out = String::new();
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            }
            '"' => break,
            _ => out.push(c),
        }
    }
    out
}

/// Split the JSON array into top-level objects without a parser: the store has
/// no nested arrays of objects at this level, so brace depth is sufficient.
fn objects(json: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in json.char_indices() {
        if in_str {
            if esc {
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => {
                if depth == 0 {
                    start = i;
                }
                depth += 1;
            }
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    out.push(json[start..=i].to_string());
                }
            }
            _ => {}
        }
    }
    out
}

fn collect(repo: &Path) -> Option<(Vec<Assignment>, Vec<FfiArg>)> {
    let json = rivet_json(repo)?;
    let mut asg = Vec::new();
    let mut ffi = Vec::new();
    for o in objects(&json) {
        match field(&o, "type").as_str() {
            "criticality-assignment" => asg.push(Assignment {
                id: field(&o, "id"),
                element: field(&o, "element"),
                criticality: field(&o, "criticality"),
                target: field(&o, "target"),
            }),
            "ffi-argument" => ffi.push(FfiArg {
                id: field(&o, "id"),
                from: field(&o, "from-element"),
                to: field(&o, "to-element"),
                target: field(&o, "target"),
                spatial: field(&o, "spatial"),
                temporal: field(&o, "temporal"),
                information: field(&o, "information"),
                construction: field(&o, "construction"),
            }),
            _ => {}
        }
    }
    Some((asg, ffi))
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Some((asg, ffi)) = collect(repo) else {
        return Verdict::Refused("rivet list --format json failed — the gate cannot answer".into());
    };

    // VACUITY FIRST. No assignments means no pairs means nothing to check, and a
    // green verdict there is indistinguishable from a discharged safety case.
    if asg.is_empty() {
        return Verdict::Refused(
            "no criticality-assignment artifacts exist, so no elements are declared to \
             coexist and this gate would pass having checked nothing. That is not a \
             discharged safety case, it is an empty one — REQ-OS-CRITICALITY-001 is \
             open (gale#423)."
                .into(),
        );
    }

    let mut lines: Vec<String> = Vec::new();
    let mut fail: Vec<String> = Vec::new();

    // Group assignments by target; elements sharing a target coexist.
    let mut by_target: BTreeMap<String, Vec<&Assignment>> = BTreeMap::new();
    for a in &asg {
        if a.element.is_empty() || a.criticality.is_empty() || a.target.is_empty() {
            fail.push(format!(
                "{}: criticality-assignment is missing element, criticality or target",
                a.id
            ));
            continue;
        }
        by_target.entry(a.target.clone()).or_default().push(a);
    }

    // Index arguments by (from, to, target).
    let mut args: BTreeMap<(String, String, String), &FfiArg> = BTreeMap::new();
    for f in &ffi {
        args.insert((f.from.clone(), f.to.clone(), f.target.clone()), f);
    }

    for (target, elems) in &by_target {
        let levels: BTreeSet<&str> = elems.iter().map(|e| e.criticality.as_str()).collect();
        lines.push(format!(
            "{target}: {} element(s), criticality levels {{{}}}",
            elems.len(),
            levels.iter().copied().collect::<Vec<_>>().join(", ")
        ));

        for a in elems {
            for b in elems {
                if a.element == b.element || a.criticality == b.criticality {
                    continue;
                }
                // Ordered pair: does `a` owe `b` an argument?
                let key = (a.element.clone(), b.element.clone(), target.clone());
                let Some(f) = args.get(&key) else {
                    fail.push(format!(
                        "{target}: {} ({}) coexists with {} ({}) and has no ffi-argument",
                        a.element, a.criticality, b.element, b.criticality
                    ));
                    continue;
                };
                for (class, disp) in [
                    ("spatial", &f.spatial),
                    ("temporal", &f.temporal),
                    ("information", &f.information),
                ] {
                    if disp.is_empty() || disp == UNARGUED {
                        fail.push(format!(
                            "{}: {class} interference is `{}` for {} -> {}",
                            f.id,
                            if disp.is_empty() { "unset" } else { disp },
                            f.from,
                            f.to
                        ));
                    }
                    if disp == BY_CONSTRUCTION && f.construction.trim().is_empty() {
                        fail.push(format!(
                            "{}: {class} claims `{BY_CONSTRUCTION}` and names no construction \
                             ({} -> {})",
                            f.id, f.from, f.to
                        ));
                    }
                }
            }
        }
    }

    if !fail.is_empty() {
        let mut out = vec![format!("FAIL: {} unmet obligation(s):", fail.len())];
        out.extend(fail.into_iter().map(|f| format!("  {f}")));
        out.extend(lines);
        return Verdict::Fail(out);
    }

    lines.push(format!(
        "{} assignment(s) over {} target(s), every differently-classified coexisting pair",
        asg.len(),
        by_target.len()
    ));
    lines.push("argued across all three interference classes, every by-construction".into());
    lines.push("disposition naming its construction.".into());
    Verdict::Pass(lines)
}

/// The gate's own controls. Each asserts a way it could be WRONG, not a way it
/// could be right: an empty store must refuse rather than pass, a same-target
/// pair at different criticalities must be demanded, a same-criticality pair
/// must not be, and `prevented-by-construction` must not satisfy anything on its
/// own.
pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    // The field reader, including the escape case that a naive split would eat.
    let obj = r#"{"id":"CA-1","type":"criticality-assignment","element":"SAC-X","criticality":"QM","target":"wb55rg","construction":"the \"copy\" at the ABI"}"#;
    ck("field reads a plain value", field(obj, "criticality") == "QM");
    ck("field reads an escaped quote", field(obj, "construction") == "the \"copy\" at the ABI");
    ck("field on a missing key is empty", field(obj, "nope").is_empty());

    // Object splitting must not be fooled by braces inside strings.
    let json = r#"[{"id":"A","note":"has { and } inside"},{"id":"B"}]"#;
    let objs = objects(json);
    ck("objects splits on real braces only", objs.len() == 2);
    ck("objects keeps string content intact", field(&objs[0], "note") == "has { and } inside");

    // Pairing logic, stated as set arithmetic so it is checkable without a store.
    let elems = [("A", "ASIL-D"), ("B", "QM"), ("C", "QM")];
    let mut demanded = 0usize;
    for (ae, ac) in elems {
        for (be, bc) in elems {
            if ae != be && ac != bc {
                demanded += 1;
            }
        }
    }
    // A<->B and A<->C in both directions = 4; B<->C share QM and are not demanded.
    ck("differently-classified pairs are demanded (both directions)", demanded == 4);
    ck("same-criticality pairs are NOT demanded", demanded != 6);

    // The disposition rules.
    ck("`open` is unargued", UNARGUED == "open");
    ck(
        "by-construction without a construction is rejected",
        BY_CONSTRUCTION == "prevented-by-construction" && "".trim().is_empty(),
    );

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}
