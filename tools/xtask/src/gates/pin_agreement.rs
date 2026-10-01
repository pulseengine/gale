//! A tool version hardcoded in a workflow must match what `varve.toml` resolves.
//!
//! Two workflows download their own synth/loom/meld instead of resolving them
//! through varve, so the toolchain pin exists in three places and can drift.
//! `drv-cross-arch.yml` already carries a comment about this, written after it
//! drifted the first time, extended after it drifted the second, and ending:
//!
//!     "the two pins want a gate, not a note."
//!
//! It then drifted a third time — when the pin moved to layer 2026.10.1 — which
//! is the argument for this file. A warning that has failed to prevent the thing
//! it warns about three times is not a control.
//!
//! WHAT THE DRIFT COSTS, measured at the moment this gate was written: four of
//! the five hardcoded versions disagreed with the pin, `gustos-dissolve.yml`'s
//! synth by **21 minor versions** (0.57.0 against 0.78.0) and its meld by 11.
//! Those gates were green. They were green about a toolchain the project does
//! not use — the same shape as a gate that passes because it cannot distinguish,
//! which this repo has found four times in one release. A dissolve gate that
//! runs synth 0.57.0 is not evidence about the artifact gale ships.
//!
//! WHY IT REFUSES RATHER THAN GUESSES: the comparison needs to know what the
//! pinned layer actually ships, and the only authority for that is varve. If
//! varve is absent, or the layer is not in the store, this gate cannot answer
//! and says so (exit 3, never a pass). Hardcoding a second copy of the expected
//! versions here would make this file a fourth place to drift, which is the
//! defect rather than the fix.

use crate::verdict::Verdict;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "pin-agreement";

/// `<TOOL>_VERSION:` in a workflow maps to this payload name in the layer.
fn tool_for(var: &str) -> Option<&'static str> {
    Some(match var {
        "SYNTH" => "synth",
        "LOOM" => "loom",
        "MELD" => "meld",
        "RIVET" => "rivet",
        "ORDEAL" => "ordeal",
        "WITNESS" => "witness",
        "SPAR" => "spar",
        "WSC" => "wsc",
        "KILND" => "kilnd",
        _ => return None,
    })
}

/// The layer named by `varve.toml`. Read from the file rather than from
/// `varve`'s own resolution, so a stale shim cannot make the gate agree with
/// itself.
fn pinned_layer(repo: &Path) -> Option<String> {
    let t = std::fs::read_to_string(repo.join("varve.toml")).ok()?;
    for l in t.lines() {
        let l = l.trim();
        if let Some(v) = l.strip_prefix("layer") {
            let v = v.trim_start().strip_prefix('=')?.trim();
            return Some(v.trim_matches('"').to_string());
        }
    }
    None
}

/// What that layer ships, as {tool: version}, from varve's own inventory.
fn layer_tools(repo: &Path, layer: &str) -> Option<BTreeMap<String, String>> {
    // varve may be the stale ~/.cargo/bin copy that cannot parse a modern
    // realms file, so prefer its own install location and fall back to PATH.
    let candidates = [
        std::env::var("HOME").map(|h| format!("{h}/.varve/bin/varve")).unwrap_or_default(),
        "varve".to_string(),
    ];
    for exe in candidates.iter().filter(|c| !c.is_empty()) {
        let out = Command::new(exe)
            .args(["inspect", "--layer", layer, "--json"])
            .current_dir(repo)
            .output();
        let Ok(out) = out else { continue };
        if !out.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let mut map = BTreeMap::new();
        // Flat scan over the payload objects: {"kind":"tool",...,"name":"synth",
        // ...,"version":"0.78.0"}. A JSON crate for two fields would be the
        // larger change; the shape is stable and `varve docs inspect` documents it.
        for obj in text.split('{') {
            let name = field(obj, "name");
            let version = field(obj, "version");
            let kind = field(obj, "kind");
            if kind == "tool" && !name.is_empty() && !version.is_empty() {
                map.insert(name, version);
            }
        }
        if !map.is_empty() {
            return Some(map);
        }
    }
    None
}

fn field(obj: &str, key: &str) -> String {
    let pat = format!("\"{key}\":");
    let Some(i) = obj.find(&pat) else { return String::new() };
    let rest = obj[i + pat.len()..].trim_start();
    if !rest.starts_with('"') {
        return String::new();
    }
    rest[1..].split('"').next().unwrap_or("").to_string()
}

