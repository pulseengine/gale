//! Every Rust toolchain CI installs is an exact, named release.
//!
//! Fleet rule (2026-11): the repo names one exact stable Rust in
//! `rust-toolchain.toml`, and every workflow names it too, on a SHA-pinned
//! toolchain action. A floating `stable` is an input that moves under a fixed
//! pin set: 1.99.0 changed which gust:os providers keep a `__stack_pointer`
//! global (gale#434) with no commit in this repo. A floating `nightly` is worse:
//! it moves daily.
//!
//! Before this gate, 24 of the 32 `dtolnay/rust-toolchain` steps named no
//! `toolchain:` and took the action branch's default, and coverage.yml's step
//! said `# nightly` on a SHA from the action's `stable` branch -- it had been
//! installing stable all along.
//!
//! What the gate requires:
//!   (a) `rust-toolchain.toml`'s `channel` is an exact `X.Y.Z`;
//!   (b) every `dtolnay/rust-toolchain` step has a `with: toolchain:`;
//!   (c) that value is an exact `X.Y.Z` or a DATED `<channel>-YYYY-MM-DD`, never
//!       a bare `stable` / `nightly` / `beta` (or `X.Y`, which floats the patch);
//!   (d) the action ref is a 40-hex commit SHA, not a branch or tag;
//!   (e) a trailing comment on the `uses:` line, if any, names that toolchain --
//!       so a comment cannot claim a toolchain the step does not install;
//!   (f) an exact `X.Y.Z` there equals the `rust-toolchain.toml` channel (a step
//!       installing targets onto a different stable than the one cargo then
//!       selects is how a job ends in "target not installed");
//!   (g) outside comments, `cargo +<tc>`, `rustup toolchain install <tc>`,
//!       `rustup default <tc>` and `--default-toolchain <tc>` obey (c) too.
//!
//! The workflows are hand-scanned by indentation like job_timeouts.rs. The
//! scanner REFUSES when it cannot answer: no workflows at all, or a workflow
//! that mentions the action more often than it found steps for it.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "toolchain-pin";

const ACTION: &str = "dtolnay/rust-toolchain";

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

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"').trim_matches('\'')
}

/// The value of a `key: value` line with any trailing ` # comment` removed.
fn value_of(t: &str) -> &str {
    let v = t.split_once(':').map(|(_, v)| v).unwrap_or("");
    let v = match v.find(" #") {
        Some(i) => &v[..i],
        None => v,
    };
    unquote(v)
}

/// The `channel` under `[toolchain]` in rust-toolchain.toml.
fn channel(text: &str) -> Option<String> {
    let mut in_tc = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_tc = t == "[toolchain]";
            continue;
        }
        if in_tc && t.starts_with("channel") && t.contains('=') {
            let v = t.split_once('=').unwrap().1;
            let v = v.split('#').next().unwrap_or("");
            return Some(unquote(v).to_string());
        }
    }
    None
}

#[derive(Debug)]
struct Step {
    line: usize,
    sha: String,
    comment: Option<String>,
    toolchain: Option<String>,
}

/// Every `dtolnay/rust-toolchain` step in one workflow. A step's keys sit at
/// the column of its `uses:` key (after `- ` when `uses:` opens the step); the
/// step ends at the next line at that column starting `- `, or any line left of
/// it. `toolchain:` counts only inside that step's own `with:` block.
fn steps(text: &str) -> Vec<Step> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        let indent = line.len() - t.len();
        let (key_col, rest) = match t.strip_prefix("- ") {
            Some(r) => (indent + 2, r.trim_start()),
            None => (indent, t),
        };
        let Some(u) = rest.strip_prefix("uses:") else {
            i += 1;
            continue;
        };
        let (target, comment) = match u.find('#') {
            Some(h) => (u[..h].trim(), Some(u[h + 1..].trim().to_string())),
            None => (u.trim(), None),
        };
        let target = unquote(target);
        let Some(sha) = target.strip_prefix(ACTION).and_then(|r| r.strip_prefix('@')) else {
            i += 1;
            continue;
        };
        let mut step = Step { line: i + 1, sha: sha.to_string(), comment, toolchain: None };
        let mut in_with = false;
        let mut j = i + 1;
        while j < lines.len() {
            let l = lines[j];
            let lt = l.trim_start();
            if lt.is_empty() || lt.starts_with('#') {
                j += 1;
                continue;
            }
            let li = l.len() - lt.len();
            if li < key_col || (li + 2 == key_col && lt.starts_with("- ")) || (li == key_col && lt.starts_with("- ")) {
                break;
            }
            if li == key_col {
                in_with = lt.trim_end() == "with:";
            } else if in_with && li == key_col + 2 && lt.starts_with("toolchain:") {
                step.toolchain = Some(value_of(lt).to_string());
            }
            j += 1;
        }
        out.push(step);
        i = j.max(i + 1);
    }
    out
}

