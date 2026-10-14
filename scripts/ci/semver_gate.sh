#!/usr/bin/env bash
# Refuses a release whose version breaks the rule in docs/release.md: each
# published crate's API is compared with release `previous`, and a change that
# breaks callers needs a minor version bump (0.9 -> 0.10), never a patch.
#
# Usage: semver_gate.sh <previous version, e.g. 0.9.43>
# Run it after `cargo xtask bump-release-version`, so the manifests carry the
# new version and cargo-semver-checks reads the release type from the change.
# SEMVER_GATE_CRATES, a space-separated list, narrows the crates for a test.
set -euo pipefail
previous="${1:?previous release version}"
if [ -n "${SEMVER_GATE_CRATES:-}" ]; then
    crates="$(tr ' ' '\n' <<< "$SEMVER_GATE_CRATES")"
else
    cargo xtask publish-order >/dev/null
    crates="$(cat publish-order.txt)"
fi

failed=()
while IFS= read -r crate; do
    [ -n "$crate" ] || continue
    if cargo xtask crate-published "$crate" "$previous"; then
        :
    else
        rc=$?
        if [ "$rc" -eq 1 ]; then
            echo "semver gate: ${crate} has no ${previous} release to compare with; skipped."
            continue
        fi
        echo "semver gate: could not read the registry for ${crate} ${previous}." >&2
        exit 1
    fi
    log="$(mktemp)"
    if cargo semver-checks -p "$crate" --baseline-version "$previous" 2>&1 | tee "$log"; then
        :
    elif grep -q "since they have no library target: ${crate}" "$log"; then
        # A proc-macro crate (cranpose-macros) has no API that rustdoc lists.
        echo "semver gate: ${crate} has no library target; skipped."
    elif grep -q "failed to build rustdoc for crate ${crate} v${previous}" "$log"; then
        # The release being compared with does not build with every feature on
        # (cranpose-testing 0.9.43); compare the default features instead.
        echo "semver gate: ${crate} ${previous} does not build with all features; comparing default features."
        cargo semver-checks -p "$crate" --baseline-version "$previous" --default-features \
            || failed+=("$crate")
    else
        failed+=("$crate")
    fi
    rm -f "$log"
done <<< "$crates"

if [ "${#failed[@]}" -gt 0 ]; then
    echo >&2
    echo "semver gate: these crates break callers of ${previous}: ${failed[*]}." >&2
    echo "A breaking change needs the next minor version: delete this tag and tag the" >&2
    echo "green main head with v<major>.<minor + 1>.0 instead (docs/release.md)." >&2
    exit 1
fi
echo "semver gate: every published crate keeps the API of ${previous}, or the version allows the change."
