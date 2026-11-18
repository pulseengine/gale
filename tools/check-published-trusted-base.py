#!/usr/bin/env python3
"""Check that the published trusted-base.json describes the commit just deployed.

REQ-HONESTY-TCB-001 says the Pages deploy publishes trusted-base.json "whose
commit equals the deployed SHA". pages.yml wrote the file with GITHUB_SHA, but no
step fetched the published URL and compared, so the clause held by construction
and nothing observed it (requirements review 2026-11-17, row R23). A stale CDN
copy, a deploy that published another run's artifact, or a deploy that silently
did not replace the file would all have passed.

This script fetches the URL after the deploy and compares the `commit` field
with the expected SHA. GitHub Pages serves through a CDN with a cache lifetime of
up to 10 minutes, so it retries until a deadline. The retry is here, not in
shell, so it cannot exit 0 when the attempts run out (tools/check-retry-loops.py).

Usage:
    check-published-trusted-base.py --url URL --sha SHA [--deadline SECONDS]
    check-published-trusted-base.py --self-test

Exit 0: the published document is schema 1 for pulseengine/gale at SHA.
Exit 1: it is not, or could not be fetched, by the deadline.
Exit 2: usage error.
"""

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.request

SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def check(text, expected_sha):
    """Return a list of reasons the document does not describe expected_sha."""
    try:
        doc = json.loads(text)
    except ValueError as e:
        return [f"not JSON: {e}"]
    if not isinstance(doc, dict):
        return ["top level is not an object"]
    errs = []
    if doc.get("schema") != 1:
        errs.append(f"schema is {doc.get('schema')!r}, expected 1")
    if doc.get("repo") != "pulseengine/gale":
        errs.append(f"repo is {doc.get('repo')!r}, expected 'pulseengine/gale'")
    commit = doc.get("commit")
    if not isinstance(commit, str) or not SHA_RE.match(commit):
        errs.append(f"commit {commit!r} is not a 40-hex SHA")
    elif commit != expected_sha:
        errs.append(f"commit is {commit}, deployed SHA is {expected_sha}")
    if not isinstance(doc.get("generated_utc"), str):
        errs.append("generated_utc missing")
    return errs


def http_fetch(url, sha):
    # A query string the CDN has not seen, so the first request is not a cached copy.
    sep = "&" if "?" in url else "?"
    req = urllib.request.Request(
        f"{url}{sep}deployed={sha}-{int(time.time())}",
        headers={"Cache-Control": "no-cache", "User-Agent": "gale-ci"},
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return r.read().decode("utf-8", "replace")


def poll(fetch, expected_sha, deadline_s, interval_s, sleep=time.sleep, now=time.monotonic):
    """Fetch until the document checks clean or the deadline passes. Returns errors."""
    end = now() + deadline_s
    attempt = 0
    while True:
        attempt += 1
        try:
            errs = check(fetch(), expected_sha)
        except (urllib.error.URLError, OSError) as e:
            errs = [f"fetch failed: {e}"]
        if not errs:
            print(f"attempt {attempt}: published trusted-base.json is at {expected_sha}")
            return []
        print(f"attempt {attempt}: " + "; ".join(errs))
        if now() >= end:
            return errs
        sleep(interval_s)


def self_test():
    sha = "a" * 40
    good = json.dumps({"schema": 1, "repo": "pulseengine/gale", "commit": sha,
                       "generated_utc": "2026-11-18T00:00:00Z"})

    def with_(**kw):
        d = json.loads(good)
        for k, v in kw.items():
            if v is None:
                d.pop(k)
            else:
                d[k] = v
        return json.dumps(d)

    cases = [
        ("control: the deployed SHA", good, True),
        ("planted: another commit's SHA", with_(commit="b" * 40), False),
        ("planted: short SHA prefix", with_(commit=sha[:7]), False),
        ("planted: commit missing", with_(commit=None), False),
        ("planted: schema 2", with_(schema=2), False),
        ("planted: another repo", with_(repo="pulseengine/other"), False),
        ("planted: generated_utc missing", with_(generated_utc=None), False),
        ("planted: an HTML 404 page", "<html>404</html>", False),
    ]
    failed = 0
    for name, text, want_ok in cases:
        ok = not check(text, sha)
        mark = "ok" if ok == want_ok else "WRONG"
        failed += ok != want_ok
        print(f"  {mark:5} {name}: {'passes' if ok else 'caught'}")

    # The retry: a stale copy that turns current passes; one that never does fails
    # at the deadline instead of passing.
    clock = [0.0]
    seq = iter([with_(commit="b" * 40), good])
    errs = poll(lambda: next(seq), sha, 600, 20,
                sleep=lambda s: clock.__setitem__(0, clock[0] + s), now=lambda: clock[0])
    ok = not errs
    failed += not ok
    print(f"  {'ok' if ok else 'WRONG':5} control: stale copy, then current: {'passes' if ok else 'caught'}")

    clock = [0.0]
    errs = poll(lambda: with_(commit="b" * 40), sha, 60, 20,
                sleep=lambda s: clock.__setitem__(0, clock[0] + s), now=lambda: clock[0])
    ok = not errs
    failed += ok
    print(f"  {'WRONG' if ok else 'ok':5} planted: stale until the deadline: {'passes' if ok else 'caught'}")

    def boom():
        raise urllib.error.URLError("unreachable")
    clock = [0.0]
    errs = poll(boom, sha, 0, 20, sleep=lambda s: None, now=lambda: clock[0])
    failed += not errs
    print(f"  {'ok' if errs else 'WRONG':5} planted: URL unreachable: {'caught' if errs else 'passes'}")

    print(f"self-test: {11 - failed} of 11 as expected")
    return 1 if failed else 0


def main():
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--url")
    p.add_argument("--sha")
    p.add_argument("--deadline", type=int, default=900)
    p.add_argument("--interval", type=int, default=30)
    p.add_argument("--self-test", action="store_true")
    a = p.parse_args()
    if a.self_test:
        return self_test()
    if not a.url or not a.sha or not SHA_RE.match(a.sha):
        p.print_usage(sys.stderr)
        print("need --url and a 40-hex --sha", file=sys.stderr)
        return 2
    errs = poll(lambda: http_fetch(a.url, a.sha), a.sha, a.deadline, a.interval)
    if errs:
        print(f"FAIL: {a.url} does not describe {a.sha} after {a.deadline}s", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