/// Every `<TOOL>_VERSION: "x.y.z"` in every workflow, with where it was found.
fn hardcoded(repo: &Path) -> Vec<(String, String, String, usize)> {
    let dir = repo.join(".github/workflows");
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(&dir) else { return out };
    let mut files: Vec<_> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
    files.sort();
    for f in files {
        if f.extension().map(|e| e != "yml" && e != "yaml").unwrap_or(true) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        for (n, l) in text.lines().enumerate() {
            let t = l.trim();
            if t.starts_with('#') {
                continue;
            }
            let Some((lhs, rhs)) = t.split_once(':') else { continue };
            let Some(var) = lhs.strip_suffix("_VERSION") else { continue };
            if tool_for(var).is_none() {
                continue;
            }
            let ver = rhs.trim().trim_matches('"').trim_matches('\'').to_string();
            if ver.is_empty() || ver.contains("${{") {
                continue; // expression, not a literal pin
            }
            let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
            out.push((name, var.to_string(), ver, n + 1));
        }
    }
    out
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Some(layer) = pinned_layer(repo) else {
        return Verdict::Refused("no `layer = ` in varve.toml — the gate has no pin to compare against".into());
    };
    let Some(tools) = layer_tools(repo, &layer) else {
        return Verdict::Refused(format!(
            "cannot read what layer {layer} ships (`varve inspect --layer {layer} --json`). \
             The pinned layer is the only authority for this comparison, and hardcoding a \
             second copy of the expected versions here would make this gate a further place \
             to drift."
        ));
    };

    let found = hardcoded(repo);
    // Vacuity guard: if no workflow hardcodes a version, this gate has nothing
    // to check — which is the GOAL (everything resolves through varve), but it
    // is indistinguishable from a broken scanner. Say which it is.
    if found.is_empty() {
        return Verdict::Pass(vec![
            format!("layer {layer}: no workflow hardcodes a tool version."),
            "That is the end state this gate exists to reach — every tool resolved".into(),
            "through varve. If a *_VERSION was expected here, the scanner is broken.".into(),
        ]);
    }

    let mut lines = vec![format!("varve.toml pins layer {layer}")];
    let mut fail = Vec::new();
    for (file, var, ver, line) in &found {
        let tool = tool_for(var).unwrap();
        match tools.get(tool) {
            Some(want) if want == ver => {
                lines.push(format!("  ok     {file}:{line} {var}_VERSION {ver}"));
            }
            Some(want) => {
                fail.push(format!(
                    "{file}:{line} {var}_VERSION is {ver}, but layer {layer} ships {tool} {want}"
                ));
            }
            None => fail.push(format!(
                "{file}:{line} {var}_VERSION is {ver}, but layer {layer} ships no {tool} at all"
            )),
        }
    }

    if !fail.is_empty() {
        let mut out = vec![format!(
            "FAIL: {} hardcoded version(s) disagree with the pin:",
            fail.len()
        )];
        out.extend(fail.into_iter().map(|f| format!("  {f}")));
        out.push("".into());
        out.push("A gate that downloads its own toolchain is not testing the one gale".into());
        out.push("ships. Either move the value with the layer, or resolve the tool".into());
        out.push("through varve so there is one pin instead of several.".into());
        out.extend(lines);
        return Verdict::Fail(out);
    }

    lines.push(format!("{} hardcoded version(s), all agreeing with the pin.", found.len()));
    Verdict::Pass(lines)
}

/// Controls for the ways this gate could be wrong rather than right: the version
/// parser on each quoting style, the comment and expression exclusions, and the
/// tool-name mapping.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    ck("tool_for maps SYNTH", tool_for("SYNTH") == Some("synth"));
    ck("tool_for rejects an unknown var", tool_for("NODE").is_none());

    let obj = r#""kind":"tool","name":"synth","version":"0.78.0""#;
    ck("field reads name", field(obj, "name") == "synth");
    ck("field reads version", field(obj, "version") == "0.78.0");
    ck("field on a missing key is empty", field(obj, "nope").is_empty());

    // The pin must be read from varve.toml, not from whatever varve resolves —
    // otherwise a stale shim would let the gate agree with itself.
    let layer = pinned_layer(repo);
    ck("pinned_layer reads varve.toml", layer.as_deref().map(|l| !l.is_empty()).unwrap_or(false));

    // The scanner must find the versions that exist today. If this goes empty,
    // `run` would report the "no workflow hardcodes a version" PASS for the
    // wrong reason — a broken scanner looking identical to the goal state.
    let found = hardcoded(repo);
    ck("scanner finds at least one hardcoded version", !found.is_empty());
    ck(
        "scanner captures file, var, version and line",
        found.iter().all(|(f, v, ver, l)| !f.is_empty() && !v.is_empty() && !ver.is_empty() && *l > 0),
    );

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}
