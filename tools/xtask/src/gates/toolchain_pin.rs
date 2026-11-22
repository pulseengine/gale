//! Every Rust toolchain CI installs is an exact, named release.
//!
//! Fleet rule (2026-11): the repo names one exact stable Rust in
//! `rust-toolchain.toml`, and every workflow names it too, on a SHA-pinned
//! toolchain action. A floating `stable` is an input that moves under a fixed
//! pin set: 1.99.0 changed which gust:os providers keep a `__stack_pointer`
//! global (gale#434) with no commit in this repo. A floating `nightly` is worse:
//! it moves daily.
//!
//! Before this gate, 24 of the 32 steps using the Rust toolchain install action
//! named no `toolchain:` and took the action branch's default, and coverage.yml's
//! step said `# nightly` on a SHA from the action's `stable` branch -- it had been
//! installing stable all along.
//!
//! SCANNED: `.github/workflows/*.yml|yaml` (the files GitHub runs) and every
//! `action.yml|yaml` under `.github/actions/` (composite actions), plus the
//! repository's rust-toolchain files.
//!
//! What the gate requires:
//!   (a) the root `rust-toolchain.toml` exists, its `[toolchain]` `channel` key
//!       (matched exactly, once) is an exact `X.Y.Z`, it sets no `path`; there is
//!       no legacy `rust-toolchain` file at the root (rustup prefers it) and no
//!       `rust-toolchain`/`rust-toolchain.toml` anywhere else (rustup uses the
//!       nearest one, so a nested file silently re-selects for its subtree);
//!   (b) every step using the action has exactly one `with: toolchain:`, block
//!       or flow (`with: { toolchain: X }`);
//!   (c) that value is an exact `X.Y.Z` or a DATED `<channel>-YYYY-MM-DD`, never
//!       a bare `stable` / `nightly` / `beta`, `X.Y`, a variable or expression;
//!   (d) the action ref (after the last `@`) is a 40-hex commit SHA;
//!   (e) a trailing comment on the `uses:` line names the installed toolchain
//!       and NO other toolchain-like word (bare stable/nightly/beta, `X.Y`,
//!       `X.Y.Z`, `<channel>-<digit>...`);
//!   (f) an exact `X.Y.Z` there equals the `rust-toolchain.toml` channel;
//!   (g) shell, tokenised on `; | & ( ) \``, quotes and whitespace, with `\`
//!       continuations joined and `#` comments dropped: `+<tc>` after a command
//!       whose basename is cargo, rustc, rustfmt, rustdoc, rustup, cargo-clippy
//!       or clippy-driver (case-insensitive, so `$CARGO` counts); every name of
//!       `rustup default|run|override set|install|update|toolchain
//!       install|add|update`; `--toolchain X|=X`; `--default-toolchain X|=X`;
//!       `RUSTUP_TOOLCHAIN=X`; each obeys (c). A rustup-init run (rustup-init
//!       in command position, or `sh.rustup.rs` piped to a shell) without
//!       `--default-toolchain` fails: it installs `stable`;
//!   (h) any YAML key `toolchain` or `RUSTUP_TOOLCHAIN`, anywhere (another
//!       action's input, a reusable workflow's input, a job env), obeys (c);
//!   (i) a `container:`/`image:` naming the `rust` image carries an `X.Y.Z` tag.
//!
//! The scanner REFUSES when it cannot answer: no workflows; a file with more
//! non-comment lines naming the action (case-insensitive, `uses:` or not) than
//! steps it parsed -- an anchor, alias, block scalar or flow layout; or a clean
//! tree in which no step uses the action at all (the step checks would have
//! checked nothing).
//!
//! CANNOT_SEE (deliberately out of scope; a pass says nothing about these):
//! - scripts a workflow runs (`./ci/build.sh`): only workflow/action YAML is read;
//! - the contents of reusable workflows in other repositories (their
//!   `toolchain:` input IS checked by (h));
//! - other toolchain-install actions beyond their `toolchain:` input;
//! - container images other than `rust`, and whatever a self-hosted runner or
//!   image has preinstalled;
//! - the value behind a variable or `${{ }}` expression: it is failed, not
//!   resolved;
//! - MODULE.bazel's rules_rust toolchain (`versions = [...]`) for Bazel builds;
//! - Kani's own nightly: cargo-kani installs the toolchain its release bundles,
//!   so it is pinned only as far as the `kani-verifier` version is;
//! - `RUSTC_BOOTSTRAP` (nightly features on a stable compiler).

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "toolchain-pin";

/// The action, lowercase (GitHub resolves owner/repo case-insensitively).
const ACTION: &str = "dtolnay/rust-toolchain";

