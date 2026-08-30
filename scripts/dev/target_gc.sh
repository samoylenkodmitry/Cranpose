#!/usr/bin/env bash
# Keep worktree cargo target directories inside a free-disk budget.
#
# Every agent worktree builds its own complete `target/` for this workspace and
# nothing ever collects them. On 2026-08-29 that filled a 926GB disk to 117MB
# free and hard-stopped a running agent mid-investigation: its Bash tool could
# not create an output-capture file. Roughly 350GB sat in worktree target dirs.
#
# The policy here is cache eviction, because `target/` is a cache: every byte in
# it is reproducible from source and a compiler. So this does not ask whether a
# branch is finished, it asks how much disk is left, and evicts the
# least-recently-built targets until enough is free.
#
# That framing matters, because the two obvious eligibility rules both fail on
# this repo's actual workflow, and both were measured failing before this was
# written:
#
#   * "reclaim merged branches" -- this repo squash-merges, so a branch whose
#     work is in main is not an ancestor of it. docs/frame-cost-attribution
#     shipped as commit 6fca42ed and `git merge-base --is-ancestor` still
#     answers NO. The rule would essentially never fire. Asking `gh` instead
#     would put a network call and a flip-flopping auth account (see AGENTS.md)
#     in the path of reclaiming disk, which is worse than not reclaiming.
#   * "reclaim worktrees idle for N days" -- the 2026-08-29 exhaustion was
#     produced entirely by worktrees created within the previous 48 hours. Any
#     idle window wide enough to be safe is far wider than the hours it takes to
#     fill the disk.
#
# Least-recently-built, evicted only while under the free-space target, has
# neither problem: it always has a candidate, it stops as soon as the disk is
# healthy, and it never consults branch topology.
#
# Safety, in order of how much it matters:
#
#   1. The only thing this ever removes is a directory that cargo itself has
#      marked as a cache. `CACHEDIR.TAG` carries a fixed signature written by
#      cargo (bford.info/cachedir/); a directory without it is not touched, no
#      matter what it is called. Source, git state, and uncommitted work live
#      outside any such directory and are therefore unreachable from here.
#      target/ is gitignored, so no edit, staged or not, can be lost.
#   2. A target written to in the last few minutes has a build in flight.
#      Deleting under a running cargo produces confusing failures, so recent
#      write activity protects a target absolutely -- it is never evicted, even
#      if that means the budget goes unmet.
#   3. The repository's primary checkout, worktrees git reports as locked, and
#      the worktree this runs from are left alone unless asked for explicitly.
#      Protecting the primary checkout is a policy call rather than a safety
#      one: agent worktrees are interchangeable and cost only a rebuild, but the
#      primary checkout is the durable workspace someone returns to.
#   4. Dry run is the default. Nothing is removed without --apply.
#
# The removal renames before deleting. The rename is atomic and frees the
# `target` name immediately, so a build starting at that moment creates a fresh
# tree instead of racing a half-deleted one.
set -euo pipefail

readonly CACHEDIR_SIGNATURE='Signature: 8a477f597d28d172789f06886806bc55'

apply=0
min_free_gb=150
busy_minutes=15
include_locked=0
include_self=0
include_main=0
min_size_mb=100
extra_roots=()

usage() {
    cat <<'USAGE'
usage: target_gc.sh [options]

  --apply             Actually reclaim. Without it, report only (the default).
  --min-free-gb N     Evict until the disk has N GB free (default: 150).
  --busy-minutes N    A target written within N minutes has a live build and is
                      never evicted (default: 15).
  --min-size-mb N     Ignore target dirs smaller than N MB (default: 100).
  --root DIR          Also sweep cargo target dirs found under DIR. Repeatable.
                      Use for sibling checkouts that are not worktrees of this
                      repository.
  --include-locked    Also consider worktrees git reports as locked.
  --include-main      Also consider the repository's primary checkout. It is
                      protected by default: agent worktrees are disposable and
                      cost only a rebuild, but the primary checkout is the
                      durable workspace someone returns to.
  --include-self      Also consider the worktree this script runs from.
  -h, --help          This text.
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --apply) apply=1 ;;
        --include-locked) include_locked=1 ;;
        --include-main) include_main=1 ;;
        --include-self) include_self=1 ;;
        --min-free-gb) min_free_gb="${2:?--min-free-gb needs a value}"; shift ;;
        --busy-minutes) busy_minutes="${2:?--busy-minutes needs a value}"; shift ;;
        --min-size-mb) min_size_mb="${2:?--min-size-mb needs a value}"; shift ;;
        --root) extra_roots+=("${2:?--root needs a value}"); shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "target_gc.sh: unknown argument '$1'" >&2; usage >&2; exit 2 ;;
    esac
    shift
