#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(git -C "$SCRIPT_DIR/.." rev-parse --show-toplevel)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/dev_build_common.sh"

enable_local_tmpdir
enable_local_cargo_job_limit

usage() {
    cat <<'USAGE'
Usage:
  scripts/x11_compare_origin_main.sh [options] -- [x11_compare_window options]

Options:
  --ref <git-ref>          Baseline git ref [origin/main].
  --fetch                  Fetch origin before resolving the baseline ref.
  --label <name>           Output filename prefix [origin-main].
  --output-root <path>     Directory for captures and cargo targets
                           [$HOME/.cache/cranpose/x11-comparison].
  --worktree-root <path>   Directory for detached baseline worktrees
                           [$HOME/.cache/cranpose/worktrees].
  --overlay-current-file <relative-path>
                           Copy a current-tree harness file into the baseline
                           worktree before building. Repeatable.
  -h, --help               Show this help.

Everything after -- is passed to scripts/x11_compare_window.sh, for example:
  --title "Cranpose Demo" --ignore-top-px 45 --expected-size 360x240
USAGE
}

baseline_ref="origin/main"
fetch_origin=0
label="origin-main"
output_root="${CRANPOSE_X11_COMPARISON_DIR:-$HOME/.cache/cranpose/x11-comparison}"
worktree_root="${CRANPOSE_BASELINE_WORKTREE_ROOT:-$HOME/.cache/cranpose/worktrees}"
pass_through=()
overlay_current_files=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        --ref)
            baseline_ref="${2:?missing --ref value}"
            shift 2
            ;;
        --fetch)
            fetch_origin=1
            shift
            ;;
        --label)
            label="${2:?missing --label value}"
            shift 2
            ;;
        --output-root)
            output_root="${2:?missing --output-root value}"
            shift 2
            ;;
        --worktree-root)
            worktree_root="${2:?missing --worktree-root value}"
            shift 2
            ;;
        --overlay-current-file)
            overlay_current_files+=("${2:?missing --overlay-current-file value}")
            shift 2
            ;;
        --)
            shift
            pass_through=("$@")
            break
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            echo "unknown option before --: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if [ "$fetch_origin" = "1" ]; then
    git -C "$REPO_ROOT" fetch origin
fi

baseline_sha="$(git -C "$REPO_ROOT" rev-parse --verify "$baseline_ref^{commit}")"
baseline_short="${baseline_sha:0:12}"
repo_name="$(basename "$REPO_ROOT")"
baseline_root="$worktree_root/$repo_name-$baseline_short"
output_dir="$output_root/$label"

mkdir -p "$worktree_root" "$output_dir"

if [ -e "$baseline_root" ]; then
    existing_sha="$(git -C "$baseline_root" rev-parse --verify HEAD 2>/dev/null || true)"
    if [ "$existing_sha" != "$baseline_sha" ]; then
        echo "baseline path exists but does not match $baseline_ref: $baseline_root" >&2
        echo "expected $baseline_sha, found ${existing_sha:-not-a-git-worktree}" >&2
        exit 1
    fi
else
    git -C "$REPO_ROOT" worktree add --detach "$baseline_root" "$baseline_sha"
fi

for relative_path in "${overlay_current_files[@]}"; do
    case "$relative_path" in
        /*|*..*)
            echo "overlay path must be workspace-relative and must not contain '..': $relative_path" >&2
            exit 2
            ;;
    esac
    source_path="$REPO_ROOT/$relative_path"
    target_path="$baseline_root/$relative_path"
    if [ ! -f "$source_path" ]; then
        echo "overlay source file does not exist: $source_path" >&2
        exit 1
    fi
    mkdir -p "$(dirname "$target_path")"
    cp "$source_path" "$target_path"
done

exec "$SCRIPT_DIR/x11_compare_window.sh" \
    --baseline-root "$baseline_root" \
    --current-root "$REPO_ROOT" \
    --label "$label" \
    --output-dir "$output_dir" \
    --baseline-target "$output_dir/baseline-target" \
    --current-target "$output_dir/current-target" \
    "${pass_through[@]}"
