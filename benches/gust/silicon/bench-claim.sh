#!/usr/bin/env bash
# Shared-bench claim helper (gale side of jess#226 / gale#356).
#
# jess and gale drive the SAME physical probes. Two agents on one probe does not fail
# cleanly -- it fails as a corrupted session that reads like a hardware fault, which is
# the worst kind of evidence to have in a safety campaign. `with-device` (jess) holds an
# OS flock for exactly the wrapped command's lifetime, so a crashed holder cannot wedge
# the bench.
#
# Source this and call `claim <device> <purpose> -- <command...>`.
#
#   claim "$(probe_device)" "gale: REQ-OS-UNPRIV-001 g474" -- probe-rs run --chip ... elf
#
# TWO THINGS THIS ADDS OVER CALLING with-device DIRECTLY:
#
# 1. DEVICE NAMES COME FROM THE HOST'S REGISTRY, and must match it exactly. jess
#    registered the Pi's probe as `stlink-v1` -- deliberately not `stlink-v3`, which is a
#    different physical probe still on the Mac. If gale claimed `stlink-v1-f100` and jess
#    claimed `stlink-v1`, both would succeed and neither would exclude the other: a lock
#    whose name does not match the hardware is the vacuous-gate failure in a new costume.
#    with-device now refuses unregistered names outright (exit 2), which turns that class
#    of mistake from silent into loud.
#
#    For the LOCAL Mac probe the serial is still the right key. This host has had two
#    ST-LINKs attached at once (the G474's V3 and the VLDISCOVERY's V1) -- they are
#    different physical devices. A shared name like "stlink" would either serialise two
#    independent probes for nothing, or, after a replug, name a DIFFERENT board than the
#    one the last run meant.
#
# 2. REMOTE CLAIMS LOCK ON THE HOST THAT OWNS THE PROBE. gale's F100 lives on a
#    Raspberry Pi and is driven over ssh, so:
#        WRONG: with-device ... -- ssh pi@... openocd     (locks this laptop; no probe here)
#        RIGHT: ssh pi@... with-device ... -- openocd     (locks the Pi)
#    `claim_remote` does the right one. jess's "collisions are necessarily same-machine"
#    holds for the PROBE and not for the AGENTS: two laptops can both ssh into that Pi.
#
# If with-device cannot be found this REFUSES rather than running unclaimed, because a
# claim you silently skipped is worse than no convention at all. Deliberate override:
#   BENCH_UNCLAIMED=1   (prints a loud banner; use when you know you are alone)
set -uo pipefail

: "${BENCH_WHO:=gale}"
export BENCH_WHO

# Resolve with-device: explicit override, then PATH, then a sibling jess checkout.
_resolve_with_device() {
    if [ -n "${WITH_DEVICE:-}" ]; then echo "$WITH_DEVICE"; return 0; fi
    # VARVE FIRST. varve took with-device into the layer (varve#130); 2026.09.1 carries
    # 0.2.1 as a dispatched tool. Before this, gale resolved from PATH and silently used
    # whatever copy happened to be installed — `varve verify` flagged exactly that:
    #   "`with-device` on your PATH is /Users/r/.local/bin/with-device, not the pinned ..."
    # A bench claim asserted by an unpinned binary is a weaker statement than it looks.
    if command -v varve >/dev/null 2>&1 && varve which with-device >/dev/null 2>&1; then
        # stderr suppressed: varve which warns about PATH shadowing, which is real and
        # is reported properly by `varve verify` — inside a resolver it is just noise.
        varve which with-device 2>/dev/null | head -1; return 0
    fi
    if command -v with-device >/dev/null 2>&1; then command -v with-device; return 0; fi
    local sib
    # ${BASH_SOURCE[0]} is unset outside bash, and `set -u` turns that into a hard
    # error mid-resolution — guard it rather than assume the caller is bash.
    local here="${BASH_SOURCE[0]:-${0:-.}}"
    # A jess SOURCE checkout no longer provides a runnable tool (it is a Rust crate now),
    # so these are binary locations only. Pin the signed release rather than building:
    #   release:pulseengine/jess@v0.7.2!with-device-0.2.2-<triple>.tar.gz!with-device
    for sib in "$HOME/bench/with-device" \
               "$HOME/.local/bin/with-device" \
               "$(dirname "$here")/../../../../jess/target/release/with-device"; do
        # -f AND -x: `[ -x dir ]` is TRUE for any traversable directory, and
        # jess/tools/bench/with-device became a DIRECTORY when the tool was
        # rewritten in Rust. Testing -x alone resolved to that directory and
        # failed later with "permission denied" (exit 126) — a confusing error
        # far from its cause.
        [ -f "$sib" ] && [ -x "$sib" ] && { echo "$sib"; return 0; }
    done
    return 1
}

