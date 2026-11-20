#!/usr/bin/env bash
# Regenerate targets/generated/ from the AADL target models.
#
# The chain is ALWAYS: targets/*.aadl --(pinned spar)--> spar-items JSON
# (tools/gust-target-gen/tests/golden/f100.items.json, committed as the
# generator's golden input) --(gust-target-gen)--> targets/generated/.
#
# spar is the varve pin and nothing else: `varve which spar`, resolved in the
# repo so varve.toml selects the layer. There is NO fallback -- not to a bare
# `spar` on PATH (a stale ~/.cargo/bin build shadows the pin on dev machines)
# and not to the committed fixture. That fallback is what let CI "regenerate"
# from the committed JSON without ever reading the .aadl: moving the G031 IWDG
# base to 0x40004000 passed regen, the drift gate and every generator test.
# If the pin cannot be resolved this script FAILS.
#
# The generated files are COMMITTED and DO NOT EDIT them by hand.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"            # targets
REPO="$(cd "$HERE/.." && pwd)"                   # repo root
GEN="$HERE/generated"
FIXTURE="$REPO/tools/gust-target-gen/tests/golden/f100.items.json"
mkdir -p "$GEN"
cd "$REPO"   # varve reads the pin from this tree's varve.toml

die() { echo "regen: FAIL: $*" >&2; exit 1; }

command -v varve >/dev/null 2>&1 \
  || die "varve is not on PATH; the pinned spar cannot be resolved (install varve; there is no fallback)"
SPAR="$(varve which spar)" \
  || die "\`varve which spar\` failed; run \`varve install\` in $REPO (there is no fallback)"
[ -n "$SPAR" ] && [ -x "$SPAR" ] || die "\`varve which spar\` gave '$SPAR', which is not an executable"
# Captured, not piped into grep -q: an early-exiting reader would SIGPIPE spar
# and pipefail would turn a good binary into a failure (or worse, the reverse).
SPAR_VERSION="$("$SPAR" --version)" || die "$SPAR --version failed"
case "$SPAR_VERSION" in
  "spar "*) ;;
  *) die "$SPAR answers --version with '$SPAR_VERSION', not 'spar <semver>'" ;;
esac
echo "regen: $SPAR_VERSION ($SPAR) -- regenerating $FIXTURE from targets/*.aadl"

# Write to a temp file and move it into place, so a failing spar cannot leave a
# truncated fixture behind for the generator to consume.
# The TRACKED top-level models, the same list `cargo xtask check target-reference`
# feeds spar (git ls-files order), so a stray local .aadl cannot make the
# committed fixture look stale -- or fresh.
MODELS=()
while IFS= read -r m; do
  case "${m#targets/}" in */*) continue ;; esac
  MODELS+=("$m")
done < <(git ls-files -- 'targets/*.aadl')
[ ${#MODELS[@]} -gt 0 ] || die "no tracked targets/*.aadl"

TMP="$(mktemp "${FIXTURE}.XXXXXX")"
trap 'rm -f "$TMP"' EXIT
"$SPAR" items --format json "${MODELS[@]}" > "$TMP" || die "spar items rejected the models"
[ -s "$TMP" ] || die "spar items produced no output"
chmod 644 "$TMP"   # mktemp creates 0600; the committed fixture is 0644
mv "$TMP" "$FIXTURE"
trap - EXIT

# Run the HOST generator from the repo root. os/gust/.cargo/config.toml sets
# target = thumbv7m-none-eabi, so invoking cargo with the cwd anywhere under
# os/gust cross-compiles this host tool and dies in a dependency
# ("can't find crate", E0463). The script was cwd-sensitive and only worked when
# run from outside that tree.
cd "$REPO"

for BOARD in \
  "STM32F100::Board.vldiscovery" \
  "STM32G474::Board.nucleo" \
  "STM32WL55::Board.nucleo" \
  "STM32WB55::Board.nucleo" \
  "STM32G031::Board.nucleo"; do
  STEM="$(printf '%s' "${BOARD%%::*}" | tr '[:upper:]' '[:lower:]')"
  cargo run --locked --quiet --manifest-path "$REPO/tools/gust-target-gen/Cargo.toml" -- \
    --items "$FIXTURE" --board "$BOARD" --out "$GEN" \
    --reference "$REPO/targets/reference/$STEM.peripherals.tsv" \
    --board-reference "$REPO/targets/reference/$STEM.board.tsv"
done

echo "regen: generated/ is up to date."
