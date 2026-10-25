#!/usr/bin/env bash
set -euo pipefail

# clippy_budget.py against a stand-in cargo that prints fixed clippy JSON:
# one float_cmp site reported by two targets, a second float_cmp site, a
# summary line without a span, and optionally a warning outside the budget.

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT
failures=0

mkdir -p "$workdir/repo/scripts/ci" "$workdir/bin"
cp "$script_dir/clippy_budget.py" "$workdir/repo/scripts/ci/"
cat > "$workdir/bin/cargo" <<'CARGO'
#!/usr/bin/env bash
site() {
  printf '{"reason":"compiler-message","message":{"level":"warning","message":"%s","code":{"code":"%s"},"rendered":"warning: %s\\n","spans":[{"is_primary":true,"file_name":"%s","line_start":%s,"column_start":1}]}}\n' "$1" "$2" "$1" "$3" "$4"
}
site "strict comparison" clippy::float_cmp src/lib.rs 3
site "strict comparison" clippy::float_cmp src/lib.rs 3
site "strict comparison" clippy::float_cmp src/lib.rs 9
if [ -n "${EXTRA_WARNING:-}" ]; then
  site "unused variable" unused_variables src/lib.rs 12
fi
printf '{"reason":"compiler-message","message":{"level":"warning","message":"3 warnings emitted","code":null,"rendered":"warning: 3 warnings emitted\\n","spans":[]}}\n'
CARGO
chmod +x "$workdir/bin/cargo"

budget_file="$workdir/repo/scripts/ci/clippy_budget.toml"
budget() {
  printf '# budgets\n\n[lints]\n"clippy::float_cmp" = %s\n' "$1" > "$budget_file"
}
run() {
  PATH="$workdir/bin:$PATH" python3 "$workdir/repo/scripts/ci/clippy_budget.py" "$@" > "$workdir/out" 2>&1
}
expect() {
  local name="$1" want="$2"
  shift 2
  local got=0
  run "$@" || got=$?
  if [ "$got" = "$want" ]; then
    echo "ok: $name"
  else
    echo "FAIL: $name (exit $got, wanted $want)"
    cat "$workdir/out"
    failures=$((failures + 1))
  fi
}

budget 2
expect "a site two targets report counts once and matches its budget" 0

budget 1
expect "a count over its budget fails" 1

budget 3
expect "a count under its budget fails until the budget comes down" 1

budget 3
expect "--lower writes the lower count" 0 --lower
if grep -q '"clippy::float_cmp" = 2' "$budget_file"; then
  echo "ok: the budget file holds the lowered count"
else
  echo "FAIL: the budget file holds the lowered count"
  cat "$budget_file"
  failures=$((failures + 1))
fi

budget 2
EXTRA_WARNING=1 expect "a warning outside the budget fails" 1

if [ "$failures" -ne 0 ]; then
  echo "clippy_budget: $failures check(s) failed"
  exit 1
fi
echo "clippy_budget: all checks passed"