/// (g): toolchain names given to cargo/rustup on non-comment lines.
fn invocations(text: &str) -> Vec<(usize, String, String)> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if t.starts_with('#') {
            continue;
        }
        let words: Vec<&str> = t.split_whitespace().collect();
        for w in 0..words.len() {
            let next = words.get(w + 1).map(|s| unquote(s).to_string());
            let found = match (words[w], words.get(w + 1).copied()) {
                ("cargo", Some(x)) if x.starts_with('+') => {
                    Some(("cargo +".to_string(), unquote(&x[1..]).to_string()))
                }
                ("--default-toolchain", Some(_)) => Some(("--default-toolchain".to_string(), next.unwrap())),
                ("rustup", Some("default")) => words.get(w + 2).map(|x| ("rustup default".to_string(), unquote(x).to_string())),
                ("rustup", Some("toolchain")) if words.get(w + 2) == Some(&"install") => {
                    words.get(w + 3).map(|x| ("rustup toolchain install".to_string(), unquote(x).to_string()))
                }
                _ => None,
            };
            if let Some((what, tc)) = found {
                if what == "--default-toolchain" && tc == "none" {
                    continue;
                }
                out.push((n + 1, what, tc));
            }
        }
    }
    out
}

/// The whole check over in-memory inputs, so the self-test runs the same code
/// the real tree does.
fn check(toolchain_file: Option<&str>, workflows: &[(String, String)]) -> Verdict {
    if workflows.is_empty() {
        return Verdict::Refused("no workflows found -- the sweep would be vacuous".into());
    }
    let mut bad = Vec::new();
    let chan = match toolchain_file {
        None => {
            bad.push("(a) rust-toolchain.toml: missing -- the build floats on whatever rustup defaults to".into());
            None
        }
        Some(text) => match channel(text) {
            None => {
                bad.push("(a) rust-toolchain.toml: no `channel` under [toolchain]".into());
                None
            }
            Some(c) if !is_exact_release(&c) => {
                bad.push(format!("(a) rust-toolchain.toml: channel `{c}` is not an exact X.Y.Z"));
                None
            }
            Some(c) => Some(c),
        },
    };

    let (mut nsteps, mut nexact, mut ndated, mut ninv) = (0usize, 0usize, 0usize, 0usize);
    for (name, text) in workflows {
        let found = steps(text);
        let mentions = text
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .filter(|l| l.contains(&format!("{ACTION}@")) && l.contains("uses:"))
            .count();
        if mentions != found.len() {
            return Verdict::Refused(format!(
                "{name}: {mentions} `uses: {ACTION}@` line(s) but the scanner parsed {} step(s) -- \
                 a layout it does not understand, so it cannot answer",
                found.len()
            ));
        }
        for s in &found {
            nsteps += 1;
            let at = format!("{name}:{}", s.line);
            if !(s.sha.len() == 40 && s.sha.bytes().all(|b| b.is_ascii_hexdigit())) {
                bad.push(format!("(d) {at}: ref `{}` is not a 40-hex commit SHA", s.sha));
            }
            let Some(tc) = &s.toolchain else {
                bad.push(format!("(b) {at}: no `with: toolchain:` -- installs whatever the action branch defaults to"));
                continue;
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
                if !cm.split(|c: char| c.is_whitespace() || c == ',' || c == ';' || c == '(' || c == ')').any(|w| w == tc) {
                    bad.push(format!("(e) {at}: comment `# {cm}` does not name the toolchain it installs (`{tc}`)"));
                }
            }
        }
        for (line, what, tc) in invocations(text) {
            ninv += 1;
            if !is_pinned(&tc) {
                bad.push(format!("(g) {name}:{line}: `{what}{}{tc}` is not an exact X.Y.Z or a dated nightly", if what.ends_with('+') { "" } else { " " }));
            }
        }
    }

    if !bad.is_empty() {
        let mut l = vec![format!("FAIL: {} toolchain(s) not pinned exactly:", bad.len())];
        l.extend(bad.into_iter().map(|b| format!("  {b}")));
        return Verdict::Fail(l);
    }
    Verdict::Pass(vec![format!(
        "rust-toolchain.toml channel {}; {nsteps} {ACTION} step(s) in {} workflow(s), all SHA-pinned \
         with an explicit toolchain ({nexact} exact release, {ndated} dated); {ninv} cargo +/rustup \
         toolchain name(s), all pinned.",
        chan.unwrap_or_default(),
        workflows.len()
    )])
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let tc = std::fs::read_to_string(repo.join("rust-toolchain.toml"))
        .or_else(|_| std::fs::read_to_string(repo.join("rust-toolchain")))
        .ok();
    let wf = repo.join(".github/workflows");
    let Ok(rd) = std::fs::read_dir(&wf) else {
        return Verdict::Refused(format!("cannot read {}", wf.display()));
    };
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yml" | "yaml")))
        .collect();
    files.sort();
    let mut wfs = Vec::new();
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(f) else {
            return Verdict::Refused(format!("cannot read {name}"));
        };
        wfs.push((name, text));
    }
    check(tc.as_deref(), &wfs)
}

