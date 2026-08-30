#!/usr/bin/env bash
# Decide whether a pull request touches nothing a build can observe.
#
# Prints `true` or `false`, and sets the `docs_only` step output when it runs
# under Actions. Jobs that compile, link or render anything guard their steps
# on it; the jobs that genuinely gate prose (`just typos`, `just doc` and the
# doctests `just test` compiles out of crates/cranpose-core/README.md) must not.
#
# The predicate is deliberately narrow. A path counts as prose when it ends in
# `.md`, or when its basename is one of an enumerated set of extensionless
# legal files. Everything else -- images under docs/, tools/, scripts/,
# workflow files -- keeps the full board. A wrong `true` silently disables the
# robot suite, so every branch that cannot prove the diff is prose answers
# `false`.
#
# The legal set is a closed enumeration rather than a rule like "extensionless
# files", because the bypass surface of a list can be read off the list. Both
# endpoints of a rename must be in it or the rename is not prose: #560 moved
# LICENSE-APACHE to LICENSE, and with only one of the two named, --no-renames
# reports the other side and the diff answers `false`.
# "Extensionless" is unbounded and would swallow `justfile`, `Makefile` and
# `Dockerfile`, each of which decides what CI builds. Those three are pinned as
# explicit negatives in docs_only_change_test.sh so the set stays closed.
#
# Keyed on paths in the diff, never on the branch name, PR title or label.
# #540 was branched `docs/comment-policy` and carried 98 non-markdown files out
# of 100; a `docs/*` branch filter would have skipped both robot suites on it.
#
# Why this does not merge into scripts/ci/gate_diff.py, which also reads the
# diff: the two have deliberately opposite failure semantics. gate_diff scopes
# a correctness gate, so it fails LOUDLY when it cannot scope -- a gate that
# guesses its scope is worse than useless. This decides whether to skip work,
# so it fails SAFE: every ambiguous branch answers `false` and runs everything.
# gate_diff also derives its base from `origin/main`, which is why it needs
# `ensure_ref` to defend against a stale ref on a persistent runner; this reads
# `HEAD^1`/`HEAD^2` off the pull request's own merge commit and never consults
# `origin/main` at all, so that hazard cannot reach it. If they are ever
# unified, unify toward two callers of one helper -- never make either call the
# other, or "cannot tell" becomes a broken run in one direction and a silent
# pass in the other.
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
        LICENSE|LICENSE-APACHE|LICENSE-MIT|NOTICE|AUTHORS) ;;
        */LICENSE|*/LICENSE-APACHE|*/LICENSE-MIT|*/NOTICE|*/AUTHORS) ;;
        *) emit false ;;
    esac
done <<< "$files"

emit true
