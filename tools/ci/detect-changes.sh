#!/usr/bin/env bash
# Changed-area detector for coverage.yml, gust-renode.yml, zephyr-tests.yml and
# llvm-lto.yml: is this change docs-only?
#
# Writes code=true|false to $GITHUB_OUTPUT. On code=false the heavy jobs skip
# by job-level `if:`, which still REPORTS their required context as skipped
# (= success); a workflow-level `paths:` reports nothing (gale#294).
#
# Inputs (env): EVENT_NAME, BASE_SHA. pull_request passes the PR base;
# merge_group passes merge_group.base_sha (the checkout is the queue commit, so
# base...HEAD is what that queue entry adds).
#
# GATE_PROBE_FILES (temper's gate probe, .github/gate-probe.json): a file listing
# changed paths, one per line. When set, those paths replace the event and the
# diff, so the probe exercises the classification below. Without this, the probe
# (which sets no EVENT_NAME) would always hit "unknown event, run everything"
# and pass without testing anything.
#
# FAILS CLOSED: any other event, an empty base, a failed or empty diff, or any
# file that is not docs runs everything. "Docs" is narrow on purpose: a `.md`
# or LICENSE file whose basename no non-markdown file in the tree mentions (a
# build, test or script that names a .md file may read it).
set -uo pipefail
out="${GITHUB_OUTPUT:-/dev/stdout}"
run_all() { echo "code=true" >> "$out"; echo "-> $1; running everything"; exit 0; }

if [ -n "${GATE_PROBE_FILES:-}" ]; then
  [ -r "$GATE_PROBE_FILES" ] || run_all "GATE_PROBE_FILES=$GATE_PROBE_FILES is not readable"
  changed="$(grep -v '^[[:space:]]*$' "$GATE_PROBE_FILES")" || run_all "GATE_PROBE_FILES lists no paths"
else
  case "${EVENT_NAME:-}" in
    pull_request|merge_group) ;;
    *) run_all "event ${EVENT_NAME:-<none>}" ;;
  esac
  [ -n "${BASE_SHA:-}" ] || run_all "no base sha"
  changed="$(git diff --name-only "$BASE_SHA"...HEAD)" || run_all "git diff failed against $BASE_SHA"
fi
[ -n "$changed" ] || run_all "no changed paths"
echo "changed files:"; echo "$changed"
while IFS= read -r f; do
  case "$f" in
    *.md|LICENSE|LICENSE-*) ;;
    *) run_all "non-docs file $f" ;;
  esac
  if git grep -qF -- "$(basename "$f")" -- ':!*.md' ':!tools/ci/detect-changes.sh'; then
    run_all "$f is named by a non-markdown file"
  fi
done <<< "$changed"
echo "code=false" >> "$out"
echo "-> docs-only change; skipping the heavy jobs"
