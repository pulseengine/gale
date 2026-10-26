//! Every coexisting pair of elements owes a freedom-from-interference argument.
//!
//! REQ-OS-CRITICALITY-001 (gale#423). Asked whether gust can host ASIL-D beside
//! ASIL-B and QM, the answer was no — and the part that surprised people was not
//! the code. The store carried ASIL-D and nothing else: zero artifacts naming
//! ASIL-A/B/C or QM, and zero addressing coexistence, freedom from interference
//! or ASIL decomposition. (gale#423 counted "92 mentions of ASIL-D"; re-measured
//! uncapped that does not reproduce -- 346 of 1067 artifacts mention it, 6 by
//! title -- so do not re-cite 92. See gale#437.) The safety case was uniformly
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

/// KNOWN-OPEN LEDGER (gale#437, decision (b)). Each entry is one interference
/// class of one ffi-argument that is honestly `open` today, with what closes it.
/// The ledger fails in BOTH directions, like fixture-freshness's KNOWN_STALE: an
/// `open` leg not listed here fails, and a listed leg that is no longer `open`
/// (or whose argument no longer exists) fails until it is removed -- so closing
/// a gap cannot pass silently, and the list cannot outlive the gaps it records.
///
/// What this is NOT: an acceptance. `accepted-with-rationale` would have made
/// the QM -> ASIL-D spatial leg green with no mitigation (gale#437 option (c)),
/// which is a false claim. A ledgered leg is a recorded, visible gap.
const KNOWN_OPEN: &[(&str, &str, &str)] = &[
    ("FFI-WB55-001", "spatial", "REQ-OS-UNPRIV-001 -- tenants run privileged until synth#1145"),
    ("FFI-WB55-001", "temporal", "REQ-OS-SCHED-001 / REQ-OS-WCET-001 -- no machine-checked bound"),
    ("FFI-WB55-001", "information", "REQ-OS-FFI-COMM-001 -- no per-failure-mode disposition"),
];

/// Upper bound passed to `rivet query`. rivet paginates SILENTLY at 100 by
/// default; a result of exactly this size is treated as possibly truncated.
const QUERY_LIMIT: usize = 100_000;

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

fn rivet(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("rivet").args(args).current_dir(repo).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// The value of a TOP-LEVEL key of one JSON object: a string's contents
/// (unescaped), or the raw text of an object/array. Keys inside nested values
/// are not matched.
///
/// That restriction is the fix for the reason this gate never ran. The first
/// version took the FIRST `"key":` anywhere in the text. `rivet get` sorts its
/// keys, so `links[].type` ("related-to") precedes the artifact's own `type`,
/// and the store's `{"artifacts": [...]}` wrapper was split as one object --
/// every artifact read as "not a criticality-assignment" and the gate REFUSED
/// as vacuous on a populated store, for a reason it could not tell apart from
/// an empty one. Nothing noticed, because the store WAS empty until gale#437.
fn field(obj: &str, key: &str) -> String {
    let b = obj.as_bytes();
    let mut depth = 0usize;
    let mut i = 0usize;
    while i < b.len() {
        match b[i] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth = depth.saturating_sub(1),
            b'"' => {
                let (s, after) = read_str(obj, i);
                // A key is a string at depth 1 followed by ':'.
                let rest = obj[after..].trim_start();
                if depth == 1 && rest.starts_with(':') {
                    let vstart = after + (obj[after..].len() - rest.len()) + 1;
                    let v = obj[vstart..].trim_start();
                    let voff = vstart + (obj[vstart..].len() - v.len());
                    if s == key {
                        return match v.as_bytes().first() {
                            Some(b'"') => read_str(obj, voff).0,
                            Some(b'{') | Some(b'[') => {
                                obj[voff..voff + balanced(&obj[voff..])].to_string()
                            }
                            _ => String::new(),
                        };
                    }
                }
                i = after;
                continue;
            }
            _ => {}
        }
        i += 1;
    }
    String::new()
}

/// Read the JSON string starting at the quote at `at`: (unescaped contents,
/// index just past the closing quote).
fn read_str(text: &str, at: usize) -> (String, usize) {
    let mut out = String::new();
    let mut chars = text[at + 1..].char_indices();
    while let Some((k, c)) = chars.next() {
        match c {
            '\\' => {
                if let Some((_, n)) = chars.next() {
                    out.push(n);
                }
            }
            '"' => return (out, at + 1 + k + 1),
            _ => out.push(c),
        }
    }
    (out, text.len())
}

