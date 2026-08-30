#!/usr/bin/env bash
# Regression test for docs_only_change.sh.
#
# A wrong `false` costs one redundant run. A wrong `true` skips the robot
# suite, the Android build, the iOS build, the architecture budgets and the
# wasm build on a change that could break any of them, and the board stays
# green while it does. That asymmetry is the whole design: every case that
# must not be mistaken for prose is pinned here.
#
# The second pass is what keeps this file honest. Asserting `false` proves
# nothing on its own -- the fail-safe branches answer `false` too, so a
# negative case can pass while testing nothing. So each path-based negative is
# replayed against a mutant detector whose path predicate is inverted, and must
# flip to `true`. A case that does not flip is not exercising the predicate.
# Positives are excluded: they already report `true`, so they cannot flip.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
detector="$script_dir/docs_only_change.sh"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

repo="$workdir/repo"
cases="$workdir/cases"
failures=0
: > "$cases"

open_pr() { git -C "$repo" checkout -q -b "$1" main; }

# Land the branch as a merge commit, the shape actions/checkout gives a job:
# first parent the base, second the head. Record it as a replayable case.
land() {
    local branch="$1" expect="$2" kind="$3" label="$4"
    git -C "$repo" checkout -q main
    git -C "$repo" merge -q --no-ff "$branch" -m "merge $branch"
    printf '%s\t%s\t%s\t%s\n' "$(git -C "$repo" rev-parse HEAD)" "$expect" "$kind" "$label" >> "$cases"
}

detect_at() { # sha, detector
    git -C "$repo" checkout -q --detach "$1"
    (cd "$repo" && GITHUB_EVENT_NAME=pull_request bash "$2")
}

check() {
    local label="$1" want="$2" got="$3"
    if [[ "$got" == "$want" ]]; then
        printf 'ok    %-44s %s\n' "$label" "$got"
    else
        printf 'FAIL  %-44s got %s, want %s\n' "$label" "$got" "$want"
        failures=$((failures + 1))
    fi
}

# --- fixtures -------------------------------------------------------------
mkdir -p "$repo"
git -C "$repo" init -q -b main
git -C "$repo" config user.email ci@example.invalid
git -C "$repo" config user.name ci
mkdir -p "$repo/docs/render-reference" "$repo/crates/foo/src" \
         "$repo/crates/foo/shaders" "$repo/tools/api-surface/src" \
         "$repo/.github/workflows"
echo prose > "$repo/docs/guide.md"
echo prose > "$repo/README.md"
echo 'fn main() {}' > "$repo/crates/foo/src/lib.rs"
echo 'fn helper() {}' > "$repo/crates/foo/src/helper.rs"
echo 'fn mixed() {}' > "$repo/crates/foo/src/mixed.rs"
echo 'fn shade() {}' > "$repo/crates/foo/shaders/blur.wgsl"
echo '[package]' > "$repo/crates/foo/Cargo.toml"
echo 'fn main() {}' > "$repo/tools/api-surface/src/main.rs"
echo 'name: ci' > "$repo/.github/workflows/ci.yml"
printf 'not-really-a-png' > "$repo/docs/render-reference/contract.png"
printf 'Apache License\n' > "$repo/LICENSE-APACHE"
printf 'MIT License\n' > "$repo/LICENSE-MIT"
printf 'Notices\n' > "$repo/NOTICE"
printf 'Contributors\n' > "$repo/AUTHORS"
printf 'default:\n\t@echo hi\n' > "$repo/justfile"
printf 'all:\n\techo hi\n' > "$repo/Makefile"
printf 'FROM scratch\n' > "$repo/Dockerfile"
git -C "$repo" add -A
git -C "$repo" commit -qm base

# Prose, the case the filter exists for. Nested and root-level both count:
# TIME_WASTERS.md and AGENTS.md live at the root, and the `**/*.md` glob that
# looks like the obvious filter does not match them.
open_pr prose-nested; echo more >> "$repo/docs/guide.md"
git -C "$repo" commit -qam .; land prose-nested true path "nested .md only"

open_pr prose-root; echo more >> "$repo/README.md"
git -C "$repo" commit -qam .; land prose-root true path "root-level .md only"

# Everything a build can observe. Each of these must outvote the prose in the
# same diff, so the mixed case carries a doc edit alongside the source edit.
open_pr mixed; echo more >> "$repo/docs/guide.md"
echo '// tweak' >> "$repo/crates/foo/src/lib.rs"
git -C "$repo" commit -qam .; land mixed false path "mixed: one doc + one .rs"

open_pr source-only; echo '// tweak' >> "$repo/crates/foo/src/helper.rs"
git -C "$repo" commit -qam .; land source-only false path ".rs only"

open_pr shader-only; echo '// tweak' >> "$repo/crates/foo/shaders/blur.wgsl"
git -C "$repo" commit -qam .; land shader-only false path "shader (.wgsl) only"

open_pr manifest-only; echo 'edition = "2024"' >> "$repo/crates/foo/Cargo.toml"
git -C "$repo" commit -qam .; land manifest-only false path "Cargo.toml only"

open_pr workflow-only; echo '# tweak' >> "$repo/.github/workflows/ci.yml"
git -C "$repo" commit -qam .; land workflow-only false path "workflow file only"

# tools/ is Rust that no gate currently builds. It still runs the full board:
# nothing pins it that way, and the cost of being wrong is one-sided.
open_pr tools-only; echo '// tweak' >> "$repo/tools/api-surface/src/main.rs"
git -C "$repo" commit -qam .; land tools-only false path "tools/ source only"