const RUST_CMDS: &[&str] = &["cargo", "rustc", "rustfmt", "rustdoc", "rustup", "cargo-clippy", "clippy-driver"];
/// Shell words after which a rustup-init token is in command position.
const CMD_LEAD: &[&str] = &["sh", "bash", "dash", "zsh", "exec", "sudo", "then", "do", "else", "if", "env", "time"];
/// rustup flags that take a value as the next word.
const VALUE_FLAGS: &[&str] = &["--profile", "-c", "--component", "-t", "--target", "--toolchain"];

/// `X.Y.Z`, all numeric.
fn is_exact_release(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// `stable|beta|nightly-YYYY-MM-DD`.
fn is_dated(s: &str) -> bool {
    for ch in ["stable-", "beta-", "nightly-"] {
        if let Some(d) = s.strip_prefix(ch) {
            let b = d.as_bytes();
            return b.len() == 10
                && b[4] == b'-'
                && b[7] == b'-'
                && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
        }
    }
    false
}

fn is_pinned(s: &str) -> bool {
    is_exact_release(s) || is_dated(s)
}

/// A word that reads as a toolchain name, pinned or not (rule (e)).
fn looks_like_toolchain(w: &str) -> bool {
    let w = w.trim_end_matches(['.', ':', '!', '?']);
    if matches!(w, "stable" | "nightly" | "beta") || is_pinned(w) {
        return true;
    }
    let parts: Vec<&str> = w.split('.').collect();
    if (2..=3).contains(&parts.len()) && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit())) {
        return true;
    }
    ["stable-", "beta-", "nightly-"]
        .iter()
        .any(|p| w.strip_prefix(p).is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit())))
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"').trim_matches('\'').trim()
}

/// The code part of a YAML (or shell) line: everything before a `#` that starts
/// the line or follows whitespace, outside quotes.
fn strip_comment(line: &str) -> &str {
    let (mut sq, mut dq) = (false, false);
    let mut prev_ws = true;
    for (i, c) in line.char_indices() {
        match c {
            '\'' if !dq => sq = !sq,
            '"' if !sq => dq = !dq,
            '#' if !sq && !dq && prev_ws => return &line[..i],
            _ => {}
        }
        prev_ws = c.is_whitespace();
    }
    line
}

/// (key, value) of a `key: value` line, after an optional `- ` (any width) and
/// with quotes around the key removed. None when the line is not a mapping entry.
fn key_value(code: &str) -> Option<(String, String, usize)> {
    let t = code.trim_start();
    let indent = code.len() - t.len();
    let (rest, col) = match t.strip_prefix('-') {
        Some(r) if r.starts_with(' ') => {
            let r2 = r.trim_start();
            (r2, indent + (t.len() - r2.len()))
        }
        _ => (t, indent),
    };
    let colon = rest.find(':')?;
    let key = unquote(&rest[..colon]);
    if key.is_empty() || key.contains(' ') && !rest[..colon].trim().starts_with(['"', '\'']) {
        return None;
    }
    Some((key.to_string(), rest[colon + 1..].trim().to_string(), col))
}

/// `toolchain` values in a flow map `{ a: b, toolchain: X }`.
fn flow_values(v: &str, key: &str) -> Vec<String> {
    let inner = v.trim().trim_start_matches('{').trim_end_matches('}');
    inner
        .split(',')
        .filter_map(|kv| kv.split_once(':'))
        .filter(|(k, _)| unquote(k) == key)
        .map(|(_, v)| unquote(v).to_string())
        .collect()
}

/// The `[toolchain]` table: (channel values, has `path`).
fn toolchain_table(text: &str) -> (Vec<String>, bool) {
    let mut in_tc = false;
    let (mut chans, mut path) = (Vec::new(), false);
    for line in text.lines() {
        let t = line.split('#').next().unwrap_or("").trim();
        if t.starts_with('[') {
            in_tc = t == "[toolchain]";
            continue;
        }
        if !in_tc {
            continue;
        }
        if let Some((k, v)) = t.split_once('=') {
            match unquote(k) {
                "channel" => chans.push(unquote(v).to_string()),
                "path" => path = true,
                _ => {}
            }
        }
    }
    (chans, path)
}

#[derive(Debug)]
struct Step {
    line: usize,
    sha: String,
    comment: Option<String>,
    toolchains: Vec<String>,
    toolchain_lines: Vec<usize>,
}

