#!/usr/bin/env bash
# Remove an agent's git worktree once its work is safe elsewhere.
#
# Agents create worktrees for every task and leave them behind: each one is a
# full checkout plus a build directory that can reach tens of gigabytes, and
# the maintainer ended up deleting them by hand. This is the one command an
# agent runs when it is done with a worktree, instead of `rm -rf`, so the
# decision about what may go is made here, by rules, and never by a guess.
#
# It removes a worktree only when every one of these holds, and otherwise
# refuses and says which one failed:
#
#   1. The path is the top of a *linked* worktree. A repository's primary
#      checkout is never removed: it is the durable workspace someone returns
#      to, and whatever it holds may exist nowhere else.
#   2. Git does not report the worktree as locked.
#   3. No live process runs in it or names it: a build, a test binary, or a
#      shell standing in it.
#   4. Nothing is uncommitted: no modified, staged or untracked file. The only
#      untracked entries allowed are directories cargo tagged as caches
#      (CACHEDIR.TAG), which hold build output and nothing else.
#   5. Every commit it holds is on a remote: HEAD is reachable from a
#      remote-tracking ref after a fetch. Work that exists only here is never
#      deleted.
#
# Then `git worktree remove` deletes the directory and its registration, and
# the local branch is deleted too when a remote holds its tip. Nothing outside
# the worktree is touched. `--dry-run` reports the decision and removes nothing.
set -euo pipefail

readonly CACHEDIR_SIGNATURE='Signature: 8a477f597d28d172789f06886806bc55'

usage() {
    cat <<'USAGE'
usage: worktree_done.sh [--dry-run] [--no-fetch] <worktree-path>

Removes a linked git worktree whose work is committed and on a remote, and
its local branch when a remote holds it. Refuses, and says why, otherwise.

  --dry-run   Report what would happen; remove nothing.
  --no-fetch  Judge "on a remote" by the remote-tracking refs as they are,
              without fetching first.
USAGE
}

dry_run=0
fetch=1
path=""
while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run) dry_run=1 ;;
        --no-fetch) fetch=0 ;;
        -h|--help) usage; exit 0 ;;
        -*) echo "worktree_done.sh: unknown option '$1'" >&2; usage >&2; exit 2 ;;
        *)
            if [ -n "$path" ]; then
                echo "worktree_done.sh: one worktree at a time" >&2
                exit 2
            fi
            path="$1"
            ;;
    esac
    shift
done

refuse() {
    echo "worktree_done.sh: keeping $path: $*" >&2
    exit 1
}

[ -n "$path" ] || { usage >&2; exit 2; }
[ -d "$path" ] || refuse "not a directory"
path="$(cd "$path" && pwd -P)"

top="$(git -C "$path" rev-parse --show-toplevel 2>/dev/null || true)"
[ -n "$top" ] || refuse "not inside a git repository"
top="$(cd "$top" && pwd -P)"
[ "$top" = "$path" ] || refuse "not the top of a worktree (that is $top)"

# 1. A linked worktree has its own git dir under the common one; the primary
#    checkout's git dir is the common dir itself.
git_dir="$(git -C "$path" rev-parse --path-format=absolute --git-dir)"
common_dir="$(git -C "$path" rev-parse --path-format=absolute --git-common-dir)"
[ "$git_dir" != "$common_dir" ] || refuse "it is the repository's primary checkout"

# 2. Locked worktrees are someone's explicit request to keep them.
if git -C "$path" worktree list --porcelain | awk -v wt="$path" '
        /^worktree / { current = substr($0, 10) }
        /^locked/ && current == wt { found = 1 }
        END { exit found ? 0 : 1 }'; then
    refuse "git reports it locked"
fi

