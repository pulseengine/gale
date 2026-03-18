//! CI gate: verify plain/src/ matches verus-strip output from src/.
//! Fails if any file diverges — means src/ was edited without regenerating plain/.
//!
//! Run via: bazel test //tools/verus-strip:gate_test
//! Or:      cargo test --manifest-path tools/verus-strip/Cargo.toml --test gate

use std::fs;
use std::path::Path;

const FILES: &[&str] = &[
    "error", "priority", "thread", "wait_queue", "sem", "mutex",
    "condvar", "msgq", "pipe", "stack", "fifo", "lifo", "timer",
    "event", "mem_slab", "queue", "futex", "mbox", "timeout", "poll",
    "sched", "thread_lifecycle", "timeslice", "heap", "kheap", "work",
    "fatal", "fault_decode", "mempool", "dynamic", "smp_state",
    "stack_config", "device_init", "mem_domain", "spinlock", "atomic",
    "userspace", "ring_buf", "lib",
];

fn find_gale_root() -> &'static Path {
    // Walk up from the test binary location to find the gale root
    // (contains src/ and plain/src/)
    let candidates = [
        Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap(),
        Path::new("."),
        Path::new("../.."),
    ];
    for p in candidates {
        if p.join("src").is_dir() && p.join("plain/src").is_dir() {
            return Box::leak(p.to_path_buf().into_boxed_path());
        }
    }
    panic!("Cannot find gale root (need src/ and plain/src/ directories)");
}

#[test]
fn plain_matches_stripped_src() {
    let root = find_gale_root();
    let mut failures = Vec::new();

    for name in FILES {
        let src_path = root.join(format!("src/{name}.rs"));
        let plain_path = root.join(format!("plain/src/{name}.rs"));

        let src_content = fs::read_to_string(&src_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {e}", src_path.display()));
        let plain_content = fs::read_to_string(&plain_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {e}", plain_path.display()));

        let stripped = verus_strip::strip_file(&src_content);

        if stripped.output != plain_content {
            // Find first differing line for diagnostics
            let stripped_lines: Vec<&str> = stripped.output.lines().collect();
            let plain_lines: Vec<&str> = plain_content.lines().collect();
            let first_diff = stripped_lines.iter().zip(plain_lines.iter())
                .enumerate()
                .find(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| format!("  line {}: stripped={:?} vs plain={:?}", i + 1, a, b))
                .unwrap_or_else(|| {
                    format!("  length: stripped={} vs plain={} lines",
                        stripped_lines.len(), plain_lines.len())
                });
            failures.push(format!("{name}.rs:\n{first_diff}"));
        }
    }

    if !failures.is_empty() {
        panic!(
            "plain/src/ is out of sync with src/. Regenerate with:\n\
             for f in {}; do verus-strip src/$f.rs -o plain/src/$f.rs; done\n\n\
             Divergences:\n{}",
            FILES.join(" "),
            failures.join("\n"),
        );
    }
}

/// Gate for standalone Rocq files (plain/*.rs) — generated with --standalone mode.
/// These are used directly by rocq_of_rust for theorem proving.
const STANDALONE_FILES: &[&str] = &[
    "sem", "mutex", "condvar", "msgq", "stack", "pipe", "timer", "event", "mem_slab",
];

#[test]
fn plain_standalone_matches_stripped_standalone() {
    let root = find_gale_root();
    let mut failures = Vec::new();

    for name in STANDALONE_FILES {
        let src_path = root.join(format!("src/{name}.rs"));
        let plain_path = root.join(format!("plain/{name}.rs"));

        let src_content = fs::read_to_string(&src_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {e}", src_path.display()));
        let plain_content = fs::read_to_string(&plain_path)
            .unwrap_or_else(|e| panic!("Cannot read {}: {e}", plain_path.display()));

        let stripped = verus_strip::strip_file(&src_content);
        let standalone = verus_strip::make_standalone(&stripped.output);

        if standalone != plain_content {
            let s_lines: Vec<&str> = standalone.lines().collect();
            let p_lines: Vec<&str> = plain_content.lines().collect();
            let first_diff = s_lines.iter().zip(p_lines.iter())
                .enumerate()
                .find(|(_, (a, b))| a != b)
                .map(|(i, (a, b))| format!("  line {}: stripped={:?} vs plain={:?}", i + 1, a, b))
                .unwrap_or_else(|| {
                    format!("  length: stripped={} vs plain={} lines",
                        s_lines.len(), p_lines.len())
                });
            failures.push(format!("{name}.rs:\n{first_diff}"));
        }
    }

    if !failures.is_empty() {
        panic!(
            "plain/ standalone files are out of sync. Regenerate with:\n\
             for f in {}; do verus-strip --standalone src/$f.rs -o plain/$f.rs; done\n\n\
             Divergences:\n{}",
            STANDALONE_FILES.join(" "),
            failures.join("\n"),
        );
    }
}
