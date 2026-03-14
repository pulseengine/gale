//! CLI for stripping Verus annotations from Rust source files.
//!
//! Usage:
//!   verus-strip <input.rs>              # print stripped output to stdout
//!   verus-strip <input.rs> -o <out.rs>  # write to file
//!   verus-strip --check <dir>           # verify src/ strips to match plain/

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage:");
        eprintln!("  verus-strip <input.rs>              # strip to stdout");
        eprintln!("  verus-strip <input.rs> -o <out.rs>  # strip to file");
        eprintln!("  verus-strip --check <src/> <plain/> # verify convergence");
        process::exit(1);
    }

    if args[1] == "--check" {
        if args.len() < 4 {
            eprintln!("Usage: verus-strip --check <src-dir> <plain-dir>");
            process::exit(1);
        }
        let src_dir = Path::new(&args[2]);
        let plain_dir = Path::new(&args[3]);
        check_convergence(src_dir, plain_dir);
    } else {
        let input_path = Path::new(&args[1]);
        let input = fs::read_to_string(input_path).unwrap_or_else(|e| {
            eprintln!("Error reading {}: {}", input_path.display(), e);
            process::exit(1);
        });

        let result = verus_strip::strip_file(&input);
        let output = result.output;

        if args.len() >= 4 && args[2] == "-o" {
            let out_path = Path::new(&args[3]);
            fs::write(out_path, &output).unwrap_or_else(|e| {
                eprintln!("Error writing {}: {}", out_path.display(), e);
                process::exit(1);
            });
            eprintln!("Wrote {}", out_path.display());
        } else {
            print!("{}", output);
        }
    }
}

/// Check that stripping src/ produces output structurally matching plain/.
/// Reports differences per file.
fn check_convergence(src_dir: &Path, plain_dir: &Path) {
    let mut files_checked = 0;
    let mut files_diverged = 0;

    let mut src_files: Vec<PathBuf> = Vec::new();
    collect_rs_files(src_dir, &mut src_files);
    src_files.sort();

    for src_path in &src_files {
        let relative = src_path.strip_prefix(src_dir).unwrap();
        let plain_path = plain_dir.join(relative);

        if !plain_path.exists() {
            eprintln!("  SKIP  {} (no plain/ counterpart)", relative.display());
            continue;
        }

        let src_content = fs::read_to_string(src_path).unwrap();
        let plain_content = fs::read_to_string(&plain_path).unwrap();

        let stripped = verus_strip::strip_file(&src_content).output;

        // Compare stripped Verus output against plain file.
        // The plain file may have ADDITIONAL content (tests, derive macros,
        // trait impls) that the Verus source lacks. So we check that every
        // non-blank line from the stripped output appears in the plain file
        // in order, allowing gaps for the additions.
        let stripped_lines: Vec<&str> = stripped
            .lines()
            .filter(|l: &&str| !l.trim().is_empty())
            .collect();
        let plain_lines: Vec<&str> = plain_content
            .lines()
            .filter(|l: &&str| !l.trim().is_empty())
            .collect();

        // Check: all stripped lines should appear in plain, preserving order
        let mut plain_idx = 0;
        let mut missing: Vec<String> = Vec::new();

        for stripped_line in &stripped_lines {
            let needle = stripped_line.trim();
            if needle.is_empty() {
                continue;
            }

            // Search forward in plain lines
            let mut found = false;
            while plain_idx < plain_lines.len() {
                if plain_lines[plain_idx].trim() == needle {
                    plain_idx += 1;
                    found = true;
                    break;
                }
                plain_idx += 1;
            }

            if !found {
                missing.push(stripped_line.to_string());
            }
        }

        files_checked += 1;

        if missing.is_empty() {
            eprintln!("  OK    {}", relative.display());
        } else {
            files_diverged += 1;
            eprintln!("  DIFF  {} ({} lines not found in plain)", relative.display(), missing.len());
            for line in missing.iter().take(5) {
                eprintln!("        > {}", line.trim());
            }
            if missing.len() > 5 {
                eprintln!("        ... and {} more", missing.len() - 5);
            }
        }
    }

    eprintln!();
    eprintln!("Checked {} files: {} OK, {} diverged", files_checked, files_checked - files_diverged, files_diverged);

    if files_diverged > 0 {
        process::exit(1);
    }
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect_rs_files(&path, out);
            } else if path.extension().map_or(false, |e| e == "rs") {
                out.push(path);
            }
        }
    }
}
