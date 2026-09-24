#!/usr/bin/env bash
set -euo pipefail

# The robot suite runs its examples in two classes: the ones whose answers a
# busy machine can change run one at a time on an empty box, and everything
# else runs in parallel. Misfiling an example is not a red test -- a parallel
# example that secretly measures time goes flaky, and a serial one that does
# not just costs wall time -- so the classification is checked here rather
# than trusted.

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
RUNNER="$REPO_ROOT/run_robot_test.sh"

failures=0
check() {
    local description="$1"
    shift
    if "$@"; then
        echo "ok: $description"
    else
        echo "FAIL: $description" >&2
        failures=$((failures + 1))
    fi
}

classes_file="$(mktemp)"
trap 'rm -f "$classes_file"' EXIT
(cd "$REPO_ROOT" && "$RUNNER" --list-classes 2>/dev/null) \
    | grep -E '^(parallel|serial) ' > "$classes_file"

class_of() {
    awk -v name="$1" '$2 == name { print $1 }' "$classes_file"
}

discovered=$(
    cd "$REPO_ROOT"
    for file in apps/desktop-demo/robot-runners/robot_*.rs; do
        [ -f "$file" ] || continue
        grep -qE '^pub\(crate\) fn main\(' "$file" && basename "$file" .rs
    done | sort -u | grep -c ''
)
classified=$(grep -c '' "$classes_file")
check "every discovered example is classified ($discovered)" \
    [ "$discovered" -eq "$classified" ]

check "an example that compares frame work is serial" \
    [ "$(class_of robot_text_handle_cycle_stability)" = serial ]

# Not a named example on the parallel side. `robot_lazy_list` was that name
# until it turned out to define its own `wait_for_text`, which is a bounded
# wait and so genuinely serial -- the expectation was stale, not the
# classifier, and it took main red. What actually matters is that the
# partition still buys something: a predicate that widens until nearly
# everything measures time leaves a suite that is sequential again with extra
# steps. The fixtures below pin the classifier's logic in both directions.
parallel_count=$(awk '$1 == "parallel"' "$classes_file" | grep -c '')
check "most examples still run in parallel ($parallel_count of $classified)" \
    [ "$parallel_count" -ge "$((classified / 2))" ]
check "some examples run serially ($((classified - parallel_count)))" \
    [ "$parallel_count" -lt "$classified" ]

capture_command="$(cd "$REPO_ROOT" && just --dry-run robot-captures 2>&1)"
capture_examples="$(awk '{ for (i = 1; i < NF; i++) if ($i == "--example") print $(i + 1) }' <<< "$capture_command")"
check "the capture recipe declares examples" [ -n "$capture_examples" ]
fast_command="$(cd "$REPO_ROOT" && just --dry-run robot-linux-fast 2>&1)"
for example in $capture_examples; do
    check "$example belongs to the serial capture suite" \
        [ "$(class_of "$example")" = serial ]
    check "the fast suite does not start an empty capture run for $example" \
        bash -c '! grep -q -- "--example $2" <<< "$1"' _ "$fast_command" "$example"
done

# The transitive case has no instance in the suite today: every example that
# measures time also names the measurement itself. A fixture proves the
# classifier would still catch one that reached it only through a module,
# which is the shape a shared helper makes easy to write by accident.
fixture="$(mktemp -d)"
trap 'rm -f "$classes_file"; rm -r -- "$fixture"' EXIT
mkdir -p "$fixture/apps/desktop-demo/robot-runners"
cat > "$fixture/apps/desktop-demo/robot-runners/frame_stats.rs" <<'RS'
pub(crate) fn sample() -> f32 {
    let started = std::time::Instant::now();
    started.elapsed().as_secs_f32()
}
RS
# Nothing in this example's own source names a clock or a frame statistic:
# only the module it pulls in does. An example spelled this way is exactly
# what a classifier that reads one file at a time files as parallel.
cat > "$fixture/apps/desktop-demo/robot-runners/robot_via_module.rs" <<'RS'
use crate::frame_stats;

pub(crate) fn main() {
    let _ = frame_stats::sample();
}
RS
# The same reach through a `use crate::{...}` list that rustfmt wrapped over
# several lines, which a line-at-a-time reading misses.
cat > "$fixture/apps/desktop-demo/robot-runners/robot_via_wrapped_import.rs" <<'RS'
use crate::{
    frame_stats::{self},
};

pub(crate) fn main() {
    let _ = frame_stats::sample();
}
RS
cat > "$fixture/apps/desktop-demo/robot-runners/robot_plain.rs" <<'RS'
pub(crate) fn main() {
    println!("pixels only");
}
RS

# A class filter that removes every selected example must say so. It is a
# legitimate configuration -- `robot-captures` asks for the parallel class and
# all four of its examples measure -- but reporting "Total: 0, Passed: 0" and
# exiting zero reads exactly like a run that checked something, and CI
# believed that for a whole board.
empty_class_output="$(
    cd "$REPO_ROOT" \
        && "$RUNNER" --classes parallel --skip-build \
            --example robot_glass_tiles --example robot_lazy_perf 2>&1 || true
)"
check "a class filter that selects nothing says so" \
    grep -q "NOTHING RAN" <<< "$empty_class_output"
check "and does not call it a pass" \
    grep -q "This is not a pass" <<< "$empty_class_output"

fixture_classes="$(cd "$fixture" && "$RUNNER" --list-classes 2>/dev/null | grep -E '^(parallel|serial) ')"
check "an example that measures only through a module is serial" \
    grep -qx "serial robot_via_module" <<< "$fixture_classes"
check "an example that reaches the module through a wrapped import list is serial" \
    grep -qx "serial robot_via_wrapped_import" <<< "$fixture_classes"
check "a fixture example with no measurement is parallel" \
    grep -qx "parallel robot_plain" <<< "$fixture_classes"
check "a module without a main is not itself an example" \
    bash -c '! grep -q " frame_stats$" <<< "$1"' _ "$fixture_classes"

# Every runner is a module of one binary, so the build does not depend on the
# class: whatever runs, cargo is asked for `robot` and nothing else. A stub
# cargo records what the runner actually asked for.
stub_dir="$fixture/bin"
mkdir -p "$stub_dir"
cargo_args_file="$fixture/cargo-args"
cat > "$stub_dir/cargo" <<'CARGO'
#!/usr/bin/env bash
printf '%s\n' "$@" > "$ROBOT_CLASSES_TEST_CARGO_ARGS"
exit 0
CARGO
chmod +x "$stub_dir/cargo"

build_selection() {
    : > "$cargo_args_file"
    (
        cd "$fixture" \
            && PATH="$stub_dir:$PATH" \
               ROBOT_CLASSES_TEST_CARGO_ARGS="$cargo_args_file" \
               "$RUNNER" --classes "$1" --build-only
    ) > /dev/null 2>&1 || true
    cat "$cargo_args_file"
}

for class in parallel serial all; do
    build="$(build_selection "$class")"
    check "building the $class class reaches cargo, so the next checks read something" \
        grep -qx -- "--profile" <<< "$build"
    check "building the $class class asks for the robot binary and nothing else" \
        bash -c '[ "$(grep -A1 -x -- "--example" <<< "$1" | grep -vx -- "--example")" = robot ] \
            && ! grep -qx -- "--examples" <<< "$1"' _ "$build"
done

if [ "$failures" -ne 0 ]; then
    echo "$failures robot scheduling-class check(s) failed" >&2
    exit 1
fi
echo "robot scheduling classes: all checks passed"