done

# A directory is reclaimable only if cargo says it is a cache. This is the one
# check that stands between this script and somebody's source tree.
is_cargo_target_dir() {
    local dir="$1"
    [ -d "$dir" ] || return 1
    [ -f "$dir/CACHEDIR.TAG" ] || return 1
    grep -qF "$CACHEDIR_SIGNATURE" "$dir/CACHEDIR.TAG" 2>/dev/null
}

mtime_of() {
    stat -f %m "$1" 2>/dev/null || stat -c %Y "$1" 2>/dev/null || echo 0
}

# The newest of the target root, its profile directories, and their fingerprint
# directories. Those are rewritten whenever cargo produces anything, which makes
# this a good "last built" clock without walking a tree that can hold millions
# of files -- a recursive scan of an 80GB target dir costs more than the
# eviction it is deciding.
target_last_build() {
    local dir="$1" newest candidate entry
    newest="$(mtime_of "$dir")"
    for entry in "$dir"/*; do
        [ -d "$entry" ] || continue
        candidate="$(mtime_of "$entry")"
        [ "$candidate" -gt "$newest" ] 2>/dev/null && newest="$candidate"
        if [ -d "$entry/.fingerprint" ]; then
            candidate="$(mtime_of "$entry/.fingerprint")"
            [ "$candidate" -gt "$newest" ] 2>/dev/null && newest="$candidate"
        fi
    done
    printf '%s\n' "$newest"
}

# `du` exits non-zero when a file disappears under it, which happens constantly
# in a target dir with a live cargo in it. Under `set -o pipefail` that failure
# would propagate and abort the whole sweep partway through, silently hiding
# every candidate after the first busy one -- so swallow it and keep the total
# du still prints.
# Any live process whose command line mentions the worktree -- a rustc writing
# into its target, or a test binary being executed out of it.
#
# mtime alone is not enough, and the gap is not theoretical: `cargo test` writes
# nothing to target/ while it *runs* the binaries it just built, and this
# workspace has single tests that run for two minutes. A suite that compiles for
# twenty minutes and then runs for forty looks, to a timestamp, like forty
# minutes of idleness. Evicting there produces exactly this, which names the
# build as the culprit and the disk not at all:
#
#   error: test failed, to rerun pass `-p desktop-app --test <name>`
#   Caused by:
#     could not execute process `target/ci/deps/<name>-<hash>` (never executed)
#   Caused by:
#     No such file or directory (os error 2)
#
# Processes in this script's own process group are ignored: the `du` this very
# sweep spawns carries the path it is measuring on its command line, and would
# otherwise report every candidate as busy with itself.
worktree_has_live_process() {
    local wt="$1" pids pid pgid mypgid
    command -v pgrep >/dev/null 2>&1 || return 1
    pids="$(pgrep -f "$wt" 2>/dev/null || true)"
    [ -n "$pids" ] || return 1
    mypgid="$(ps -o pgid= -p $$ 2>/dev/null | tr -d ' ')"
    for pid in $pids; do
        pgid="$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d ' ')"
        [ -n "$pgid" ] || continue
        [ "$pgid" = "$mypgid" ] && continue
        return 0
    done
    return 1
}

dir_size_mb() {
    local kb
    kb="$( { du -sk "$1" 2>/dev/null || true; } | awk 'NR == 1 { print $1 }')"
    [ -n "$kb" ] || kb=0
    printf '%d\n' "$(( kb / 1024 ))"
}

free_gb() {
    local gb
    gb="$( { df -k "${1:-$PWD}" 2>/dev/null || true; } | awk 'NR == 2 { printf "%d", $4 / 1048576 }')"
    [ -n "$gb" ] || gb=0
    printf '%s\n' "$gb"
}

human_gb() {
    awk -v mb="$1" 'BEGIN { printf "%.1fG", mb / 1024 }'
}

reclaim() {
    local target="$1" staged
    staged="${target}.gc-$$"
    mv "$target" "$staged" || return 1
    rm -rf "$staged"
}

# A sweep killed between the rename and the delete leaves a `target.gc-<pid>`
# behind, and nothing else in the system knows that name -- which would
# reintroduce exactly the unbounded-growth bug this script exists to fix, in
# miniature. Reap them first. The name is only ever produced by reclaim() four
# lines above, and the contents are a target dir that was already judged
# evictable, so this needs no further proof about what it is removing.
reap_orphans() {
    local wt staged reaped=0
    while IFS=$'\t' read -r wt _; do
        [ -n "$wt" ] && [ -d "$wt" ] || continue
        for staged in "$wt"/target.gc-*; do
            [ -d "$staged" ] || continue
            rm -rf "$staged"
            [ -d "$staged" ] || reaped=$((reaped + 1))
        done
    done <<EOF
$(worktree_records)
EOF
    [ "$reaped" -eq 0 ] || echo "Reaped $reaped interrupted sweep(s)."
}

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [ -z "$repo_root" ]; then
    echo "target_gc.sh: not inside a git repository" >&2
    exit 2
fi
self_root="$(cd "$repo_root" && pwd -P)"

# The primary checkout is the parent of the *common* git dir; every linked
# worktree shares that same common dir while having its own gitdir. Protecting
# it is a policy call, not a safety one: least-recently-built is the right
# eviction order among interchangeable scratch worktrees, but the primary
# checkout is not interchangeable with them, and a sweep that reclaims it
# charges a full rebuild to whoever was using the machine directly.
common_dir="$(git -C "$repo_root" rev-parse --git-common-dir 2>/dev/null || echo)"
case "$common_dir" in
    /*) ;;
    *) common_dir="$repo_root/$common_dir" ;;
esac
main_worktree="$(cd "$(dirname "$common_dir")" 2>/dev/null && pwd -P || echo)"

# `git worktree list --porcelain` emits a blank-line-separated record per
# worktree: a `worktree <path>` line, then optional `branch`, `detached`, and
# `locked` lines. Parse the records rather than the human format, whose columns
# shift with path length.
worktree_records() {
    { git -C "$repo_root" worktree list --porcelain 2>/dev/null || true; } | awk '
        /^worktree /  { path = substr($0, 10); locked = 0 }
        /^locked/     { locked = 1 }
        /^$/          { if (path != "") { print path "\t" locked; path = "" } }
        END           { if (path != "") { print path "\t" locked } }
    '
}

# One line per candidate: last_build_epoch, size_mb, protected_reason, path.
# Sorted oldest-built first, which is the eviction order.
collect_candidates() {
    local wt locked target root

    while IFS=$'\t' read -r wt locked; do
        [ -n "$wt" ] && [ -d "$wt" ] || continue
        target="$wt/target"
        is_cargo_target_dir "$target" || continue
        emit_candidate "$target" "$wt" "$locked"
    done <<EOF
$(worktree_records)
EOF

    for root in ${extra_roots+"${extra_roots[@]}"}; do
        [ -d "$root" ] || continue
        while IFS= read -r target; do
            [ -n "$target" ] || continue
            target="$(dirname "$target")"
            is_cargo_target_dir "$target" || continue
            emit_candidate "$target" "$(dirname "$target")" 0
        done < <(find "$root" -maxdepth 3 -type f -name CACHEDIR.TAG 2>/dev/null)
    done
}

emit_candidate() {
    local target="$1" wt="$2" locked="$3" size_mb last protect="" wt_real now
    local live=0
    worktree_has_live_process "$wt" && live=1
    size_mb="$(dir_size_mb "$target")"
    [ -n "$size_mb" ] || size_mb=0
    [ "$size_mb" -ge "$min_size_mb" ] || return 0

    last="$(target_last_build "$target")"
    now="$(date +%s)"

    if [ "$live" -eq 1 ]; then
        protect="live process using this worktree"
    elif [ $(( (now - last) / 60 )) -lt "$busy_minutes" ]; then
        protect="live build (written <${busy_minutes}m ago)"
    elif [ "$locked" = "1" ] && [ "$include_locked" -eq 0 ]; then
        protect="git-locked worktree"
    else
        wt_real="$(cd "$wt" 2>/dev/null && pwd -P || echo "$wt")"
        if [ "$include_self" -eq 0 ] && [ "$wt_real" = "$self_root" ]; then
            protect="current worktree"
        elif [ "$include_main" -eq 0 ] && [ -n "$main_worktree" ] \
            && [ "$wt_real" = "$main_worktree" ]; then
            protect="primary checkout (--include-main to override)"
        fi
    fi

    # "-" rather than "" for "not protected": the reader splits on tab, and tab
    # is IFS *whitespace*, so bash collapses a run of them into one delimiter.
    # An empty field here silently shifts the path into $protect and leaves
    # $target empty, which the display loop then skips -- every genuine eviction
    # candidate disappears and only protected ones survive, which reads exactly
    # like "nothing is reclaimable".
    printf '%s\t%s\t%s\t%s\n' "$last" "$size_mb" "${protect:--}" "$target"
}

[ "$apply" -eq 1 ] && reap_orphans

candidates="$(collect_candidates | sort -n)"
if [ -z "$candidates" ]; then
    echo "No cargo target directories above ${min_size_mb}MB found."
    exit 0
fi

start_free="$(free_gb "$self_root")"
echo "Free disk: ${start_free}G. Target: ${min_free_gb}G."
echo
printf '%-38s %8s  %-14s %s\n' "TARGET DIR" "SIZE" "LAST BUILT" "DECISION"

reclaimed_mb=0
would_mb=0
now="$(date +%s)"
free_now="$start_free"

while IFS=$'\t' read -r last size_mb protect target; do
    [ -n "$target" ] || continue
    label="$(basename "$(dirname "$target")")"
    age_h=$(( (now - last) / 3600 ))
    if [ "$age_h" -lt 48 ]; then age="${age_h}h ago"; else age="$(( age_h / 24 ))d ago"; fi

    if [ "$protect" != "-" ]; then
        printf '%-38s %8s  %-14s %s\n' "$label" "$(human_gb "$size_mb")" "$age" "protected: $protect"
        continue
    fi

    projected=$(( free_now + size_mb / 1024 ))
    if [ "$free_now" -ge "$min_free_gb" ]; then
        printf '%-38s %8s  %-14s %s\n' "$label" "$(human_gb "$size_mb")" "$age" "keep: budget already met"
        continue
    fi

    if [ "$apply" -eq 1 ]; then
        if reclaim "$target"; then
            reclaimed_mb=$(( reclaimed_mb + size_mb ))
            free_now="$(free_gb "$self_root")"
            printf '%-38s %8s  %-14s %s\n' "$label" "$(human_gb "$size_mb")" "$age" "RECLAIMED -> ${free_now}G free"
        else
            printf '%-38s %8s  %-14s %s\n' "$label" "$(human_gb "$size_mb")" "$age" "FAILED to reclaim"
        fi
    else
        would_mb=$(( would_mb + size_mb ))
        free_now="$projected"
        printf '%-38s %8s  %-14s %s\n' "$label" "$(human_gb "$size_mb")" "$age" "would reclaim -> ~${free_now}G free"
    fi
done <<EOF
$candidates
EOF

echo
if [ "$apply" -eq 1 ]; then
    echo "Reclaimed $(human_gb "$reclaimed_mb"). Free disk: $(free_gb "$self_root")G (was ${start_free}G)."
    if [ "$(free_gb "$self_root")" -lt "$min_free_gb" ]; then
        echo "Still under the ${min_free_gb}G target: everything else is protected." >&2
    fi
else
    echo "Would reclaim $(human_gb "$would_mb"). Re-run with --apply to do it."
fi
