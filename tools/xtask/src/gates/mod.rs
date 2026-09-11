//! The gate registry. Adding a gate means adding one line here; `xtask check`
//! with no argument lists what exists, so a gate cannot be added and then
//! invoked by nothing -- the failure mode gale#368 was opened to fix.

pub mod data_overlap;
pub mod graph_env;
pub mod object_freshness;
pub mod proof_completeness;
pub mod providers;
pub mod retry_loops;
pub mod wit_resolve;

use crate::verdict::Verdict;
use std::path::Path;

pub struct Gate {
    pub name: &'static str,
    pub about: &'static str,
    pub run: fn(&Path, Option<&str>) -> Verdict,
    pub self_test: fn(&Path, Option<&str>) -> Verdict,
}

pub const GATES: &[Gate] = &[
    Gate {
        name: data_overlap::NAME,
        about: "the fused core module's data segments are disjoint",
        run: data_overlap::run,
        self_test: data_overlap::self_test,
    },
    Gate {
        name: graph_env::NAME,
        about: "no raw `env` import survives in the composed graph (VER-DRV-GRAPH-001)",
        run: graph_env::run,
        self_test: graph_env::self_test,
    },
    Gate {
        name: object_freshness::NAME,
        about: "no committed object is older than the sources that produce it",
        run: object_freshness::run,
        self_test: object_freshness::self_test,
    },
    Gate {
        name: proof_completeness::NAME,
        about: "a Rocq proof that is merely STATED cannot pass as proven",
        run: proof_completeness::run,
        self_test: proof_completeness::self_test,
    },
    Gate {
        name: providers::NAME,
        about: "every gust:os provider still compiles against the current WIT",
        run: providers::run,
        self_test: providers::self_test,
    },
    Gate {
        name: retry_loops::NAME,
        about: "no CI retry loop can swallow its own failure",
        run: retry_loops::run,
        self_test: retry_loops::self_test,
    },
    Gate {
        name: wit_resolve::NAME,
        about: "generated WIT worlds resolve against the gust:hal seam",
        run: wit_resolve::run,
        self_test: wit_resolve::self_test,
    },
];

pub fn find(name: &str) -> Option<&'static Gate> {
    GATES.iter().find(|g| g.name == name)
}
