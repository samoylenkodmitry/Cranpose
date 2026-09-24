#!/usr/bin/env bash
# Regression test for run_robot_test.sh's robot example discovery.
#
# A robot test is a file matching robot_*.rs under apps/desktop-demo/
# robot-runners/ that defines the runner entry point, `pub(crate) fn main`.
# Everything else matching that glob is a module the runners share, and the
# `robot` binary has no runner by that name. Before this predicate existed,
# discovery hardcoded a single excluded filename; a second shared module
# named robot_exit.rs was not on that list, and the suite asked for it anyway
# and reported a failure across the whole board the day a release was being
# cut. This file pins the predicate that replaced the hardcoded name, so a
# future shared module cannot repeat that outage.
#
# The predicate has two independent halves -- the robot_*.rs glob and the
# `pub(crate) fn main` grep -- and either one failing open reopens a
# different way to break the suite. Asserting "not discovered" proves nothing by itself: a
# discovery loop that finds nothing at all would pass every negative
# trivially. So every negative case here is replayed against a mutant with
# the relevant half of the predicate inverted, and must flip to "discovered".
# A case that does not flip is not exercising the half of the predicate it
# claims to. Positives are excluded from that replay: they already report
# success, so they cannot flip.
#
# This test never runs a real cargo build. It stubs the build tool
# run_robot_test.sh calls (CARGO_RUNNER) and stops at --build-only, which
# covers discovery, selection and skipping -- the whole surface this test
# exists to pin -- before the suite would spend minutes compiling or need a
# display.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "$script_dir/../.." && pwd)"
subject="$repo_root/run_robot_test.sh"
dev_build_common="$repo_root/scripts/dev_build_common.sh"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

fixture="$workdir/fixture"
failures=0

# --- fixtures ----------------------------------------------------------

# Every combination the predicate has to tell apart: right prefix with the
# entry point (discovered), right prefix without it (the robot_exit.rs
# shape), and wrong prefix with and without it (never discovered, regardless
# of content).
mkdir -p "$fixture/apps/desktop-demo/robot-runners" \
         "$fixture/scripts"
runners="$fixture/apps/desktop-demo/robot-runners"
printf 'pub(crate) fn main() {}\n' > "$runners/robot_alpha_fixture.rs"
printf 'pub(crate) fn main() {}\n' > "$runners/robot_beta_fixture.rs"
printf 'pub fn helper() {}\n' > "$runners/robot_exit_fixture.rs"
printf 'pub(crate) fn main() {}\n' > "$runners/not_robot_launcher_fixture.rs"
printf 'pub fn helper() {}\n' > "$runners/not_robot_helper_fixture.rs"

# Stands in for a real `cargo build`. It echoes what it was asked to build,
# so the one `robot` binary is verifiable by name.
cat > "$fixture/cargo-dev.sh" <<'STUB'
#!/usr/bin/env bash
echo "STUB_BUILD_ARGS: $*"
exit 0
STUB
chmod +x "$fixture/cargo-dev.sh"

# The real helper library, not a copy: symlinked so this test always runs
# against whatever host-lock and sccache logic the subject currently
# sources, rather than a snapshot that can drift from it.
ln -s "$dev_build_common" "$fixture/scripts/dev_build_common.sh"

# A fresh copy of the subject, taken new on every run, so this test always
# exercises whatever run_robot_test.sh currently does. It resolves
# ROBOT_DIR relative to the working directory and its own
# script directory relative to its invocation path, so it must be invoked
# with the fixture as both cwd and its own directory for those to line up
# with the stub cargo-dev.sh and the symlinked helper library above.
cp -p "$subject" "$fixture/run_robot_test.sh"

# --- harness -------------------------------------------------------------

run() {
    local script="$1"
    shift
    set +e
    last_output="$(cd "$fixture" && CI=1 \
        CRANPOSE_ROBOT_LOG_FILE="$workdir/run.log" \
        CRANPOSE_ROBOT_SUMMARY_FILE="$workdir/run.summary" \
        bash "$script" --build-only "$@" 2>&1)"
    last_status=$?
    set -e
}

check() {
    local label="$1" want_status="$2" needle="$3"
    if [[ "$last_status" -eq "$want_status" ]] && [[ "$last_output" == *"$needle"* ]]; then
        printf 'ok    %-58s status=%s\n' "$label" "$last_status"
    else
        printf 'FAIL  %-58s status=%s (want %s), needle %s\n' \
            "$label" "$last_status" "$want_status" \
            "$([[ "$last_output" == *"$needle"* ]] && echo found || echo MISSING)"
        printf '%s\n' "$last_output" | sed 's/^/      | /'
        failures=$((failures + 1))
    fi
}

