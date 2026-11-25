//! The gate registry. Adding a gate means adding one line here; `xtask check`
//! with no argument lists what exists, so a gate cannot be added and then
//! invoked by nothing -- the failure mode gale#368 was opened to fix.

pub mod aadl_parse;
pub mod armv6m_isa;
pub mod axioms_audit;
pub mod binary_ledger;
pub mod criticality_axis;
pub mod data_overlap;
pub mod graph_env;
pub mod gust_target_gen_wasm;
pub mod job_timeouts;
pub mod kani_coverage;
pub mod lex;
pub mod lockfile_audit;
pub mod lockfile_census;
pub mod moved_paths;
pub mod object_freshness;
pub mod peripheral_addresses;
pub mod pin_agreement;
pub mod proof_completeness;
pub mod providers;
pub mod renode_pin;
pub mod renode_targets;
pub mod retry_loops;
pub mod sieve_pin;
pub mod target_reference;
pub mod toolchain_pin;
pub mod trust_ledger;
pub mod trusted_base;
pub mod vacuous_lemmas;
pub mod verus_strip_wasm;
pub mod wasi_component;
pub mod wit_resolve;
pub mod yaml;
pub mod zephyr_module_archive;

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
        name: aadl_parse::NAME,
        about: "every committed AADL model parses with the pinned spar (gale#441)",
        run: aadl_parse::run,
        self_test: aadl_parse::self_test,
    },
    Gate {
        name: armv6m_isa::NAME,
        about: "no image built for an ARMv6-M board carries code ARMv6-M cannot execute",
        run: armv6m_isa::run,
        self_test: armv6m_isa::self_test,
    },
    Gate {
        name: axioms_audit::NAME,
        about: "Rocq and Lean axioms, read from the compiled objects, equal the committed ledger (REQ-HONESTY-AXIOMS-001)",
        run: axioms_audit::run,
        self_test: axioms_audit::self_test,
    },
    Gate {
        name: binary_ledger::NAME,
        about: "every committed binary has a recorded reason and a live consumer",
        run: binary_ledger::run,
        self_test: binary_ledger::self_test,
    },
    Gate {
        name: criticality_axis::NAME,
        about: "every coexisting pair of elements has a freedom-from-interference argument",
        run: criticality_axis::run,
        self_test: criticality_axis::self_test,
    },
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
        name: gust_target_gen_wasm::NAME,
        about: "gust-target-gen as a WASI component writes the same files as native on every board (needs --native --wasm --runtime)",
        run: gust_target_gen_wasm::run,
        self_test: gust_target_gen_wasm::self_test,
    },
    Gate {
        name: job_timeouts::NAME,
        about: "every CI job declares a timeout-minutes",
        run: job_timeouts::run,
        self_test: job_timeouts::self_test,
    },
    Gate {
        name: kani_coverage::NAME,
        about: "every crate with a #[kani::proof] harness is run by a CI cargo kani that can fail",
        run: kani_coverage::run,
        self_test: kani_coverage::self_test,
    },
    Gate {
        name: lockfile_audit::NAME,
        about: "every committed Cargo.lock is audited for RustSec vulnerabilities (gale#534)",
        run: lockfile_audit::run,
        self_test: lockfile_audit::self_test,
    },
    Gate {
        name: lockfile_census::NAME,
        about: "every Cargo.toml and MODULE.bazel has a committed, current lock, or a listed reason (gale#534)",
        run: lockfile_census::run,
        self_test: lockfile_census::self_test,
    },
    Gate {
        name: object_freshness::NAME,
        about: "each committed object matches its recorded hash, inputs and toolchain (gale#506)",
        run: object_freshness::run,
        self_test: object_freshness::self_test,
    },
    Gate {
        name: peripheral_addresses::NAME,
        about: "every peripheral address the code uses comes from the AADL target model",
        run: peripheral_addresses::run,
        self_test: peripheral_addresses::self_test,
    },
    Gate {
        name: pin_agreement::NAME,
        about: "a tool version hardcoded in a workflow matches what varve.toml resolves",
        run: pin_agreement::run,
        self_test: pin_agreement::self_test,
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
        name: renode_pin::NAME,
        about: "every Renode gale runs is the one pinned Renode (module, action, workflows agree)",
        run: renode_pin::run,
        self_test: renode_pin::self_test,
    },
    Gate {
        name: renode_targets::NAME,
        about: "every renode_test target defined is actually run by CI",
        run: renode_targets::run,
        self_test: renode_targets::self_test,
    },
    Gate {
        name: retry_loops::NAME,
        about: "no CI retry loop can swallow its own failure",
        run: retry_loops::run,
        self_test: retry_loops::self_test,
    },
    Gate {
        name: sieve_pin::NAME,
        about: "every sieve CI runs is the one pinned sieve, and the pin agrees with itself",
        run: sieve_pin::run,
        self_test: sieve_pin::self_test,
    },
    Gate {
        name: target_reference::NAME,
        about: "every modelled peripheral base equals the pinned reference table (targets/reference)",
        run: target_reference::run,
        self_test: target_reference::self_test,
    },
    Gate {
        name: toolchain_pin::NAME,
        about: "rust-toolchain.toml and every CI toolchain name an exact or dated Rust, on a SHA-pinned action",
        run: toolchain_pin::run,
        self_test: toolchain_pin::self_test,
    },
    Gate {
        name: trust_ledger::NAME,
        about: "assumption spellings per file equal the committed ledger (v0.11.0 bet 3)",
        run: trust_ledger::run,
        self_test: trust_ledger::self_test,
    },
    Gate {
        name: trusted_base::NAME,
        about: "the honesty doc's trusted-base figures equal the source's (gale#467)",
        run: trusted_base::run,
        self_test: trusted_base::self_test,
    },
    Gate {
        name: vacuous_lemmas::NAME,
        about: "no proof fn proves nothing (ratchet over a ledger, gale#462)",
        run: vacuous_lemmas::run,
        self_test: vacuous_lemmas::self_test,
    },
    Gate {
        name: verus_strip_wasm::NAME,
        about: "verus-strip as a WASI component is byte-identical to native (needs --native --wasm --runtime)",
        run: verus_strip_wasm::run,
        self_test: verus_strip_wasm::self_test,
    },
    Gate {
        name: wit_resolve::NAME,
        about: "generated WIT worlds resolve against the gust:hal seam",
        run: wit_resolve::run,
        self_test: wit_resolve::self_test,
    },
    Gate {
        name: moved_paths::NAME,
        about: "a directory moved in the rework leaves no live reference to its old path",
        run: moved_paths::run,
        self_test: moved_paths::self_test,
    },
    Gate {
        name: zephyr_module_archive::NAME,
        about: "the Zephyr module archive is reproducible, self-contained, and named",
        run: zephyr_module_archive::run,
        self_test: zephyr_module_archive::self_test,
    },
];