# Images under docs/ are fixtures, not prose.
open_pr docs-image; printf changed > "$repo/docs/render-reference/contract.png"
git -C "$repo" commit -qam .; land docs-image false path "docs/ image"

# Rename detection would report only the .md destination and read as prose.
open_pr rename-to-md; git -C "$repo" mv crates/foo/src/lib.rs crates/foo/src/lib.md
git -C "$repo" commit -qam .; land rename-to-md false path ".rs renamed to .md"

open_pr delete-source; git -C "$repo" rm -q crates/foo/src/helper.rs
git -C "$repo" commit -qam .; land delete-source false path "source deletion"

# The enumerated legal set. #560 renamed LICENSE-APACHE to LICENSE and burned a
# full heavy run on it; a licence file cannot change what any target builds.
open_pr licence-rename; git -C "$repo" mv LICENSE-APACHE LICENSE
git -C "$repo" commit -qam .; land licence-rename true path "#560: LICENSE-APACHE -> LICENSE"

open_pr legal-set; echo more >> "$repo/NOTICE"; echo more >> "$repo/AUTHORS"
echo more >> "$repo/LICENSE-MIT"
git -C "$repo" commit -qam .; land legal-set true path "NOTICE + AUTHORS + LICENSE-MIT"

# The set is closed on purpose. "Extensionless file" would swallow all three of
# these, and each one decides what CI builds.
open_pr justfile-only; echo '# tweak' >> "$repo/justfile"
git -C "$repo" commit -qam .; land justfile-only false path "justfile only"

open_pr makefile-only; echo '# tweak' >> "$repo/Makefile"
git -C "$repo" commit -qam .; land makefile-only false path "Makefile only"

open_pr dockerfile-only; echo '# tweak' >> "$repo/Dockerfile"
git -C "$repo" commit -qam .; land dockerfile-only false path "Dockerfile only"

# Prose plus a licence file is still prose; prose plus source is not.
open_pr licence-and-source; echo more >> "$repo/NOTICE"
echo '// tweak' >> "$repo/crates/foo/src/mixed.rs"
git -C "$repo" commit -qam .; land licence-and-source false path "LICENSE-set + .rs"

# --- pass 1: the predicate itself ----------------------------------------
echo "-- predicate --"
while IFS=$'\t' read -r sha expect kind label; do
    check "$label" "$expect" "$(detect_at "$sha" "$detector")"
done < "$cases"

# Events with no base to diff against, and a HEAD that is not a merge commit,
# never skip: the diff is unknowable, not empty.
git -C "$repo" checkout -q main
check "push event" false "$(cd "$repo" && GITHUB_EVENT_NAME=push bash "$detector")"

# main's own shape: a push event on a single-parent commit, where the squash
# merge leaves no HEAD^2. Both guards are live and only the first fires, so a
# reordering that broke the event check would still pass on the parent check
# and nobody would learn which one was load-bearing.
git -C "$repo" checkout -q --detach "$(git -C "$repo" rev-parse main^2)"
check "push event on single-parent HEAD" false \
    "$(cd "$repo" && GITHUB_EVENT_NAME=push bash "$detector")"
check "unset event name" false \
    "$(cd "$repo" && env -u GITHUB_EVENT_NAME bash "$detector")"
git -C "$repo" checkout -q main
check "workflow_dispatch event" false \
    "$(cd "$repo" && GITHUB_EVENT_NAME=workflow_dispatch bash "$detector")"
git -C "$repo" checkout -q --detach "$(git -C "$repo" rev-parse main^2)"
check "HEAD is not a merge commit" false \
    "$(cd "$repo" && GITHUB_EVENT_NAME=pull_request bash "$detector")"

# The step output the workflows read on must actually be written.
git -C "$repo" checkout -q main
output_file="$workdir/github_output"; : > "$output_file"
(cd "$repo" && GITHUB_EVENT_NAME=pull_request GITHUB_OUTPUT="$output_file" \
    bash "$detector" >/dev/null)
check "step output written" "docs_only=false" "$(cat "$output_file")"

# --- pass 2: the assertions must be load-bearing --------------------------
# Invert the path predicate so every path is accepted as prose. Every
# path-based case must now report `true`; one that still reports `false` is
# passing for some unrelated reason and is not testing what it claims.
mutant="$workdir/mutant.sh"
sed 's|^        \*) emit false ;;$|        *) ;;|' "$detector" > "$mutant"
if cmp -s "$detector" "$mutant"; then
    echo "FAIL  mutation did not apply -- the predicate moved, update this test" >&2
    exit 1
fi

# Only the negatives carry information here: a positive case already reports
# `true`, so it would "flip" to `true` without proving anything.
echo "-- mutation: inverted predicate must break every negative assertion --"
while IFS=$'\t' read -r sha expect kind label; do
    [[ "$kind" == "path" && "$expect" == "false" ]] || continue
    got="$(detect_at "$sha" "$mutant")"
    if [[ "$got" == "true" ]]; then
        printf 'ok    %-44s flips under mutation\n' "$label"
    else
        printf 'FAIL  %-44s did NOT flip (got %s): assertion is not load-bearing\n' \
            "$label" "$got"
        failures=$((failures + 1))
    fi
done < "$cases"

if [[ "$failures" -ne 0 ]]; then
    printf '\n%d case(s) failed\n' "$failures" >&2
    exit 1
fi
printf '\nall docs-only filter cases pass, and every negative assertion is load-bearing\n'