/// Every step using the action in one YAML file. A step's keys sit at the
/// column of its `uses:` key; the step ends at a line left of that column, or
/// a `-` item at or left of it. `toolchain:` counts only directly inside the
/// step's own `with:` (block at the first nested column, or a flow map).
fn steps(text: &str) -> Vec<Step> {
    let lines: Vec<&str> = text.lines().map(|l| l.trim_end_matches('\r')).collect();
    let mut out = Vec::new();
    for (i, raw) in lines.iter().enumerate() {
        let code = strip_comment(raw);
        let Some((k, v, key_col)) = key_value(code) else { continue };
        let target = unquote(&v).to_ascii_lowercase();
        if k != "uses" || !target.starts_with(ACTION) {
            continue;
        }
        let sha = target.rsplit_once('@').map(|(_, r)| r.to_string()).unwrap_or_default();
        let comment = raw[code.len()..].trim().strip_prefix('#').map(|c| c.trim().to_string());
        let mut step = Step { line: i + 1, sha, comment, toolchains: Vec::new(), toolchain_lines: Vec::new() };
        let (mut in_with, mut with_col) = (false, None::<usize>);
        for (j, l) in lines.iter().enumerate().skip(i + 1) {
            let c = strip_comment(l);
            let t = c.trim_start();
            if t.is_empty() {
                continue;
            }
            let li = c.len() - t.len();
            if li < key_col || (li <= key_col && t.starts_with('-')) {
                break;
            }
            if li == key_col {
                let kv = key_value(c);
                in_with = kv.as_ref().is_some_and(|(k, _, _)| k == "with");
                with_col = None;
                if let Some((k, v, _)) = kv {
                    if k == "with" && v.starts_with('{') {
                        for tc in flow_values(&v, "toolchain") {
                            step.toolchains.push(tc);
                            step.toolchain_lines.push(j + 1);
                        }
                        in_with = false;
                    }
                }
            } else if in_with {
                let wc = *with_col.get_or_insert(li);
                if li == wc {
                    if let Some((k, v, _)) = key_value(c) {
                        if k == "toolchain" {
                            step.toolchains.push(unquote(&v).to_string());
                            step.toolchain_lines.push(j + 1);
                        }
                    }
                }
            }
        }
        out.push(step);
    }
    out
}

/// YAML lines joined across shell `\` continuations, comments dropped, with the
/// 1-based number of the first physical line.
fn logical_lines(text: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut cont = false;
    for (n, raw) in text.lines().enumerate() {
        let code = strip_comment(raw.trim_end_matches('\r')).trim_end();
        let (body, more) = match code.strip_suffix('\\') {
            Some(b) => (b, true),
            None => (code, false),
        };
        if cont {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(body.trim_start());
            }
        } else {
            out.push((n + 1, body.to_string()));
        }
        cont = more;
    }
    out
}

fn base(tok: &str) -> String {
    tok.rsplit('/').next().unwrap_or(tok).trim_start_matches('$').trim_matches(['{', '}']).to_ascii_lowercase()
}

/// rustup names after a verb, skipping flags (and a flag's value).
fn names(toks: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut k = 0;
    while k < toks.len() {
        let t = toks[k];
        if t.starts_with('-') {
            if !t.contains('=') && VALUE_FLAGS.contains(&t) {
                k += 1;
            }
        } else {
            out.push(t.to_string());
        }
        k += 1;
    }
    out
}

/// (g): every toolchain name given to cargo/rustup/rustup-init on one logical
/// line, as (what, name); `None` name = a rustup-init with no --default-toolchain.
fn invocations(line: &str) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let segs: Vec<&str> = line.split([';', '|', '&', '(', ')', '`', '\n']).collect();
    let has_default = line.split_whitespace().any(|w| unquote(w).starts_with("--default-toolchain"));
    for seg in &segs {
        let cleaned = seg.replace(['"', '\''], " ");
        let toks: Vec<&str> = cleaned.split_whitespace().collect();
        for (i, tok) in toks.iter().enumerate() {
            let b = base(tok);
            if RUST_CMDS.contains(&b.as_str()) {
                if let Some(n) = toks.get(i + 1).and_then(|n| n.strip_prefix('+')).filter(|n| !n.is_empty()) {
                    out.push((format!("{b} +"), Some(n.to_string())));
                }
            }
            for flag in ["--toolchain", "--default-toolchain"] {
                if *tok == flag {
                    out.push((flag.to_string(), Some(toks.get(i + 1).map(|s| s.to_string()).unwrap_or_default())));
                } else if let Some(v) = tok.strip_prefix(flag).and_then(|r| r.strip_prefix('=')) {
                    out.push((flag.to_string(), Some(v.to_string())));
                }
            }
            if let Some(v) = tok.strip_prefix("RUSTUP_TOOLCHAIN=") {
                out.push(("RUSTUP_TOOLCHAIN=".into(), Some(v.to_string())));
            }
            if b.starts_with("rustup-init") && (i == 0 || CMD_LEAD.contains(&base(toks[i - 1]).as_str())) && !has_default {
                out.push(("rustup-init without --default-toolchain".into(), None));
            }
            if b == "rustup" {
                let mut k = i + 1;
                while k < toks.len() && (toks[k].starts_with('+') || toks[k].starts_with('-')) {
                    k += 1;
                }
                let rest = &toks[k.min(toks.len())..];
                let verb = rest.first().copied().unwrap_or("");
                let sub = rest.get(1).copied().unwrap_or("");
                let found: Vec<String> = match (verb, sub) {
                    ("default", _) | ("run", _) => names(&rest[1..]).into_iter().take(1).collect(),
                    ("override", "set") => names(&rest[2..]).into_iter().take(1).collect(),
                    ("install", _) | ("update", _) => names(&rest[1..]),
                    ("toolchain", "install" | "add" | "update") => names(&rest[2..]),
                    _ => Vec::new(),
                };
                let label = if verb == "toolchain" || verb == "override" { format!("rustup {verb} {sub}") } else { format!("rustup {verb}") };
                for n in found {
                    out.push((label.clone(), Some(n)));
                }
            }
        }
    }
    // `curl https://sh.rustup.rs | sh` runs rustup-init with its default (stable).
    if line.contains("sh.rustup.rs")
        && segs.iter().skip(1).any(|s| ["sh", "bash", "dash", "zsh"].contains(&s.split_whitespace().next().map(base).unwrap_or_default().as_str()))
        && !has_default
    {
        out.push(("sh.rustup.rs piped to a shell without --default-toolchain".into(), None));
    }
    out
}

