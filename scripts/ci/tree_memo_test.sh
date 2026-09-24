#!/usr/bin/env bash
set -euo pipefail

# tree_memo.sh lets a job skip its work, so every way it could say "passed"
# wrongly is pinned here: another tree, another job, an expired marker.

# A git hook exports these for the repository it runs in; left set, they
# would point every git command below at that repository, not the fixture.
unset GIT_DIR GIT_INDEX_FILE GIT_WORK_TREE GIT_PREFIX GIT_OBJECT_DIRECTORY
# Nor may the host's own git configuration reach the fixture: a runner that
# signs every commit would make each fixture commit wait for a passphrase.
export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
subject="$script_dir/tree_memo.sh"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
export CRANPOSE_CI_MEMO_DIR="$workdir/memo"
failures=0

repo="$workdir/repo"
git init -q "$repo"
git -C "$repo" -c user.email=t@t -c user.name=t commit -q --allow-empty -m first
echo one > "$repo/file"
git -C "$repo" add file
git -C "$repo" -c user.email=t@t -c user.name=t commit -q -m one

check() {
    local label="$1" want="$2" key="$3"
    local got
    got="$(cd "$repo" && GITHUB_OUTPUT="$workdir/out" bash "$subject" check "$key" >/dev/null; tail -1 "$workdir/out")"
    if [[ "$got" == "passed=$want" ]]; then
        echo "ok    $label"
    else
        echo "FAIL  $label: got $got, want passed=$want"
        failures=$((failures + 1))
    fi
}

check "a tree nothing recorded has not passed" false "Rust/tests"
(cd "$repo" && bash "$subject" record "Rust/tests" >/dev/null)
check "the recorded tree has passed that job" true "Rust/tests"
check "the same tree has not passed another job" false "Rust/clippy"

git -C "$repo" -c user.email=t@t -c user.name=t commit -q --allow-empty -m "same tree, new commit"
check "a new commit with the same tree has passed" true "Rust/tests"

echo two > "$repo/file"
git -C "$repo" add file
git -C "$repo" -c user.email=t@t -c user.name=t commit -q -m two
check "a changed tree has not passed" false "Rust/tests"

git -C "$repo" checkout -q HEAD~1
check "going back to the recorded tree has passed again" true "Rust/tests"

touch -t 202001010000 "$CRANPOSE_CI_MEMO_DIR"/Rust_tests/*
check "an expired marker does not count" false "Rust/tests"

if (cd "$repo" && bash "$subject" bogus "Rust/tests" >/dev/null 2>&1); then
    echo "FAIL  an unknown mode must fail"
    failures=$((failures + 1))
else
    echo "ok    an unknown mode fails"
fi

if [[ "$failures" -ne 0 ]]; then
    echo "$failures tree memo check(s) failed" >&2
    exit 1
fi
echo "tree memo: all checks passed"
