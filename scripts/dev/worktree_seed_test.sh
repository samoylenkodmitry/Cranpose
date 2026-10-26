#!/usr/bin/env bash
# A seeded worktree builds what a cold build of the same tree builds.
#
# A throwaway cargo workspace exercises each way a cloned target could go
# stale: a source the branch changed, a source the seed edited after its
# build, a directory a build script watches with a file removed from it, a
# file watched by absolute path, and a crate whose code holds its own path.
# Each case checks the program's output, so a unit that should have rebuilt
# and did not shows up as a wrong value, and the cases that stay fresh check
# that cargo compiled nothing.
set -euo pipefail
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_COMMON_DIR GIT_PREFIX

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SEEDER="$SCRIPT_DIR/worktree_seed.py"

passed=0
failed=0
ok() { printf '  ok    %s\n' "$1"; passed=$((passed + 1)); }
no() { printf '  FAIL  %s\n' "$1"; failed=$((failed + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else no "$1 (expected '$3', got '$2')"; fi; }

root="$(cd "$(mktemp -d "${TMPDIR:-/tmp}/worktree_seed_test.XXXXXX")" && pwd -P)"
trap 'rm -rf "$root"' EXIT
unset CARGO_TARGET_DIR RUSTC_WRAPPER CARGO_INCREMENTAL CARGO_TERM_QUIET CARGO_TERM_VERBOSE
# The checks read cargo's "Compiling" lines; CI asks for colour.
export CARGO_TERM_COLOR=never

seed="$root/seed"
mkdir -p "$seed"/{base,paths,watcher,app}/src "$seed/watcher/assets"
cat > "$seed/Cargo.toml" <<'EOF'
[workspace]
members = ["base", "paths", "watcher", "app"]
resolver = "2"
EOF
for crate in base paths watcher; do
    printf '[package]\nname = "%s"\nversion = "0.1.0"\nedition = "2021"\n' "$crate" > "$seed/$crate/Cargo.toml"
done
echo 'pub fn value() -> u32 { 1 }' > "$seed/base/src/lib.rs"
echo 'pub const DIR: &str = env!("CARGO_MANIFEST_DIR");' > "$seed/paths/src/lib.rs"
cat > "$seed/watcher/build.rs" <<'EOF'
use std::{env, fs, path::Path};
fn main() {
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let data = Path::new(&manifest).join("../data.txt").canonicalize().unwrap();
    println!("cargo::rerun-if-changed=assets");
    println!("cargo::rerun-if-changed={}", data.display());
    let assets = fs::read_dir("assets").unwrap().count();
    let data = fs::read_to_string(data).unwrap();
    let out = Path::new(&env::var("OUT_DIR").unwrap()).join("gen.rs");
    fs::write(out, format!("pub const ASSETS: usize = {assets};\npub const DATA: &str = {:?};\n", data.trim())).unwrap();
}
EOF
echo 'include!(concat!(env!("OUT_DIR"), "/gen.rs"));' > "$seed/watcher/src/lib.rs"
echo one > "$seed/watcher/assets/one.txt"
echo two > "$seed/watcher/assets/two.txt"
echo data-1 > "$seed/data.txt"
cat > "$seed/app/Cargo.toml" <<'EOF'
[package]
name = "app"
version = "0.1.0"
edition = "2021"
[dependencies]
base = { path = "../base" }
paths = { path = "../paths" }
watcher = { path = "../watcher" }
EOF
cat > "$seed/app/src/main.rs" <<'EOF'
fn main() {
    println!("{} {} {} {}", base::value(), watcher::ASSETS, watcher::DATA, paths::DIR);
}
EOF
echo target > "$seed/.gitignore"
( cd "$seed" && cargo generate-lockfile --quiet )
git -C "$seed" init -q
git -C "$seed" config user.email t@t
git -C "$seed" config user.name t
git -C "$seed" config commit.gpgsign false
git -C "$seed" add -A
git -C "$seed" commit -qm seed
( cd "$seed" && cargo build --quiet )

# A branch that changes `base`, removes a watched asset and changes data.txt.
git -C "$seed" branch work
git -C "$seed" worktree add -q "$root/branch" work
echo 'pub fn value() -> u32 { 2 }' > "$root/branch/base/src/lib.rs"
git -C "$root/branch" rm -q watcher/assets/two.txt
echo data-2 > "$root/branch/data.txt"
git -C "$root/branch" add -A
git -C "$root/branch" commit -qm change
git -C "$seed" worktree remove "$root/branch"

run_in() { ( cd "$1" && cargo run --quiet -p app 2>/dev/null ); }
compiled_in() { ( cd "$1" && cargo build -p app 2>&1 | sed -n 's/^ *Compiling \([a-z]*\).*/\1/p' | sort | tr '\n' ' ' ); }

echo "same commit as the seed"
git -C "$seed" worktree add -q --detach "$root/same" HEAD
python3 "$SEEDER" clone --seed "$seed" "$root/same" > /dev/null
check "only units naming the tree's own path rebuild" "$(compiled_in "$root/same")" "app paths watcher "
check "the program sees this tree" "$(run_in "$root/same")" "1 2 data-1 $root/same/paths"

echo "a branch that changed sources, a watched directory and a watched file"
git -C "$seed" worktree add -q --detach "$root/changed" work
python3 "$SEEDER" clone --seed "$seed" "$root/changed" > /dev/null
check "the program matches the branch" "$(run_in "$root/changed")" "2 1 data-2 $root/changed/paths"

echo "a seed edited after its build"
echo 'pub fn value() -> u32 { 7 }' > "$seed/base/src/lib.rs"
( cd "$seed" && cargo build --quiet )
git -C "$seed" worktree add -q --detach "$root/edited" HEAD
python3 "$SEEDER" clone --seed "$seed" "$root/edited" > /dev/null
check "the committed source is built, not the seed's edit" "$(run_in "$root/edited")" "1 2 data-1 $root/edited/paths"
git -C "$seed" checkout -q -- base/src/lib.rs

echo "edits after seeding"
git -C "$seed" worktree add -q --detach "$root/later" HEAD
python3 "$SEEDER" clone --seed "$seed" "$root/later" > /dev/null
run_in "$root/later" > /dev/null
echo 'pub fn value() -> u32 { 3 }' > "$root/later/base/src/lib.rs"
echo three > "$root/later/watcher/assets/three.txt"
echo data-3 > "$root/later/data.txt"
check "the next build sees every edit" "$(run_in "$root/later")" "3 3 data-3 $root/later/paths"

echo "the post-checkout hook"
mkdir -p "$root/hooks/.githooks" "$root/hooks/scripts/dev"
cp "$SCRIPT_DIR/../../.githooks/post-checkout" "$root/hooks/.githooks/"
cp "$SEEDER" "$root/hooks/scripts/dev/"
git -C "$seed" config core.hooksPath "$root/hooks/.githooks"
git -C "$seed" config cranpose.seed "$seed"
git -C "$seed" worktree add -q --detach "$root/hooked" HEAD > /dev/null
check "a new worktree gets a target" "$([ -d "$root/hooked/target" ] && echo yes || echo no)" yes
check "and builds the same program" "$(run_in "$root/hooked")" "1 2 data-1 $root/hooked/paths"
git -C "$root/hooked" checkout -q -b elsewhere
check "a branch switch leaves the target alone" "$(compiled_in "$root/hooked")" ""

echo "refresh"
git -C "$seed" config --unset core.hooksPath
git -C "$seed" config --unset cranpose.seed
cp "$SCRIPT_DIR/worktree_done.sh" "$root/hooks/scripts/dev/"
git init -q --bare "$root/origin.git"
git -C "$seed" remote add origin "$root/origin.git"
git -C "$seed" push -q origin HEAD:main
refresh() { python3 "$root/hooks/scripts/dev/worktree_seed.py" refresh "$seed" --warm "cargo build -p app" > /dev/null; }
refresh
first="$(git -C "$seed" config --get cranpose.seed)"
check "the seed is a worktree at origin/main" "$(git -C "$first" rev-parse HEAD)" "$(git -C "$seed" rev-parse origin/main)"
check "locked against the target sweep" "$(git -C "$seed" worktree list --porcelain | grep -c '^locked cargo seed$')" 1
check "and warm" "$(compiled_in "$first")" ""
git -C "$seed" worktree add -q "$root/next" -b next origin/main
echo 'pub fn value() -> u32 { 5 }' > "$root/next/base/src/lib.rs"
git -C "$root/next" commit -qam next
git -C "$root/next" push -q origin HEAD:main
refresh
second="$(git -C "$seed" config --get cranpose.seed)"
check "the next refresh moves the seed to the new main" "$(git -C "$second" rev-parse HEAD)" "$(git -C "$root/next" rev-parse HEAD)"
check "the old seed is gone" "$([ -e "$first" ] && echo kept || echo gone)" gone
check "the new seed runs the new code" "$(run_in "$second" | cut -d' ' -f1)" 5

echo
echo "worktree_seed: $passed passed, $failed failed"
[ "$failed" -eq 0 ]
