#!/usr/bin/env bash
# Safety invariants for worktree_done.sh, on a throwaway repository with a
# throwaway remote. The script deletes checkouts, so the cases that matter
# most are the ones it must refuse: each leaves the directory in place.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DONE="$SCRIPT_DIR/worktree_done.sh"

passed=0
failed=0
ok() { printf '  ok    %s\n' "$1"; passed=$((passed + 1)); }
no() { printf '  FAIL  %s\n' "$1"; failed=$((failed + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else no "$1 (expected '$3', got '$2')"; fi; }
present() { [ -d "$1" ] && echo present || echo gone; }
# The exit status of a run, its output discarded.
status_of() { "$@" >/dev/null 2>&1 && echo 0 || echo $?; }

root="$(mktemp -d "${TMPDIR:-/tmp}/worktree_done_test.XXXXXX")"
root="$(cd "$root" && pwd -P)"
sleeper=""
cleanup() {
    [ -z "$sleeper" ] || kill "$sleeper" 2>/dev/null || true
    rm -rf "$root"
}
trap cleanup EXIT

# The tests run git, which inherits a hook's GIT_DIR/GIT_INDEX_FILE when this
# runs from a pre-commit hook and would then act on the real repository.
unset GIT_DIR GIT_INDEX_FILE GIT_WORK_TREE

origin="$root/origin.git"
git init -q --bare "$origin"
main="$root/main"
git init -q -b main "$main"
git -C "$main" config user.email t@t
git -C "$main" config user.name t
git -C "$main" config commit.gpgsign false
echo seed > "$main/seed.txt"
git -C "$main" add -A
git -C "$main" commit -qm seed
git -C "$main" remote add origin "$origin"
git -C "$main" push -q origin main

# A worktree on a new branch at main's tip, which the remote already holds.
new_worktree() {
    git -C "$main" worktree add -q -b "$1" "$root/$1"
    printf '%s\n' "$root/$1"
}

echo "worktree_done.sh invariants"

check "the primary checkout is refused" "$(status_of "$DONE" "$main")" "1"
check "the primary checkout survives" "$(present "$main/.git")" "present"

wt="$(new_worktree sub)"
mkdir -p "$wt/dir"
check "a directory inside a worktree is refused" "$(status_of "$DONE" "$wt/dir")" "1"

wt="$(new_worktree dirty)"
echo change >> "$wt/seed.txt"
check "a modified file is refused" "$(status_of "$DONE" "$wt")" "1"
check "a worktree with a modified file survives" "$(present "$wt")" "present"

wt="$(new_worktree untracked)"
echo notes > "$wt/notes.txt"
check "an untracked file is refused" "$(status_of "$DONE" "$wt")" "1"

wt="$(new_worktree unpushed)"
echo more > "$wt/more.txt"
git -C "$wt" add more.txt
git -C "$wt" commit -qm "local only"
check "a commit on no remote is refused" "$(status_of "$DONE" "$wt")" "1"
check "a worktree with an unpushed commit survives" "$(present "$wt")" "present"

wt="$(new_worktree locked)"
git -C "$main" worktree lock "$wt"
check "a locked worktree is refused" "$(status_of "$DONE" "$wt")" "1"

wt="$(new_worktree busy)"
# Job control gives the sleeper its own process group, as any other program
# would have; the script ignores only its own group.
set -m
(cd "$wt" && exec sleep 30) &
sleeper=$!
set +m
sleep 1
check "a worktree a live process stands in is refused" "$(status_of "$DONE" "$wt")" "1"
kill "$sleeper" 2>/dev/null || true
wait "$sleeper" 2>/dev/null || true
sleeper=""

wt="$(new_worktree dry)"
check "a dry run succeeds" "$(status_of "$DONE" --dry-run "$wt")" "0"
check "a dry run removes nothing" "$(present "$wt")" "present"

wt="$(new_worktree pushed)"
echo work > "$wt/work.txt"
git -C "$wt" add work.txt
git -C "$wt" commit -qm work
git -C "$wt" push -q origin pushed
check "a pushed worktree is removed" "$(status_of "$DONE" "$wt")" "0"
check "its directory is gone" "$(present "$wt")" "gone"
check "its registration is gone" \
    "$(git -C "$main" worktree list --porcelain | grep -c "^worktree $wt\$" || true)" "0"
check "its local branch, held by the remote, is deleted" \
    "$(git -C "$main" show-ref --verify --quiet refs/heads/pushed && echo kept || echo deleted)" "deleted"

wt="$(new_worktree cached)"
mkdir -p "$wt/target-extra/debug"
printf 'Signature: 8a477f597d28d172789f06886806bc55\n' > "$wt/target-extra/CACHEDIR.TAG"
check "an untracked cargo cache does not block removal" "$(status_of "$DONE" "$wt")" "0"
check "the worktree and its cache are gone" "$(present "$wt")" "gone"

wt="$(new_worktree untagged)"
mkdir -p "$wt/target-extra"
echo notes > "$wt/target-extra/notes.txt"
check "an untagged directory named like a cache is refused" "$(status_of "$DONE" "$wt")" "1"

echo "$passed passed, $failed failed"
[ "$failed" -eq 0 ]