# --- pass 1: discovery, selection and skip ---------------------------------
echo "-- discovery --"

run "$fixture/run_robot_test.sh" --example robot_alpha_fixture
check "robot_*.rs with the entry point is discovered" 0 "completed for 1 runner(s)."
check "the build asks for the one robot binary" 0 "--example robot"

run "$fixture/run_robot_test.sh" --example robot_exit_fixture
check "robot_*.rs without the entry point is NOT discovered (the robot_exit.rs shape)" \
    1 "Unknown robot example: robot_exit_fixture"

run "$fixture/run_robot_test.sh" --example not_robot_launcher_fixture
check "non-robot_-prefixed file with the entry point is NOT discovered" \
    1 "Unknown robot example: not_robot_launcher_fixture"

run "$fixture/run_robot_test.sh" --example not_robot_helper_fixture
check "non-robot_-prefixed file without the entry point is NOT discovered either" \
    1 "Unknown robot example: not_robot_helper_fixture"

run "$fixture/run_robot_test.sh"
check "default discovery finds 2 valid of 3 robot_*.rs candidates" \
    0 "completed for 2 runner(s)."

run "$fixture/run_robot_test.sh" --skip robot_alpha_fixture
check "--skip excludes the named example" 0 "Skipping robot example: robot_alpha_fixture"
check "--skip leaves the rest selected" 0 "completed for 1 runner(s)."

# --- pass 2: the predicate halves must each be load-bearing -----------------
#
# mutant_a inverts the entry-point check alone: files without it are kept
# and files with it are dropped. mutant_b broadens the glob alone: every
# .rs file in the directory is considered, not just robot_*.rs. mutant_c
# applies both, which is the only way to also flip the double negative --
# wrong prefix and no entry point -- since inverting either half alone still
# leaves the other excluding it.
mutant_a="$fixture/run_robot_test.mutant_a.sh"
sed -E 's/^([[:space:]]*)if ! grep -qE/\1if grep -qE/' "$fixture/run_robot_test.sh" > "$mutant_a"
if cmp -s "$fixture/run_robot_test.sh" "$mutant_a"; then
    echo "FAIL  mutant_a did not apply -- the entry-point check moved, update this test" >&2
    exit 1
fi

mutant_b="$fixture/run_robot_test.mutant_b.sh"
sed 's|"\$ROBOT_DIR"/robot_\*\.rs|"$ROBOT_DIR"/*.rs|' "$fixture/run_robot_test.sh" > "$mutant_b"
if cmp -s "$fixture/run_robot_test.sh" "$mutant_b"; then
    echo "FAIL  mutant_b did not apply -- the robot_*.rs glob moved, update this test" >&2
    exit 1
fi

mutant_c="$fixture/run_robot_test.mutant_c.sh"
sed -E 's/^([[:space:]]*)if ! grep -qE/\1if grep -qE/' "$mutant_b" > "$mutant_c"
if cmp -s "$mutant_b" "$mutant_c"; then
    echo "FAIL  mutant_c did not apply on top of mutant_b -- update this test" >&2
    exit 1
fi

echo "-- mutation: each half must be load-bearing --"

mutation_check() {
    local label="$1" script="$2"
    shift 2
    run "$script" "$@"
    if [[ "$last_status" -eq 0 ]]; then
        printf 'ok    %-58s flips under mutation\n' "$label"
    else
        printf 'FAIL  %-58s did NOT flip (status=%s): assertion is not load-bearing\n' \
            "$label" "$last_status"
        failures=$((failures + 1))
    fi
}

mutation_check "robot_exit_fixture flips when the entry-point check is inverted" \
    "$mutant_a" --example robot_exit_fixture
mutation_check "not_robot_launcher_fixture flips when the robot_*.rs glob is broadened" \
    "$mutant_b" --example not_robot_launcher_fixture
mutation_check "not_robot_helper_fixture flips only once both halves are broadened" \
    "$mutant_c" --example not_robot_helper_fixture

if [[ "$failures" -ne 0 ]]; then
    printf '\n%d case(s) failed\n' "$failures" >&2
    exit 1
fi
printf '\nall robot-discovery cases pass, and every negative assertion is load-bearing\n'