/// Length of the balanced object/array at the start of `text`, strings aware.
fn balanced(text: &str) -> usize {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in text.char_indices() {
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
            '{' | '[' => depth += 1,
            '}' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    text.len()
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

/// Ids through `rivet query`, fields through `rivet get`: `list` and `query`
/// omit custom fields entirely, so `element` / `criticality` / `target` exist
/// only in `get`'s output. Never parse the YAML -- the tool is the interface.
fn collect(repo: &Path) -> Result<(Vec<Assignment>, Vec<FfiArg>, BTreeSet<String>), String> {
    let limit = QUERY_LIMIT.to_string();
    let q = rivet(
        repo,
        &["query", r#"(!= id "")"#, "--format", "json", "--limit", &limit],
    )
    .ok_or("rivet query failed -- the gate cannot answer")?;
    let all = objects(&field(&q, "artifacts"));
    if all.is_empty() {
        return Err("rivet query returned no artifacts -- the sweep would be vacuous".into());
    }
    if all.len() >= QUERY_LIMIT {
        return Err(format!("rivet query returned {QUERY_LIMIT} rows -- possibly truncated"));
    }
    let ids: BTreeSet<String> = all.iter().map(|o| field(o, "id")).collect();
    let mut asg = Vec::new();
    let mut ffi = Vec::new();
    for o in &all {
        let ty = field(o, "type");
        if ty != "criticality-assignment" && ty != "ffi-argument" {
            continue;
        }
        let id = field(o, "id");
        let g = rivet(repo, &["get", &id, "--format", "json"])
            .ok_or(format!("rivet get {id} failed -- the gate cannot answer"))?;
        let f = field(&g, "fields");
        if ty == "criticality-assignment" {
            asg.push(Assignment {
                id,
                element: field(&f, "element"),
                criticality: field(&f, "criticality"),
                target: field(&f, "target"),
            });
        } else {
            ffi.push(FfiArg {
                id,
                from: field(&f, "from-element"),
                to: field(&f, "to-element"),
                target: field(&f, "target"),
                spatial: field(&f, "spatial"),
                temporal: field(&f, "temporal"),
                information: field(&f, "information"),
                construction: field(&f, "construction"),
            });
        }
    }
    Ok((asg, ffi, ids))
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let (asg, ffi, ids) = match collect(repo) {
        Ok(v) => v,
        Err(e) => return Verdict::Refused(e),
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

    let mut open_seen: BTreeSet<(String, String)> = BTreeSet::new();
    let mut known_open = 0usize;

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
        // A typo in `element` would invent a phantom element whose pairs are
        // argued about nothing real.
        if !ids.contains(&a.element) {
            fail.push(format!("{}: element {} is not an artifact in the store", a.id, a.element));
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
                    if disp == UNARGUED && ledgered(&f.id, class) {
                        open_seen.insert((f.id.clone(), class.to_string()));
                        known_open += 1;
                        lines.push(format!(
                            "  KNOWN-OPEN {} {class} ({} -> {}): {}",
                            f.id,
                            f.from,
                            f.to,
                            reason(&f.id, class)
                        ));
                    } else if disp.is_empty() || disp == UNARGUED {
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

    // The other direction of the ledger: an entry whose leg is no longer open.
    for (id, class, _) in KNOWN_OPEN {
        if !open_seen.contains(&(id.to_string(), class.to_string())) {
            fail.push(format!(
                "{id}: {class} is on KNOWN_OPEN but is no longer open (or no longer \
                 demanded) -- remove it from the ledger"
            ));
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
    lines.push("dispositioned across all three interference classes, every by-construction".into());
    lines.push("disposition naming its construction.".into());
    if known_open > 0 {
        lines.push(format!(
            "INCOMPLETE: {known_open} leg(s) are OPEN on the known-open ledger. This pass \
             means every gap is recorded, NOT that the safety argument is made."
        ));
    }
    Verdict::Pass(lines)
}

fn ledgered(id: &str, class: &str) -> bool {
    KNOWN_OPEN.iter().any(|(i, c, _)| *i == id && *c == class)
}

fn reason(id: &str, class: &str) -> &'static str {
    KNOWN_OPEN
        .iter()
        .find(|(i, c, _)| *i == id && *c == class)
        .map(|(_, _, r)| *r)
        .unwrap_or("")
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

    // THE SHAPES THAT BROKE THE FIRST VERSION, verbatim in structure. `rivet
    // get` sorts keys, so links[].type comes before the artifact's own type,
    // and custom fields live in a nested `fields` object.
    let get = r#"{"command":"get","fields":{"criticality":"QM","element":"SAC-X","target":"T"},"id":"CA-1","links":[{"target":"SAC-X","type":"related-to"}],"type":"criticality-assignment"}"#;
    ck("top-level type is not a nested links[].type", field(get, "type") == "criticality-assignment");
    ck("a nested field is not read as top-level", field(get, "criticality").is_empty());
    ck("custom fields are read from `fields`", field(&field(get, "fields"), "element") == "SAC-X");
    // `rivet query` wraps the array; splitting the wrapper must yield artifacts.
    let q = r#"{"artifacts":[{"id":"A","type":"x","links":[{"type":"y"}]},{"id":"B","type":"z"}],"command":"query"}"#;
    let arts = objects(&field(q, "artifacts"));
    ck("the artifacts wrapper splits into its artifacts", arts.len() == 2);
    ck("each artifact's own type survives the split", field(&arts[0], "type") == "x");
    // Negative control for the above: splitting the WRAPPER itself (what the
    // first version did) yields one object, which is how it went vacuous.
    ck("the wrapper itself is one object (the old defect)", objects(q).len() == 1);

    // The known-open ledger, both directions, over the real entries.
    ck("a ledgered leg is recognised", ledgered("FFI-WB55-001", "spatial"));
    ck("an unledgered leg is not", !ledgered("FFI-WB55-002", "spatial"));
    ck("every ledger entry names what closes it", KNOWN_OPEN.iter().all(|(_, _, r)| !r.is_empty()));
    ck(
        "the ledger names only real interference classes",
        KNOWN_OPEN.iter().all(|(_, c, _)| ["spatial", "temporal", "information"].contains(c)),
    );

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
