//! gust-target-gen — generate per-target constants, linker memory maps, and
//! gust:hal WIT worlds from a spar/AADL hardware model.
//!
//!   gust-target-gen --items <spar-items.json> --board <Pkg::Type.impl> --out <dir>
//!
//! Reads `spar items --format json` output, resolves the named board system
//! implementation, and writes three DO-NOT-EDIT artifacts into <dir>:
//!   gust_target_<stem>.rs   memory-<stem>.x   world-<stem>.wit
//! where <stem> is the board's package name lowercased (e.g. stm32f100).
//! Build-time only — nothing here runs on the target.

mod emit_ld;
mod emit_rs;
mod emit_wit;
mod model;

use std::path::PathBuf;

/// First value following `flag` in argv, if present.
fn arg(flag: &str) -> Option<String> {
    let mut it = std::env::args();
    while let Some(a) = it.next() {
        if a == flag {
            return it.next();
        }
    }
    None
}

fn main() {
    let items = arg("--items").expect("gust-target-gen: --items <spar-items.json> is required");
    let board = arg("--board").expect("gust-target-gen: --board <Pkg::Type.impl> is required");
    let out = arg("--out").expect("gust-target-gen: --out <dir> is required");

    let json = std::fs::read_to_string(&items)
        .unwrap_or_else(|e| panic!("gust-target-gen: cannot read --items `{items}`: {e}"));
    let target = model::parse_items(&json, &board);
    let stem = board.split("::").next().unwrap_or(&board).to_lowercase();

    let out = PathBuf::from(&out);
    std::fs::create_dir_all(&out).expect("gust-target-gen: cannot create --out dir");

    let write = |name: String, contents: String| {
        let p = out.join(&name);
        std::fs::write(&p, contents)
            .unwrap_or_else(|e| panic!("gust-target-gen: cannot write {}: {e}", p.display()));
        eprintln!("gust-target-gen: wrote {}", p.display());
    };

    write(format!("gust_target_{stem}.rs"), emit_rs::emit_rs(&target));
    write(format!("memory-{stem}.x"), emit_ld::emit_ld(&target));
    write(format!("world-{stem}.wit"), emit_wit::emit_wit(&target));
}
