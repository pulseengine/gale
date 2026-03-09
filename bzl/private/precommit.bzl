"""Pre-commit hook generation and installation rule.

Generates a git pre-commit hook that runs `bazel test //:precommit`.
Install with: bazel run //:install_precommit
"""

def _pre_commit_install_impl(ctx):
    """Generate and install a git pre-commit hook."""
    tests = [t.label for t in ctx.attr.tests]
    test_targets = " ".join([str(t) for t in tests])

    script_content = """\
#!/bin/bash
set -euo pipefail

# Find git root
GIT_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [ -z "$GIT_ROOT" ]; then
    echo "ERROR: Not inside a git repository."
    echo "Run 'git init' first."
    exit 1
fi

HOOKS_DIR="$GIT_ROOT/.git/hooks"
mkdir -p "$HOOKS_DIR"

cat > "$HOOKS_DIR/pre-commit" << 'HOOK'
#!/bin/bash
set -euo pipefail

echo "=== Running pre-commit checks ==="

# Run the pre-commit test suite
bazel test {test_suite} 2>&1
STATUS=$?

if [ $STATUS -ne 0 ]; then
    echo ""
    echo "Pre-commit checks FAILED. Commit aborted."
    echo "Fix the issues above, then try again."
    exit 1
fi

echo "=== Pre-commit checks passed ==="
HOOK

chmod +x "$HOOKS_DIR/pre-commit"
echo "Installed pre-commit hook to $HOOKS_DIR/pre-commit"
echo "Hook runs: bazel test {test_suite}"
""".format(test_suite = ctx.attr.test_suite)

    script = ctx.actions.declare_file(ctx.label.name + ".sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    return [DefaultInfo(executable = script)]

pre_commit_install = rule(
    implementation = _pre_commit_install_impl,
    attrs = {
        "tests": attr.label_list(doc = "Test targets the hook should run"),
        "test_suite": attr.string(
            mandatory = True,
            doc = "Bazel test target pattern for the pre-commit suite (e.g., //:precommit)",
        ),
    },
    executable = True,
    doc = "Install a git pre-commit hook that runs bazel test.",
)