/// Everything the gate reads, so the self-test runs the code the real tree does.
#[derive(Default, Clone)]
struct Tree {
    /// root rust-toolchain.toml
    toml: Option<String>,
    /// root legacy `rust-toolchain`
    legacy: bool,
    /// any other rust-toolchain(.toml), repo-relative
    nested: Vec<String>,
    /// workflow and composite-action YAML, (repo-relative name, text)
    files: Vec<(String, String)>,
}

fn check(tree: &Tree) -> Verdict {
    if !tree.files.iter().any(|(n, _)| n.starts_with(".github/workflows/")) {
        return Verdict::Refused("no workflows found -- the sweep would be vacuous".into());
    }
    let mut bad: Vec<String> = Vec::new();
    let mut chan = None;
    if tree.legacy {
        bad.push("(a) rust-toolchain: a legacy toolchain file at the root -- rustup prefers it over rust-toolchain.toml".into());
    }
    for n in &tree.nested {
        bad.push(format!("(a) {n}: a nested toolchain file re-selects the toolchain for its subtree"));
    }
    match &tree.toml {
        None => bad.push("(a) rust-toolchain.toml: missing -- the build floats on whatever rustup defaults to".into()),
        Some(text) => {
            let (chans, path) = toolchain_table(text);
            if path {
                bad.push("(a) rust-toolchain.toml: `path` names a custom toolchain, not a release".into());
            }
            match chans.as_slice() {
                [] => bad.push("(a) rust-toolchain.toml: no `channel` key under [toolchain]".into()),
                [c] if is_exact_release(c) => chan = Some(c.clone()),
                [c] => bad.push(format!("(a) rust-toolchain.toml: channel `{c}` is not an exact X.Y.Z")),
                _ => bad.push("(a) rust-toolchain.toml: more than one `channel` key".into()),
            }
        }
    }

    let (mut nsteps, mut nexact, mut ndated, mut ninv) = (0usize, 0usize, 0usize, 0usize);
    for (name, text) in &tree.files {
        let found = steps(text);
        let mentions = text
            .lines()
            .filter(|l| strip_comment(l).to_ascii_lowercase().contains(ACTION))
            .count();
        if mentions != found.len() {
            return Verdict::Refused(format!(
                "{name}: {mentions} line(s) name the Rust toolchain install action but the scanner parsed {} \
                 step(s) -- an anchor, alias, block scalar or flow layout it does not understand, so it cannot answer",
                found.len()
            ));
        }
        let mut step_lines = std::collections::BTreeSet::new();
        for s in &found {
            nsteps += 1;
            step_lines.extend(s.toolchain_lines.iter().copied());
            let at = format!("{name}:{}", s.line);
            if !(s.sha.len() == 40 && s.sha.bytes().all(|b| b.is_ascii_hexdigit())) {
                bad.push(format!("(d) {at}: ref `{}` is not a 40-hex commit SHA", s.sha));
            }
            let tc = match s.toolchains.as_slice() {
                [] => {
                    bad.push(format!("(b) {at}: no `with: toolchain:` -- installs whatever the action branch defaults to"));
                    continue;
                }
                [tc] => tc,
                _ => {
                    bad.push(format!("(b) {at}: more than one `toolchain:` ({})", s.toolchains.join(", ")));
                    continue;
                }
            };
            if !is_pinned(tc) {
                bad.push(format!("(c) {at}: toolchain `{tc}` is not an exact X.Y.Z or a dated <channel>-YYYY-MM-DD"));
                continue;
            }
            if is_exact_release(tc) {
                nexact += 1;
                if let Some(c) = &chan {
                    if tc != c {
                        bad.push(format!("(f) {at}: toolchain `{tc}` differs from rust-toolchain.toml's `{c}`"));
                    }
                }
            } else {
                ndated += 1;
            }
            if let Some(cm) = &s.comment {
                let named: Vec<&str> = cm
                    .split(|c: char| c.is_whitespace() || ",;()[]".contains(c))
                    .filter(|w| looks_like_toolchain(w))
                    .map(|w| w.trim_end_matches(['.', ':', '!', '?']))
                    .collect();
                if !named.contains(&tc.as_str()) || named.iter().any(|w| w != tc) {
                    bad.push(format!(
                        "(e) {at}: comment `# {cm}` must name exactly the toolchain it installs (`{tc}`), and no other"
                    ));
                }
            }
        }
        for (n, raw) in text.lines().enumerate() {
            if step_lines.contains(&(n + 1)) {
                continue;
            }
            let code = strip_comment(raw.trim_end_matches('\r'));
            let Some((k, v, _)) = key_value(code) else { continue };
            let vals: Vec<String> = if v.starts_with('{') {
                [flow_values(&v, "toolchain"), flow_values(&v, "RUSTUP_TOOLCHAIN")].concat()
            } else if k == "toolchain" || k == "RUSTUP_TOOLCHAIN" {
                vec![unquote(&v).to_string()]
            } else {
                Vec::new()
            };
            for val in vals {
                if !is_pinned(&val) {
                    bad.push(format!("(h) {name}:{}: `{k}: {val}` is not an exact X.Y.Z or a dated nightly", n + 1));
                }
            }
            if k == "image" || k == "container" {
                let img = unquote(&v);
                let last = img.rsplit('/').next().unwrap_or(img);
                if last == "rust" || last.starts_with("rust:") {
                    let tag = last.split_once(':').map(|(_, t)| t).unwrap_or("");
                    if !is_exact_release(tag.split('-').next().unwrap_or("")) {
                        bad.push(format!("(i) {name}:{}: image `{img}` is not tagged with an exact X.Y.Z", n + 1));
                    }
                }
            }
        }
        for (line, l) in logical_lines(text) {
            // Prose keys (a step or job `name:`) are not shell.
            if key_value(&l).is_some_and(|(k, _, _)| k == "name" || k == "description") {
                continue;
            }
            for (what, tc) in invocations(&l) {
                ninv += 1;
                match tc {
                    None => bad.push(format!("(g) {name}:{line}: {what} -- it installs `stable`")),
                    Some(tc) if !is_pinned(&tc) => {
                        bad.push(format!("(g) {name}:{line}: `{what}{}{tc}` is not an exact X.Y.Z or a dated nightly", if what.ends_with('+') || what.ends_with('=') { "" } else { " " }))
                    }
                    Some(_) => {}
                }
            }
        }
    }

    if !bad.is_empty() {
        let mut l = vec![format!("FAIL: {} toolchain(s) not pinned exactly:", bad.len())];
        l.extend(bad.into_iter().map(|b| format!("  {b}")));
        return Verdict::Fail(l);
    }
    if nsteps == 0 {
        return Verdict::Refused(
            "no step uses the Rust toolchain install action -- the step checks would have checked nothing".into(),
        );
    }
    Verdict::Pass(vec![format!(
        "rust-toolchain.toml channel {}; {nsteps} Rust toolchain install step(s) in {} file(s), all SHA-pinned \
         with an explicit toolchain ({nexact} exact release, {ndated} dated); {ninv} shell toolchain name(s), all \
         pinned. See CANNOT_SEE in the gate's doc for what this does not cover.",
        chan.unwrap_or_default(),
        tree.files.len()
    )])
}

