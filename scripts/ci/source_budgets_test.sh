#!/usr/bin/env bash
set -euo pipefail

# source_budgets.py against a scratch repository: one library file with one
# of each counted thing, a long file, and a test that reads source text.

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
failures=0
repo="$workdir/repo"

mkdir -p "$repo/scripts/ci" "$repo/scripts/dev" "$repo/crates/a/src" "$repo/crates/a/tests"
cp "$script_dir/source_budgets.py" "$repo/scripts/ci/"
cp "$script_dir/../dev/strip_private_docs.py" "$repo/scripts/dev/"
cat > "$repo/crates/a/src/lib.rs" <<'RUST'
/// A private helper.
fn helper() {}

#[expect(clippy::too_many_arguments)]
pub fn wide(a: u8, b: u8, c: u8, d: u8, e: u8, f: u8, g: u8, h: u8) {}

pub(crate) fn reset_for_test() {}

pub fn flag() -> bool {
    std::env::var("CRANPOSE_FLAG").is_ok()
}
RUST
seq 1 12 | sed 's/.*/const _: u8 = 0;/' > "$repo/crates/a/src/long.rs"
cat > "$repo/crates/a/tests/source.rs" <<'RUST'
#[test]
fn pins_text() {
    let source = std::fs::read_to_string("src/lib.rs").expect("reads");
    assert!(source.contains("fn helper"));
}
RUST

budget_file="$repo/scripts/ci/source_budgets.toml"
budgets() {
  cat > "$budget_file" <<TOML
# budgets

[counts]
too_many_arguments = $1
private_docs = 1
for_test_fns = 1
cranpose_env_names = 1
source_text_checks = 1

[file_lines]
limit = 11

[file_lines.over]
"crates/a/src/long.rs" = $2
TOML
}
expect() {
  local name="$1" want="$2"
  shift 2
  local got=0
  python3 "$repo/scripts/ci/source_budgets.py" "$@" > "$workdir/out" 2>&1 || got=$?
  if [ "$got" = "$want" ]; then
    echo "ok: $name"
  else
    echo "FAIL: $name (exit $got, wanted $want)"
    cat "$workdir/out"
    failures=$((failures + 1))
  fi
}

budgets 1 12
expect "every count at its budget passes" 0

budgets 0 12
expect "a count over its budget fails" 1

budgets 2 12
expect "a count under its budget fails" 1

budgets 1 11
expect "a listed file that grew fails" 1

budgets 2 13
expect "--lower writes the lower counts" 0 --lower
if grep -q '^too_many_arguments = 1$' "$budget_file" && grep -q '"crates/a/src/long.rs" = 12' "$budget_file"; then
  echo "ok: the budget file holds the lowered counts"
else
  echo "FAIL: the budget file holds the lowered counts"
  cat "$budget_file"
  failures=$((failures + 1))
fi

budgets 1 12
seq 1 13 | sed 's/.*/const _: u8 = 0;/' > "$repo/crates/a/src/new.rs"
expect "a new file over the limit fails" 1

if [ "$failures" -ne 0 ]; then
  echo "source_budgets: $failures check(s) failed"
  exit 1
fi
echo "source_budgets: all checks passed"
