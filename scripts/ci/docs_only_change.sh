#!/usr/bin/env bash
# Decide whether a pull request touches nothing a build can observe.
#
# Prints `true` or `false`, and sets the `docs_only` step output when it runs
# under Actions. Jobs that compile, link or render anything guard their steps
# on it; the jobs that genuinely gate prose (`just typos`, `just doc` and the
# doctests `just test` compiles out of crates/cranpose-core/README.md) must not.
#
# The predicate is deliberately narrow: a path counts as docs only when it ends
# in `.md`. Everything else -- images under docs/, tools/, scripts/, workflow
# files -- keeps the full board. A wrong `true` silently disables the robot
# suite, so every branch that cannot prove the diff is prose answers `false`.
set -euo pipefail

emit() {
    printf '%s\n' "$1"
    if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
        printf 'docs_only=%s\n' "$1" >> "$GITHUB_OUTPUT"
    fi
    exit 0
}

# Only a pull request carries a base to diff against. A push to main, a tag and
# a manual dispatch all run the full board.
[[ "${GITHUB_EVENT_NAME:-}" == "pull_request" ]] || emit false

# actions/checkout leaves the pull request's merge commit at HEAD, so HEAD^1 is
# the base and HEAD^2 the head. Both parents need `fetch-depth: 2`; without it,
# or when a job checks out some other ref, the diff is unknowable -- not empty.
git rev-parse --verify --quiet HEAD^1 >/dev/null || emit false
git rev-parse --verify --quiet HEAD^2 >/dev/null || emit false

# --no-renames is load-bearing. With rename detection a source file renamed to
# a .md reports only the .md destination, which would read as a prose-only
# diff; listing both sides means the vanished .rs is seen.
files="$(git diff --no-renames --name-only HEAD^1 HEAD)" || emit false

# An empty diff means the assumption above is wrong somewhere. Run everything.
[[ -n "$files" ]] || emit false

while IFS= read -r file; do
    case "$file" in
        *.md) ;;
        *) emit false ;;
    esac
done <<< "$files"

emit true
