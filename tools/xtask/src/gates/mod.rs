//! The gate registry. Adding a gate means adding one line here; `xtask check`
//! with no argument lists what exists, so a gate cannot be added and then
//! invoked by nothing -- the failure mode gale#368 was opened to fix.

pub mod wit_resolve;

use crate::verdict::Verdict;
use std::path::Path;

pub struct Gate {
    pub name: &'static str,
    pub about: &'static str,
    pub run: fn(&Path) -> Verdict,
    pub self_test: fn(&Path) -> Verdict,
}

pub const GATES: &[Gate] = &[Gate {
    name: wit_resolve::NAME,
    about: "generated WIT worlds resolve against the gust:hal seam",
    run: wit_resolve::run,
    self_test: wit_resolve::self_test,
}];

pub fn find(name: &str) -> Option<&'static Gate> {
    GATES.iter().find(|g| g.name == name)
}
