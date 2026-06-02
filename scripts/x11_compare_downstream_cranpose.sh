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
  scripts/x11_compare_downstream_cranpose.sh --app-root <path> [options] -- [x11_compare_window options]

Options:
  --app-root <path>       Downstream Cargo project to build twice.
  --framework-ref <ref>   Cranpose baseline git ref [origin/main].
  --fetch                 Fetch origin before resolving the framework ref.
  --label <name>          Output filename prefix [downstream-cranpose].
  --output-root <path>    Directory for captures and cargo targets
                          [$HOME/.cache/cranpose/x11-comparison].
  --worktree-root <path>  Directory for detached app/framework worktrees
                          [$HOME/.cache/cranpose/worktrees].
  --use-real-home         Launch baseline/current app runs with the caller's HOME.
                          By default, each run gets isolated app state under the output directory.
  -h, --help              Show this help.

Everything after -- is passed to scripts/x11_compare_window.sh, for example:
  --package leetcodedaily --bin leetcodedaily --title "LeetCode Daily Composer"
USAGE
}

app_root=
framework_ref="origin/main"
fetch_origin=0
label="downstream-cranpose"
output_root="${CRANPOSE_X11_COMPARISON_DIR:-$HOME/.cache/cranpose/x11-comparison}"
worktree_root="${CRANPOSE_BASELINE_WORKTREE_ROOT:-$HOME/.cache/cranpose/worktrees}"
use_real_home=0
pass_through=()

while [ "$#" -gt 0 ]; do
    case "$1" in
        --app-root)
            app_root="${2:?missing --app-root value}"
            shift 2
            ;;
        --framework-ref)
            framework_ref="${2:?missing --framework-ref value}"
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
        --use-real-home)
            use_real_home=1
            shift
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

if [ -z "$app_root" ]; then
    usage >&2
    exit 2
fi

if [ "$fetch_origin" = "1" ]; then
    git -C "$REPO_ROOT" fetch origin
fi

app_root="$(cd "$app_root" && pwd)"
app_sha="$(git -C "$app_root" rev-parse --verify HEAD^{commit})"
app_short="${app_sha:0:12}"
app_name="$(basename "$app_root")"

framework_sha="$(git -C "$REPO_ROOT" rev-parse --verify "$framework_ref^{commit}")"
framework_short="${framework_sha:0:12}"
framework_name="$(basename "$REPO_ROOT")"

baseline_framework_root="$worktree_root/$framework_name-$framework_short"
baseline_app_root="$worktree_root/$app_name-$app_short-baseline"
current_app_root="$worktree_root/$app_name-$app_short-current"
output_dir="$output_root/$label"

mkdir -p "$worktree_root" "$output_dir"

ensure_detached_worktree() {
    local source_root="$1"
    local destination_root="$2"
    local expected_sha="$3"
    local label="$4"

    if [ -e "$destination_root" ]; then
        local existing_sha
        existing_sha="$(git -C "$destination_root" rev-parse --verify HEAD 2>/dev/null || true)"
        if [ "$existing_sha" != "$expected_sha" ]; then
            echo "$label worktree path exists with a different commit: $destination_root" >&2
            echo "expected $expected_sha, found ${existing_sha:-not-a-git-worktree}" >&2
            exit 1
        fi
        return
    fi

    git -C "$source_root" worktree add --detach "$destination_root" "$expected_sha"
}

ensure_detached_worktree "$REPO_ROOT" "$baseline_framework_root" "$framework_sha" "baseline framework"
ensure_detached_worktree "$app_root" "$baseline_app_root" "$app_sha" "baseline app"
ensure_detached_worktree "$app_root" "$current_app_root" "$app_sha" "current app"

package_in_cargo_lock() {
    local lock_path="$1"
    local package="$2"

    if [ ! -f "$lock_path" ]; then
        return 0
    fi

    awk -v package="$package" '
        $0 == "[[package]]" {
            in_package = 1
            next
        }
        in_package && $0 ~ /^name = "/ {
            name = $0
            sub(/^name = "/, "", name)
            sub(/"$/, "", name)
            if (name == package) {
                found = 1
            }
            in_package = 0
        }
        END {
            exit(found ? 0 : 1)
        }
    ' "$lock_path"
}

cranpose_patch_args() {
    local option_name="$1"
    local framework_root="$2"
    local app_lock_path="$3"
    local config_args=()

    local crates=(
        "cranpose:crates/cranpose"
        "cranpose-animation:crates/cranpose-animation"
        "cranpose-app-shell:crates/cranpose-app-shell"
        "cranpose-assets:crates/cranpose-assets"
        "cranpose-core:crates/cranpose-core"
        "cranpose-foundation:crates/cranpose-foundation"
        "cranpose-macros:crates/cranpose-macros"
        "cranpose-platform-android:crates/cranpose-platform/android"
        "cranpose-platform-desktop-winit:crates/cranpose-platform/desktop-winit"
        "cranpose-platform-web:crates/cranpose-platform/web"
        "cranpose-render-common:crates/cranpose-render/common"
        "cranpose-render-pixels:crates/cranpose-render/pixels"
        "cranpose-render-wgpu:crates/cranpose-render/wgpu"
        "cranpose-runtime-std:crates/cranpose-runtime-std"
        "cranpose-services:crates/cranpose-services"
        "cranpose-testing:crates/cranpose-testing"
        "cranpose-ui:crates/cranpose-ui"
        "cranpose-ui-graphics:crates/cranpose-ui-graphics"
        "cranpose-ui-layout:crates/cranpose-ui-layout"
    )

    local entry package relative_path
    for entry in "${crates[@]}"; do
        package="${entry%%:*}"
        relative_path="${entry#*:}"
        if ! package_in_cargo_lock "$app_lock_path" "$package"; then
            echo "skipping unlocked Cranpose package patch: $package" >&2
            continue
        fi
        config_args+=("$option_name" "patch.crates-io.$package.path=\"$framework_root/$relative_path\"")
    done

    printf '%s\n' "${config_args[@]}"
}

baseline_patch_args=()
current_patch_args=()
mapfile -t baseline_patch_args < <(cranpose_patch_args "--baseline-cargo-config" "$baseline_framework_root" "$baseline_app_root/Cargo.lock")
mapfile -t current_patch_args < <(cranpose_patch_args "--current-cargo-config" "$REPO_ROOT" "$current_app_root/Cargo.lock")

x11_home_args=()
if [ "$use_real_home" = "0" ]; then
    x11_home_args+=(--isolated-app-home)
fi

exec "$SCRIPT_DIR/x11_compare_window.sh" \
    --baseline-root "$baseline_app_root" \
    --current-root "$current_app_root" \
    --label "$label" \
    --output-dir "$output_dir" \
    --baseline-target "$output_dir/baseline-target" \
    --current-target "$output_dir/current-target" \
    "${baseline_patch_args[@]}" \
    "${current_patch_args[@]}" \
    "${x11_home_args[@]}" \
    "${pass_through[@]}"