# 3. A process in it: its command line names the worktree, or its working
#    directory is inside it. This script's own process group is skipped, since
#    its git and ps calls carry the path.
live_process() {
    local pids pid pgid mine cwd
    mine="$(ps -o pgid= -p $$ 2>/dev/null | tr -d ' ')"
    pids="$(pgrep -f "$path" 2>/dev/null || true)"
    if command -v lsof >/dev/null 2>&1; then
        pids="$pids $(lsof -a -d cwd -Fpn 2>/dev/null | awk -v wt="$path" '
            /^p/ { pid = substr($0, 2) }
            /^n/ { dir = substr($0, 2); if (dir == wt || index(dir, wt "/") == 1) print pid }')"
    fi
    for pid in $pids; do
        [ "$pid" = "$$" ] && continue
        pgid="$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ')"
        [ -n "$pgid" ] || continue
        [ "$pgid" = "$mine" ] && continue
        cwd="$(ps -o command= -p "$pid" 2>/dev/null | cut -c1-80)"
        echo "$pid $cwd"
        return 0
    done
    return 1
}
if busy="$(live_process)"; then
    refuse "a live process uses it ($busy)"
fi

# 4. Uncommitted work. Untracked entries are allowed only when they are cargo
#    caches; everything else, ignored files aside, counts as work.
is_cargo_cache() {
    [ -f "$1/CACHEDIR.TAG" ] && grep -qF "$CACHEDIR_SIGNATURE" "$1/CACHEDIR.TAG" 2>/dev/null
}
dirty=""
while IFS= read -r line; do
    [ -n "$line" ] || continue
    status="${line:0:2}"
    entry="${line:3}"
    if [ "$status" = "??" ] && is_cargo_cache "$path/${entry%/}"; then
        continue
    fi
    dirty+="  $line"$'\n'
done < <(git -C "$path" status --porcelain=v1 --untracked-files=normal)
[ -z "$dirty" ] || refuse "it has uncommitted work:"$'\n'"$dirty"

# 5. Every commit on a remote. Pushes made by URL leave the remote-tracking
#    refs stale, so fetch first unless told not to.
if [ "$fetch" -eq 1 ]; then
    for remote in $(git -C "$path" remote); do
        git -C "$path" fetch --quiet --prune "$remote" 2>/dev/null \
            || echo "worktree_done.sh: could not fetch $remote; judging by its last known refs" >&2
    done
fi
head="$(git -C "$path" rev-parse HEAD)"
# The remote-tracking refs that hold a commit, their symbolic HEADs aside.
remote_refs_holding() {
    git -C "$1" for-each-ref --contains "$2" --format='%(refname)' refs/remotes \
        | awk '!/\/HEAD$/ { sub(/^refs\/remotes\//, ""); print }'
}
held_by="$(remote_refs_holding "$path" "$head" | head -1)"
[ -n "$held_by" ] || refuse "commit $(git -C "$path" rev-parse --short HEAD) is on no remote; push it first"

branch="$(git -C "$path" symbolic-ref --quiet --short HEAD 2>/dev/null || true)"
repo="$(dirname "$common_dir")"
[ "$(basename "$common_dir")" = ".git" ] || repo="$common_dir"

if [ "$dry_run" -eq 1 ]; then
    echo "would remove worktree $path (HEAD is on $held_by)"
    [ -z "$branch" ] || echo "would delete local branch $branch"
    exit 0
fi

git -C "$repo" worktree remove --force "$path"
echo "removed worktree $path (HEAD is on $held_by)"

# The branch goes too, but only when a remote holds its tip: the check above
# covered HEAD, and the branch is HEAD's, so this cannot lose a commit.
if [ -n "$branch" ] && git -C "$repo" show-ref --verify --quiet "refs/heads/$branch"; then
    tip="$(git -C "$repo" rev-parse "refs/heads/$branch")"
    if [ -n "$(remote_refs_holding "$repo" "$tip" | head -1)" ]; then
        git -C "$repo" branch -D "$branch" >/dev/null
        echo "deleted local branch $branch"
    fi
fi