fn yaml_files(dir: &Path, recurse: bool, only_action: bool, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            if recurse {
                yaml_files(&p, recurse, only_action, out);
            }
            continue;
        }
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        let yaml = name.ends_with(".yml") || name.ends_with(".yaml");
        if yaml && (!only_action || name == "action.yml" || name == "action.yaml") {
            out.push(p);
        }
    }
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut tree = Tree {
        toml: std::fs::read_to_string(repo.join("rust-toolchain.toml")).ok(),
        legacy: repo.join("rust-toolchain").is_file(),
        ..Tree::default()
    };
    // Tracked AND untracked-but-not-ignored files: a nested toolchain file a
    // developer has not added yet still re-selects rustc for its subtree.
    let Ok(out) = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output()
    else {
        return Verdict::Refused("cannot run git ls-files".into());
    };
    if !out.status.success() {
        return Verdict::Refused(format!("git ls-files failed in {}", repo.display()));
    }
    for p in String::from_utf8_lossy(&out.stdout).split('\0').filter(|p| p.contains('/')) {
        let b = p.rsplit('/').next().unwrap_or("");
        if b == "rust-toolchain" || b == "rust-toolchain.toml" {
            tree.nested.push(p.to_string());
        }
    }
    tree.nested.sort();
    tree.nested.dedup();
    let mut paths = Vec::new();
    yaml_files(&repo.join(".github/workflows"), false, false, &mut paths);
    yaml_files(&repo.join(".github/actions"), true, true, &mut paths);
    paths.sort();
    for p in paths {
        let name = p.strip_prefix(repo).unwrap_or(&p).to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(&p) else {
            return Verdict::Refused(format!("cannot read {name}"));
        };
        tree.files.push((name, text));
    }
    check(&tree)
}

