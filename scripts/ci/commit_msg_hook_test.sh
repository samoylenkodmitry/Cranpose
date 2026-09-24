#!/usr/bin/env bash
# .githooks/commit-msg runs the global commit-msg hook before its own check.
#
# `just hooks` points core.hooksPath at .githooks, which hides the hooks a
# developer configured globally: a machine-wide commit-msg that rewrites a
# message never saw this repository's commits. Each case runs the hook
# against a throwaway global config, so the developer's own is never read.
set -euo pipefail

HOOK="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.githooks" && pwd)/commit-msg"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
failures=0

pass() { echo "ok: $1"; }
fail() { echo "not ok: $1"; failures=$((failures + 1)); }

global_hooks="$WORK/global-hooks"
mkdir -p "$global_hooks"
cat > "$global_hooks/commit-msg" <<'EOF'
#!/bin/sh
grep -v '^Rewritten-By:' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
EOF
chmod +x "$global_hooks/commit-msg"
printf '[core]\n\thooksPath = %s\n' "$global_hooks" > "$WORK/gitconfig"

run_hook() {
    GIT_CONFIG_GLOBAL="$1" "$HOOK" "$2"
}

message="$WORK/message"
printf 'Fix a thing\n\nRewritten-By: a tool\n' > "$message"
if run_hook "$WORK/gitconfig" "$message" && ! grep -q '^Rewritten-By:' "$message"; then
    pass "the global commit-msg rewrites this repository's messages"
else
    fail "the global commit-msg did not run"
fi

printf '#!/bin/sh\nexit 3\n' > "$global_hooks/commit-msg"
printf 'Fix a thing\n' > "$message"
if run_hook "$WORK/gitconfig" "$message"; then
    fail "a global commit-msg that refuses the message did not stop the commit"
else
    pass "a global commit-msg that refuses the message stops the commit"
fi

: > "$WORK/empty-gitconfig"
printf 'release: v1.2.3\n' > "$message"
if run_hook "$WORK/empty-gitconfig" "$message" 2>/dev/null; then
    fail "a hand-written release commit was accepted without a global hook"
else
    pass "without a global hook, a hand-written release commit is still refused"
fi
printf 'release: v1.2.3 [skip ci]\n' > "$message"
if run_hook "$WORK/empty-gitconfig" "$message"; then
    pass "publish.yml's own release commit passes"
else
    fail "publish.yml's own release commit was refused"
fi

printf '[core]\n\thooksPath = %s\n' "$(dirname "$HOOK")" > "$WORK/self-gitconfig"
printf 'Fix a thing\n' > "$message"
if run_hook "$WORK/self-gitconfig" "$message"; then
    pass "a global hooksPath that is this repository's own .githooks does not recurse"
else
    fail "a global hooksPath pointing at .githooks failed"
fi

if [ "$failures" -ne 0 ]; then
    echo "commit-msg hook: $failures check(s) failed"
    exit 1
fi
echo "commit-msg hook: all checks passed"
