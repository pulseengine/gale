//! Integration test: verify that --standalone mode produces valid, self-contained Rust
//! for all 9 modules used by rocq_of_rust.
//!
//! This test:
//! 1. Strips each src/*.rs file with standalone stubs
//! 2. Verifies the output parses as valid Rust (via syn)
//! 3. Checks no `use crate::` imports remain

use std::fs;
use std::path::Path;

/// The 9 modules that need standalone files for rocq_of_rust proofs.
const STANDALONE_MODULES: &[&str] = &[
    "sem", "mutex", "condvar", "msgq", "stack", "pipe", "timer", "event", "mem_slab",
];

fn find_gale_root() -> &'static Path {
    let candidates = [
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap(),
        Path::new("."),
        Path::new("../.."),
    ];
    for p in candidates {
        if p.join("src").is_dir() && p.join("plain").is_dir() {
            return Box::leak(p.to_path_buf().into_boxed_path());
        }
    }
    panic!("Cannot find gale root (need src/ and plain/ directories)");
}

#[test]
fn standalone_files_parse_as_valid_rust() {
    let root = find_gale_root();
    let mut failures = Vec::new();

    for name in STANDALONE_MODULES {
        let src_path = root.join(format!("src/{name}.rs"));
        let src_content = fs::read_to_string(&src_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {e}", src_path.display()));

        let stripped = verus_strip::strip_file(&src_content);
        if !stripped.errors.is_empty() {
            failures.push(format!("{name}.rs: strip errors: {:?}", stripped.errors));
            continue;
        }

        let standalone = verus_strip::make_standalone(&stripped.output);

        // Check: no `use crate::` imports remain
        for (i, line) in standalone.lines().enumerate() {
            if line.trim().starts_with("use crate::") {
                failures.push(format!(
                    "{name}.rs: line {}: still has `use crate::` import: {}",
                    i + 1,
                    line.trim()
                ));
            }
        }

        // Check: parses as valid Rust
        match syn::parse_file(&standalone) {
            Ok(_) => {} // good
            Err(e) => {
                failures.push(format!("{name}.rs: syn parse error: {e}"));
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "Standalone files have issues:\n{}",
            failures.join("\n"),
        );
    }
}

#[test]
fn standalone_stubs_contain_required_types() {
    let root = find_gale_root();

    // Test that modules using Thread/WaitQueue get proper stubs
    for name in &["sem", "mutex", "condvar"] {
        let src_path = root.join(format!("src/{name}.rs"));
        let src_content = fs::read_to_string(&src_path).unwrap();
        let stripped = verus_strip::strip_file(&src_content);
        let standalone = verus_strip::make_standalone(&stripped.output);

        // These modules reference Thread, WaitQueue, ThreadId, ThreadState
        assert!(
            standalone.contains("pub struct Thread"),
            "{name}.rs: missing Thread struct in standalone output"
        );
        assert!(
            standalone.contains("pub struct WaitQueue"),
            "{name}.rs: missing WaitQueue struct in standalone output"
        );
        assert!(
            standalone.contains("pub struct ThreadId"),
            "{name}.rs: missing ThreadId struct in standalone output"
        );
        assert!(
            standalone.contains("pub enum ThreadState"),
            "{name}.rs: missing ThreadState enum in standalone output"
        );
        // WaitQueue methods needed by the module code
        assert!(
            standalone.contains("fn unpend_first"),
            "{name}.rs: missing WaitQueue::unpend_first in standalone output"
        );
        assert!(
            standalone.contains("fn pend"),
            "{name}.rs: missing WaitQueue::pend in standalone output"
        );
    }
}
