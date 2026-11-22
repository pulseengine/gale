#!/bin/sh
# temper's gate probe (.github/gate-probe.yml) runs a repo-relative command from
# .github/gate-probe.json, with GATE_PROBE_FILES naming the changed paths. sieve
# is not in the repository: the gate-probe job installs the pinned one on PATH
# (.github/actions/setup-sieve). This only hands over to it; sieve reads
# GATE_PROBE_FILES itself, makes no git call, and writes its outputs to
# $GITHUB_OUTPUT. The verdict is the probe's, from those outputs.
exec sieve classify --config sieve.toml --format github-output
