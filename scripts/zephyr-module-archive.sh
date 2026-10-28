#!/usr/bin/env bash
# Build gale's Zephyr module source as a deterministic archive (gale#463).
#
#   scripts/zephyr-module-archive.sh <version> <out-dir> [<rev>]
#
# Writes <out-dir>/gale-zephyr-module-<version>.tar.gz from <rev> (default HEAD).
# The release workflow runs this before SHA256SUMS, so the cosign signature over
# SHA256SUMS covers the archive, and it is a SLSA provenance subject.
#
# DETERMINISM comes from `git archive`, not from tar flags: entries are emitted in
# tree order, every mtime is the commit's timestamp, uid/gid are 0, and only
# tracked files are included. `gzip -n` drops the name and timestamp from the gzip
# header. Same <rev> => byte-identical output on any machine.
# `xtask check zephyr-module-archive` builds it twice and requires that.
#
# MODULE_PATHS is "exactly the module tree": zephyr/module.yml and what its build
# references, transitively, and nothing else. The xtask check extracts the
# archive and builds the module's crate from it, so a missing entry fails there
# rather than in a consumer's west build:
#   zephyr/               module.yml, CMakeLists.txt, Kconfig, the C shims
#   ffi/                  CMakeLists.txt runs cargo on ffi/Cargo.toml
#   Cargo.toml, plain/    ffi depends on gale = { path = ".." }, lib at plain/src
#   Cargo.lock(s)         locked builds
#   rust-toolchain.toml   the toolchain the build pins
#   benches/sem_bench.rs  Cargo.toml declares [[bench]] sem_bench; cargo refuses
#                         to parse the manifest without it (found by the check)
#   LICENSE
set -euo pipefail

MODULE_PATHS=(
  zephyr
  ffi/Cargo.toml ffi/Cargo.lock ffi/include ffi/src
  Cargo.toml Cargo.lock
  plain
  rust-toolchain.toml
  benches/sem_bench.rs
  LICENSE
)

version="${1:?usage: zephyr-module-archive.sh <version> <out-dir> [<rev>]}"
out_dir="${2:?usage: zephyr-module-archive.sh <version> <out-dir> [<rev>]}"
rev="${3:-HEAD}"

# A path that matches nothing would silently shrink the archive.
for p in "${MODULE_PATHS[@]}"; do
  if [ -z "$(git ls-tree -r --name-only "$rev" -- "$p" | head -1)" ]; then
    echo "::error::module path '$p' matches no tracked file at $rev" >&2
    exit 1
  fi
done

mkdir -p "$out_dir"
out="$out_dir/gale-zephyr-module-${version}.tar.gz"
git archive --format=tar --prefix="gale-zephyr-module-${version}/" "$rev" -- "${MODULE_PATHS[@]}" \
  | gzip -n -9 > "$out"
echo "$out"
