#!/usr/bin/env bash
# Safety invariants for target_gc.sh, on a throwaway repository.
#
# The invariants are the point: this script deletes directories, so the tests
# that matter are the ones proving what it refuses to delete. Two of the cases
# below are regressions that actually happened during development -- an empty
# field silently eating a record, and a delete chained onto a counter -- and
# they are here so they cannot come back.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GC="$SCRIPT_DIR/target_gc.sh"

# target_gc.sh resolves the repository it sweeps from its working directory.
# Every invocation here therefore runs with the fixture as CWD, never the
# directory this test happens to be launched from. Running it without that cd
# once, during development, pointed a --min-free-gb 999999 sweep at the real
# checkout and reclaimed ~25GB of live worktrees' build artifacts. It removed
# only cargo caches and lost nothing, but it was not what the test meant to do,
# and "the tool under test defaults to the machine you are standing on" is
# exactly the mistake a test must not make.
gc() { ( cd "$fixture_repo" && "$GC" "$@" ); }

passed=0
failed=0

ok() { printf '  ok    %s\n' "$1"; passed=$((passed + 1)); }
no() { printf '  FAIL  %s\n' "$1"; failed=$((failed + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else no "$1 (expected '$3', got '$2')"; fi; }

root="$(mktemp -d "${TMPDIR:-/tmp}/target_gc_test.XXXXXX")"
cleanup() {
    chmod -R u+w "$root" 2>/dev/null || true
    rm -rf "$root"
}
trap cleanup EXIT

# A repository with two linked worktrees, so `git worktree list` has something
# real to report -- the script parses its porcelain output and cannot be tested
# against a directory tree alone.
main_wt="$root/main"
fixture_repo="$main_wt"
mkdir -p "$main_wt"
git -C "$main_wt" init -q
git -C "$main_wt" config user.email t@t
git -C "$main_wt" config user.name t
echo seed > "$main_wt/seed.txt"
git -C "$main_wt" add -A
git -C "$main_wt" commit -qm seed
git -C "$main_wt" worktree add -q -b wt-old "$root/old"
git -C "$main_wt" worktree add -q -b wt-new "$root/new"

# A target dir cargo would recognise: the CACHEDIR.TAG signature is the only
# thing that makes a directory eligible for removal.
make_target() {
    local dir="$1" stamp="$2"
    mkdir -p "$dir/ci/.fingerprint" "$dir/ci/deps"
    printf 'Signature: 8a477f597d28d172789f06886806bc55\n' > "$dir/CACHEDIR.TAG"
    dd if=/dev/zero of="$dir/ci/deps/blob" bs=1m count=120 2>/dev/null
    touch -t "$stamp" "$dir/ci/.fingerprint" "$dir/ci" "$dir"
}

make_target "$root/old/target" 202608010000
make_target "$root/new/target" 202608200000

echo "target_gc.sh invariants"

# 1. The core safety invariant. A directory called target with no CACHEDIR.TAG
#    is not a cargo cache and must survive even when everything else is being
#    reclaimed.
mkdir -p "$root/old/nottarget"
echo "precious" > "$root/old/nottarget/source.rs"
untagged="$root/new/target-lookalike"
mkdir -p "$untagged"
echo "precious" > "$untagged/source.rs"
gc --apply --min-free-gb 999999 --busy-minutes 0 >/dev/null 2>&1 || true
check "untagged lookalike dir survives a full sweep" \
    "$([ -f "$untagged/source.rs" ] && echo kept || echo GONE)" "kept"
check "worktree source survives a full sweep" \
    "$([ -f "$root/old/seed.txt" ] && echo kept || echo GONE)" "kept"

# 2. Least-recently-built goes first. Both targets were eligible above, so the
#    older one must have been chosen before the newer one.
check "older target reclaimed" "$([ -d "$root/old/target" ] && echo present || echo gone)" "gone"

# 3. Dry run is the default and removes nothing.
make_target "$root/old/target" 202608010000
gc --min-free-gb 999999 --busy-minutes 0 >/dev/null 2>&1 || true
check "dry run removes nothing" "$([ -d "$root/old/target" ] && echo present || echo gone)" "present"

# 4. A met budget removes nothing, even with --apply.
gc --apply --min-free-gb 0 --busy-minutes 0 >/dev/null 2>&1 || true
check "met budget removes nothing" "$([ -d "$root/old/target" ] && echo present || echo gone)" "present"

# 5. An interrupted sweep leaves target.gc-<pid> behind; nothing else knows that
#    name, so it must be reaped or it grows without bound -- the same unbounded
#    growth the script exists to stop.
mkdir -p "$root/new/target.gc-4242/junk"
gc --apply --min-free-gb 0 >/dev/null 2>&1 || true
check "orphaned staging dir reaped" \
    "$([ -d "$root/new/target.gc-4242" ] && echo present || echo gone)" "gone"

# 6. Records must survive parsing when a field is empty. Emitting "" for the
#    unprotected case made bash collapse the tab run, shifting the path into the
#    wrong variable and dropping every evictable candidate -- which looked
#    exactly like "nothing is reclaimable" rather than like a bug.
make_target "$root/old/target" 202608010000
make_target "$root/new/target" 202608200000
listing="$(gc --min-free-gb 999999 --busy-minutes 0 2>&1 || true)"
check "unprotected candidates appear in the listing" \
    "$(printf '%s\n' "$listing" | grep -c 'would reclaim')" "2"

# 7. A locked worktree is left alone unless asked for.
git -C "$main_wt" worktree lock "$root/new"
listing="$(gc --min-free-gb 999999 --busy-minutes 0 2>&1 || true)"
check "locked worktree is protected" \
    "$(printf '%s\n' "$listing" | grep -c 'git-locked')" "1"
git -C "$main_wt" worktree unlock "$root/new"

echo
echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