const GOOD_TC: &str = "[toolchain]\nchannel = \"1.100.0\"\ncomponents = [\"clippy\"]\n";

/// Each step layout the scanner must read: `- uses:` opening the step, `uses:`
/// under `- name:`, an `if:` between `uses:` and `with:`, `toolchain:` after
/// other `with:` keys, a truthful comment, pinned shell names.
const GOOD_WF: &str = "\
on: push
jobs:
  a:
    runs-on: x
    steps:
      - uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca # installs 1.100.0 (stable-branch SHA)
        with:
          toolchain: 1.100.0
      - name: Install nightly
        if: true
        uses: dtolnay/rust-toolchain@6a52aad76a94a5fbc50d567bb5be24419801c1fb # installs nightly-2026-09-17 (nightly-branch SHA)
        with:
          components: rust-src
          toolchain: nightly-2026-09-17
      - run: |
          # cargo +nightly in a comment is not an invocation
          cargo +nightly-2026-09-17 fuzz run x
          sh rustup-init.sh -y --default-toolchain 1.100.0
";

fn tree(wf: &str) -> Tree {
    Tree { toml: Some(GOOD_TC.into()), files: vec![(".github/workflows/w.yml".into(), wf.into())], ..Tree::default() }
}

fn fails_with(v: &Verdict, needle: &str) -> bool {
    matches!(v, Verdict::Fail(l) if l.iter().any(|x| x.contains(needle)))
}