pub fn find(name: &str) -> Option<&'static Gate> {
    GATES.iter().find(|g| g.name == name)
}

/// Commit time (unix seconds) of the last commit that changed `relpath`'s
/// CONTENT, following renames. A pure rename (similarity 100) is skipped: moving
/// a file does not make it newer, and dating by "last commit touching the path"
/// made every input look freshly changed on the day its directory moved (the
/// rework's step 2b moved drivers/ and would have reddened every freshness gate).
/// A rename WITH edits still counts, because its content changed.
pub(crate) fn last_content_epoch(repo: &std::path::Path, relpath: &str) -> Option<i64> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["log", "--follow", "-M", "--format=%x01%ct", "--name-status", "--", relpath])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    for entry in text.split('\x01').filter(|e| !e.trim().is_empty()) {
        let mut lines = entry.lines();
        let Some(ct) = lines.next().and_then(|l| l.trim().parse::<i64>().ok()) else { continue };
        let statuses: Vec<&str> = lines.filter(|l| !l.trim().is_empty()).collect();
        // A merge commit lists no paths; the commits that brought the content
        // are listed on their own.
        if statuses.is_empty() {
            continue;
        }
        if statuses.iter().all(|s| s.starts_with("R100")) {
            continue;
        }
        return Some(ct);
    }
    None
}
