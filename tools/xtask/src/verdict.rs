//! A gate's outcome, and the exit codes that carry it.
//!
//! The distinction shell could not express, and the reason this crate exists:
//! a gate that CANNOT RUN is not a gate that PASSED. In shell the two collapse
//! constantly -- `tool 2>/dev/null | grep -c x || true` yields 0 whether the
//! property holds or the tool is missing, and the caller cannot tell. Every
//! defect the sweep in gale#368 found was some form of that collapse.
//!
//! So `Refused` is a first-class outcome with its own exit code. CI treats it
//! as a failure (non-zero), but it says something different in the log: the
//! gate did not answer, rather than answered "no".

use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// The property was checked and holds.
    Pass(Vec<String>),
    /// The property was checked and does NOT hold.
    Fail(Vec<String>),
    /// The gate could not run: a tool is missing, an input is absent. NOT a pass.
    Refused(String),
}

/// 0 pass, 1 the property failed, 2 usage, 3 the gate could not run.
/// Distinct codes so a caller can branch on "wrong" vs "could not tell".
pub const EXIT_PASS: i32 = 0;
pub const EXIT_FAIL: i32 = 1;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_REFUSED: i32 = 3;

impl Verdict {
    pub fn code(&self) -> i32 {
        match self {
            Verdict::Pass(_) => EXIT_PASS,
            Verdict::Fail(_) => EXIT_FAIL,
            Verdict::Refused(_) => EXIT_REFUSED,
        }
    }


    fn label(&self) -> &'static str {
        match self {
            Verdict::Pass(_) => "pass",
            Verdict::Fail(_) => "fail",
            Verdict::Refused(_) => "refused",
        }
    }

    /// `--format json`. Hand-rolled: the shape is fixed and tiny, and a gate
    /// that pulls a serialization stack in has a longer cold-build than the
    /// check it performs.
    pub fn to_json(&self, gate: &str) -> String {
        let lines: Vec<String> = match self {
            Verdict::Pass(l) | Verdict::Fail(l) => l.clone(),
            Verdict::Refused(r) => vec![r.clone()],
        };
        let items: Vec<String> = lines.iter().map(|l| format!("\"{}\"", escape(l))).collect();
        format!(
            "{{\"gate\":\"{}\",\"verdict\":\"{}\",\"exit\":{},\"detail\":[{}]}}",
            escape(gate),
            self.label(),
            self.code(),
            items.join(",")
        )
    }
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

impl fmt::Display for Verdict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Verdict::Pass(l) => {
                for x in l {
                    writeln!(f, "  {x}")?;
                }
                write!(f, "PASS")
            }
            Verdict::Fail(l) => {
                for x in l {
                    writeln!(f, "  {x}")?;
                }
                write!(f, "FAIL")
            }
            Verdict::Refused(r) => write!(f, "REFUSED: {r}"),
        }
    }
}