# The REGISTRY's name for this host's ST-LINK, and separately the serial to select it
# with. These are two different things and conflating them was my mistake, twice.
#
# I originally derived the lock name FROM the serial, arguing that a friendly name could
# drift onto a different board after a replug. jess solved it better: the registry maps a
# stable name to the serial, so the mapping lives in ONE place instead of in every
# consumer's string handling. A derived name does not match the registry, and
# with-device 0.2.2 refuses it outright:
#
#   UNKNOWN DEVICE 'stlink-003B001A3235511337333439'. Known: esp-jtag, stlink-v3
#   Refusing: an unregistered name would create its own lock and exclude nobody.
#
# Which is the same vacuous-lock defect I had already hit on the Pi with
# `stlink-v1-f100`. Registry name for the claim; serial for `probe-rs --probe`.
probe_device() { echo "${BENCH_DEV:-stlink-v3}"; }

# The serial comes from the registry entry, not from re-deriving it here, so the lock and
# the probe selector cannot name different boards.
probe_serial() {
    local reg="${BENCH_REGISTRY:-$HOME/.config/pulseengine/bench-devices.yaml}"
    [ -f "$reg" ] || return 1
    awk -v dev="$(probe_device):" '
        $1 == dev { in_dev = 1; next }
        in_dev && $1 == "serial:" { print $2; exit }
        in_dev && /^  [a-z0-9-]+:/ { exit }
    ' "$reg"
}

claim() {
    local dev="$1" purpose="$2"; shift 2
    [ "${1:-}" = "--" ] && shift
    local wd
    if ! wd=$(_resolve_with_device); then
        if [ "${BENCH_UNCLAIMED:-0}" = "1" ]; then
            echo "!! BENCH_UNCLAIMED=1 — running WITHOUT a bench claim on '$dev'." >&2
            echo "!! If another agent is on this probe, both measurements are suspect." >&2
            "$@"; return $?
        fi
        echo "bench-claim: with-device not found, refusing to touch '$dev' unclaimed." >&2
        echo "  pin the signed release and put the binary on PATH:\n    release:pulseengine/jess@v0.7.2!with-device-0.2.2-<triple>.tar.gz!with-device\n  or set WITH_DEVICE=/path/to/binary," >&2
        echo "  or set BENCH_UNCLAIMED=1 if you are certain you are alone." >&2
        return 4
    fi
    # v0.2.1 takes the purpose as a FLAG. The 0.2.0 python prototype took it
    # positionally, and passing it that way now makes with-device read it as a
    # second DEVICE NAME and refuse with "UNKNOWN DEVICE".
    "$wd" "$dev" --purpose "$purpose" -- "$@"
}

# Claim on a REMOTE host — the one the probe is plugged into.
# Assert that a claim on <device> is ALREADY held, without taking one. For scripts that
# run INSIDE someone else's claim and should not silently proceed if that assumption is
# wrong. with-device 0.2.2+ exports WITH_DEVICE_CLAIM and answers this directly; nesting
# unions rather than overwrites, so wrapping another claimed command still satisfies it.
#
# Holding *a* device is not holding *the* device -- that distinction is the whole point,
# and it is checked (a claim on the wrong device exits 2).
require_claim() {
    local dev="$1" wd
    if ! wd=$(_resolve_with_device); then
        [ "${BENCH_UNCLAIMED:-0}" = "1" ] && return 0
        echo "bench-claim: cannot verify a claim on '$dev' — with-device not found." >&2
        return 4
    fi
    # --require-claim arrived in 0.2.2; the pinned layer (2026.09.1) carries 0.2.1, whose
    # usage lists only --purpose and --wait. Passing it there exits 2 "unknown flag",
    # which reads like the claim check FAILED rather than like it could not be made.
    # Distinguish the two, because "I checked and you do not hold it" and "I could not
    # check" are different facts and only one of them should stop a caller.
    if ! "$wd" --help 2>&1 | grep -q -- "--require-claim"; then
        echo "bench-claim: this with-device ($("$wd" --version 2>/dev/null)) has no" >&2
        echo "  --require-claim; the assertion cannot be made. It arrived in 0.2.2 and the" >&2
        echo "  pinned layer carries 0.2.1. Not treating that as 'claim absent'." >&2
        return 5
    fi
    "$wd" --require-claim "$dev"
}

claim_remote() {
    local host="$1" dev="$2" purpose="$3"; shift 3
    [ "${1:-}" = "--" ] && shift
    # Installed by jess at ~/bench on fourpi. NOT ~/.local/bin — a stale 0.2.0
    # prototype lived there and reported exit 0 for commands it never ran.
    local remote_wd="${BENCH_REMOTE_WITH_DEVICE:-\$HOME/bench/with-device}"
    ssh "$host" "BENCH_WHO=$(printf %q "$BENCH_WHO") $remote_wd $(printf %q "$dev") --purpose $(printf %q "$purpose") -- $*"
}