/// A workflow carrying one good step plus `extra` steps, so a plant's own rule
/// fires rather than the zero-steps refusal.
fn with_steps(extra: &str) -> String {
    format!("{GOOD_WF}{extra}")
}

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    let pass = |t: &Tree| matches!(check(t), Verdict::Pass(_));
    let fail = |t: &Tree, needle: &str| fails_with(&check(t), needle);
    let refused = |t: &Tree| matches!(check(t), Verdict::Refused(_));

    ck(
        "positive control: exact channel, two pinned steps (one dated), pinned shell names -> PASS",
        matches!(check(&tree(GOOD_WF)), Verdict::Pass(l) if l.iter().any(|x| x.contains("2 Rust toolchain install step(s)") && x.contains("2 shell toolchain name(s)"))),
    );

    // Positive controls for forms a YAML parser accepts and that ARE pinned.
    let pc = |what: &str, wf: String| (what.to_string(), wf);
    for (what, wf) in [
        pc("`with: # comment`", GOOD_WF.replace("        with:\n          toolchain: 1.100.0", "        with: # pinned\n          toolchain: 1.100.0")),
        pc("`with: { toolchain: 1.100.0 }`", GOOD_WF.replace("        with:\n          toolchain: 1.100.0", "        with: { toolchain: 1.100.0 }")),
        pc("`-   uses:` (wide dash)", GOOD_WF.replace("      - uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca # installs 1.100.0 (stable-branch SHA)\n        with:\n          toolchain: 1.100.0", "      -   uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca\n          with:\n            toolchain: 1.100.0")),
        pc("a commented-out `cargo +nightly` after a command", with_steps("      - run: cargo test # was: cargo +nightly test\n")),
        pc("prose `name: ... (Cargo + Bazel)`", with_steps("      - name: build (Cargo + Bazel)\n        run: cargo test\n")),
        pc("`toolchain: \"1.100.0\"` quoted", GOOD_WF.replace("toolchain: 1.100.0", "toolchain: \"1.100.0\"")),
        pc("CRLF line endings", GOOD_WF.replace('\n', "\r\n")),
        pc("rustup-init download (not a run) beside a pinned run", with_steps("      - run: curl -o /tmp/rustup-init.sh https://sh.rustup.rs\n")),
        pc("`rustup toolchain install --profile minimal nightly-2026-09-17`", with_steps("      - run: rustup toolchain install --profile minimal -c rust-src nightly-2026-09-17\n")),
    ] {
        ck(&format!("positive control: {what} -> PASS"), pass(&tree(&wf)));
    }
    ck(
        "positive control: `[toolchain] # comment` -> PASS",
        pass(&Tree { toml: Some("[toolchain] # pinned\nchannel = \"1.100.0\"\n".into()), ..tree(GOOD_WF) }),
    );

    // (a)
    for ch in ["stable", "nightly", "1.100"] {
        let tc = GOOD_TC.replace("1.100.0\"", &format!("{ch}\""));
        ck(&format!("plant (a): channel = \"{ch}\" -> FAIL"), fail(&Tree { toml: Some(tc), ..tree(GOOD_WF) }, "(a)"));
    }
    ck("plant (a): no rust-toolchain.toml -> FAIL", fail(&Tree { toml: None, ..tree(GOOD_WF) }, "(a)"));
    ck("plant (a): a legacy `rust-toolchain` beside the .toml -> FAIL", fail(&Tree { legacy: true, ..tree(GOOD_WF) }, "(a) rust-toolchain: a legacy"));
    ck(
        "plant (a): a nested sub/rust-toolchain.toml -> FAIL",
        fail(&Tree { nested: vec!["sub/rust-toolchain.toml".into()], ..tree(GOOD_WF) }, "(a) sub/rust-toolchain.toml"),
    );
    ck(
        "plant (a): `channel_note = \"1.100.0\"` before `channel = \"stable\"` (exact key) -> FAIL",
        fail(&Tree { toml: Some("[toolchain]\nchannel_note = \"1.100.0\"\nchannel = \"stable\"\n".into()), ..tree(GOOD_WF) }, "channel `stable`"),
    );
    ck(
        "plant (a): `path = ...` in [toolchain] -> FAIL",
        fail(&Tree { toml: Some(format!("{GOOD_TC}path = \"/opt/x\"\n")), ..tree(GOOD_WF) }, "`path`"),
    );
    ck(
        "plant (a): inline `toolchain = {{ channel = \"stable\" }}` -> FAIL",
        fail(&Tree { toml: Some("toolchain = { channel = \"stable\" }\n".into()), ..tree(GOOD_WF) }, "(a)"),
    );

    // (b)
    let no_with = GOOD_WF.replace("        with:\n          toolchain: 1.100.0\n", "");
    ck("plant (b): a step with no `with:` at all -> FAIL", fail(&tree(&no_with), "(b) .github/workflows/w.yml:6"));
    let other_keys = GOOD_WF.replace("          toolchain: nightly-2026-09-17\n", "");
    ck("plant (b): `with:` carrying only other keys -> FAIL", fail(&tree(&other_keys), "(b) .github/workflows/w.yml:11"));
    let next_step = with_steps("      - uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca\n      - uses: some/other@39008827e6b4319a349e9a42dfc90579dd25f1ca\n        with:\n          components: x\n");
    ck("plant (b): the NEXT step's `with:` does not count for this one -> FAIL", fail(&tree(&next_step), "(b) .github/workflows/w.yml:19"));
    let dup = GOOD_WF.replace("          toolchain: 1.100.0\n", "          toolchain: 1.100.0\n          toolchain: 1.100.0\n");
    ck("plant (b): two `toolchain:` keys in one `with:` -> FAIL", fail(&tree(&dup), "more than one `toolchain:`"));

    // (c)
    for v in ["stable", "nightly", "beta", "1.100", "nightly-2026-9-17", "${{ matrix.rust }}"] {
        let wf = GOOD_WF.replace("toolchain: 1.100.0", &format!("toolchain: {v}"));
        ck(&format!("plant (c): toolchain: {v} -> FAIL"), fail(&tree(&wf), "(c)"));
    }
    let flow_bad = GOOD_WF.replace("        with:\n          toolchain: 1.100.0", "        with: {toolchain: stable}");
    ck("plant (c): flow `with: {toolchain: stable}` -> FAIL", fail(&tree(&flow_bad), "(c)"));

    // (d), including the forms that previously evaded the scan
    for (what, from, to) in [
        ("@stable", "uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca", "uses: dtolnay/rust-toolchain@stable"),
        ("@v1", "uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca", "uses: dtolnay/rust-toolchain@v1"),
        ("short SHA", "uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca", "uses: dtolnay/rust-toolchain@39008827"),
        ("quoted key `\"uses\":`", "- uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca", "- \"uses\": dtolnay/rust-toolchain@stable"),
        ("space before colon `uses :`", "- uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca", "- uses : dtolnay/rust-toolchain@stable"),
        ("mixed case `DTolnay/Rust-Toolchain@stable`", "uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca", "uses: DTolnay/Rust-Toolchain@stable"),
        ("sub-path `rust-toolchain/@stable`", "uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca", "uses: dtolnay/rust-toolchain/@stable"),
    ] {
        ck(&format!("plant (d): {what} -> FAIL"), fail(&tree(&GOOD_WF.replacen(from, to, 1)), "(d)"));
    }

    // (e)
    let lie = GOOD_WF.replace("# installs 1.100.0 (stable-branch SHA)", "# nightly");
    ck("plant (e): `# nightly` on a step that installs 1.100.0 -> FAIL", fail(&tree(&lie), "(e)"));
    let lie2 = GOOD_WF.replace("# installs 1.100.0 (stable-branch SHA)", "# installs nightly, never 1.100.0");
    ck("plant (e): comment names 1.100.0 AND nightly -> FAIL", fail(&tree(&lie2), "(e)"));
    let lie3 = GOOD_WF.replace("# installs 1.100.0 (stable-branch SHA)", "# stable branch, installs 1.100.0");
    ck("plant (e): `# stable branch, installs 1.100.0` (a second toolchain-like word) -> FAIL", fail(&tree(&lie3), "(e)"));

    // (f)
    let drift = GOOD_WF.replace("installs 1.100.0 (stable-branch SHA)\n        with:\n          toolchain: 1.100.0", "installs 1.99.1 (stable-branch SHA)\n        with:\n          toolchain: 1.99.1");
    ck("plant (f): a step installing 1.99.1 under a 1.100.0 channel -> FAIL", fail(&tree(&drift), "(f)"));

    // (g): one plant per shell form
    for run in [
        "cargo +nightly fuzz",
        "\"cargo +nightly test\"",
        "~/.cargo/bin/cargo +nightly test",
        "echo $(cargo +nightly --version)",
        "$CARGO +nightly test",
        "cd x;cargo +nightly test",
        "rustc +nightly --version",
        "rustfmt +nightly --check x.rs",
        "cargo-clippy +beta",
        "rustup +nightly target add wasm32-wasip2",
        "rustup override set nightly && cargo test",
        "rustup update stable",
        "rustup default stable",
        "rustup install nightly",
        "rustup run nightly cargo test",
        "rustup toolchain install nightly",
        "rustup toolchain install 1.100.0 nightly",
        "rustup toolchain add nightly",
        "rustup toolchain install --profile minimal nightly",
        "rustup component add rust-src --toolchain nightly",
        "rustup target add --toolchain=stable wasm32-wasip2",
        "sh rustup-init.sh -y --default-toolchain stable",
        "sh rustup-init.sh -y --default-toolchain=stable",
        "sh rustup-init.sh -y --default-toolchain \"$TC\"",
        "sh /tmp/rustup-init.sh -y",
        "curl https://sh.rustup.rs -sSf | sh -s -- -y",
        "RUSTUP_TOOLCHAIN=nightly cargo test",
        "cargo +${{ matrix.tc }} test",
    ] {
        let wf = with_steps(&format!("      - run: {run}\n"));
        ck(&format!("plant (g): `{run}` -> FAIL"), fail(&tree(&wf), "(g)"));
    }
    let cont = with_steps("      - run: |\n          cargo \\\n            +nightly test\n");
    ck("plant (g): `cargo \\` + newline `+nightly` -> FAIL", fail(&tree(&cont), "(g)"));

    // (h), (i)
    let env = with_steps("      - run: cargo test\n        env:\n          RUSTUP_TOOLCHAIN: stable\n");
    ck("plant (h): `env: RUSTUP_TOOLCHAIN: stable` -> FAIL", fail(&tree(&env), "(h)"));
    let other = with_steps("      - uses: actions-rust-lang/setup-rust-toolchain@v1\n        with:\n          toolchain: stable\n");
    ck("plant (h): another install action's `toolchain: stable` -> FAIL", fail(&tree(&other), "(h)"));
    let reusable = format!("{GOOD_WF}  b:\n    uses: someorg/somerepo/.github/workflows/rust.yml@main\n    with:\n      toolchain: stable\n");
    ck("plant (h): a reusable workflow's `toolchain: stable` input -> FAIL", fail(&tree(&reusable), "(h)"));
    let img = format!("{GOOD_WF}    container:\n      image: rust:latest\n");
    ck("plant (i): `image: rust:latest` -> FAIL", fail(&tree(&img), "(i)"));

    // Composite actions are scanned.
    let mut act = tree(GOOD_WF);
    act.files.push((
        ".github/actions/setup/action.yml".into(),
        "name: s\nruns:\n  using: composite\n  steps:\n    - uses: dtolnay/rust-toolchain@stable\n    - run: cargo +nightly test\n      shell: bash\n".into(),
    ));
    let v = check(&act);
    ck(
        "plant: a composite action under .github/actions/ with @stable and `cargo +nightly` -> FAIL on both",
        fails_with(&v, "(d) .github/actions/setup/action.yml") && fails_with(&v, "(g) .github/actions/setup/action.yml"),
    );

    // Refusals: the gate must not pass when it could not look.
    ck("refusal: zero workflows -> REFUSED", refused(&Tree { files: vec![], ..tree(GOOD_WF) }));
    ck("refusal: no step uses the action anywhere -> REFUSED", refused(&tree("on: push\njobs:\n  a:\n    steps:\n      - run: cargo test\n")));
    for (what, extra) in [
        ("flow-map step `- { uses: ... }`", "      - { uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca }\n"),
        ("block scalar `uses: >-`", "      - uses: >-\n          dtolnay/rust-toolchain@stable\n"),
        ("alias `uses: *act`", "      - uses: *act\n"),
    ] {
        let wf = if what.starts_with("alias") {
            format!("on: push\nenv:\n  X: &act dtolnay/rust-toolchain@stable\n{}{extra}", &GOOD_WF["on: push\n".len()..])
        } else {
            with_steps(extra)
        };
        ck(&format!("refusal: {what} -> REFUSED"), refused(&tree(&wf)));
    }

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}