const GOOD_TC: &str = "[toolchain]\nchannel = \"1.100.0\"\ncomponents = [\"clippy\"]\n";

/// A workflow covering each step layout the scanner must read correctly:
/// `- uses:` opening the step, `uses:` under `- name:`, an `if:` between
/// `uses:` and `with:`, `toolchain:` after other `with:` keys, and a comment.
const GOOD_WF: &str = "\
on: push
jobs:
  a:
    runs-on: x
    steps:
      - uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca # stable branch, installs 1.100.0
        with:
          toolchain: 1.100.0
      - name: Install nightly
        if: true
        uses: dtolnay/rust-toolchain@6a52aad76a94a5fbc50d567bb5be24419801c1fb # nightly branch, installs nightly-2026-09-17
        with:
          components: rust-src
          toolchain: nightly-2026-09-17
      - run: |
          # cargo +nightly in a comment is not an invocation
          cargo +nightly-2026-09-17 fuzz run x
          sh rustup-init.sh -y --default-toolchain 1.100.0
";

fn one(wf: &str) -> Vec<(String, String)> {
    vec![("w.yml".to_string(), wf.to_string())]
}

fn fails_with(v: &Verdict, needle: &str) -> bool {
    matches!(v, Verdict::Fail(l) if l.iter().any(|x| x.contains(needle)))
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
    let good = check(Some(GOOD_TC), &one(GOOD_WF));
    ck(
        "positive control: exact channel, two pinned steps (one dated nightly), pinned invocations -> PASS",
        matches!(&good, Verdict::Pass(l) if l.iter().any(|x| x.contains("2 dtolnay/rust-toolchain step(s)") && x.contains("2 cargo +/rustup"))),
    );

    // (a)
    for ch in ["stable", "nightly", "1.100"] {
        let tc = GOOD_TC.replace("1.100.0\"", &format!("{ch}\""));
        ck(&format!("plant (a): channel = \"{ch}\" -> FAIL"), fails_with(&check(Some(&tc), &one(GOOD_WF)), "(a)"));
    }
    ck("plant (a): no rust-toolchain.toml -> FAIL", fails_with(&check(None, &one(GOOD_WF)), "(a)"));

    // (b)
    let no_with = GOOD_WF.replace("        with:\n          toolchain: 1.100.0\n", "");
    ck("plant (b): a step with no `with:` at all -> FAIL", fails_with(&check(Some(GOOD_TC), &one(&no_with)), "(b) w.yml:6"));
    let other_keys = GOOD_WF.replace("          toolchain: nightly-2026-09-17\n", "");
    ck("plant (b): `with:` carrying only other keys -> FAIL", fails_with(&check(Some(GOOD_TC), &one(&other_keys)), "(b) w.yml:11"));
    let next_step = "\
jobs:
  a:
    steps:
      - uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca
      - uses: some/other@39008827e6b4319a349e9a42dfc90579dd25f1ca
        with:
          toolchain: 1.100.0
";
    ck(
        "plant (b): the NEXT step's `toolchain:` does not count for this one -> FAIL",
        fails_with(&check(Some(GOOD_TC), &one(next_step)), "(b) w.yml:4"),
    );
    let nested = GOOD_WF.replace("          toolchain: 1.100.0\n", "          x:\n            toolchain: 1.100.0\n");
    ck("plant (b): a `toolchain:` nested deeper than `with:` does not count -> FAIL", fails_with(&check(Some(GOOD_TC), &one(&nested)), "(b) w.yml:6"));

    // (c)
    for v in ["stable", "nightly", "beta", "1.100", "nightly-2026-9-17"] {
        let wf = GOOD_WF.replace("toolchain: 1.100.0", &format!("toolchain: {v}"));
        ck(&format!("plant (c): toolchain: {v} -> FAIL"), fails_with(&check(Some(GOOD_TC), &one(&wf)), "(c)"));
    }

    // (d)
    for r in ["stable", "v1", "39008827"] {
        let wf = GOOD_WF.replace("@39008827e6b4319a349e9a42dfc90579dd25f1ca", &format!("@{r}"));
        ck(&format!("plant (d): action ref @{r} -> FAIL"), fails_with(&check(Some(GOOD_TC), &one(&wf)), "(d)"));
    }

    // (e) -- the coverage.yml bug: `# nightly` on a step that installs stable.
    let lie = GOOD_WF.replace("# stable branch, installs 1.100.0", "# nightly");
    ck("plant (e): `# nightly` on a step that installs 1.100.0 -> FAIL", fails_with(&check(Some(GOOD_TC), &one(&lie)), "(e)"));

    // (f)
    let drift = GOOD_WF.replace("installs 1.100.0\n        with:\n          toolchain: 1.100.0", "installs 1.99.1\n        with:\n          toolchain: 1.99.1");
    ck("plant (f): a step installing 1.99.1 under a 1.100.0 channel -> FAIL", fails_with(&check(Some(GOOD_TC), &one(&drift)), "(f)"));

    // (g)
    for (from, to) in [
        ("cargo +nightly-2026-09-17 fuzz", "cargo +nightly fuzz"),
        ("--default-toolchain 1.100.0", "--default-toolchain stable"),
        ("--default-toolchain 1.100.0", "--default-toolchain 1.100.0\n          rustup toolchain install nightly"),
    ] {
        let wf = GOOD_WF.replace(from, to);
        ck(&format!("plant (g): `{}` -> FAIL", to.lines().last().unwrap().trim()), fails_with(&check(Some(GOOD_TC), &one(&wf)), "(g)"));
    }

    // Refusals: the gate must not pass when it could not look.
    ck("refusal: zero workflows -> REFUSED", matches!(check(Some(GOOD_TC), &[]), Verdict::Refused(_)));
    let flow = GOOD_WF.replace(
        "      - uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca # stable branch, installs 1.100.0\n        with:\n          toolchain: 1.100.0\n",
        "      - { uses: dtolnay/rust-toolchain@39008827e6b4319a349e9a42dfc90579dd25f1ca }\n",
    );
    ck("refusal: a step layout the scanner cannot parse -> REFUSED", matches!(check(Some(GOOD_TC), &one(&flow)), Verdict::Refused(_)));

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}
