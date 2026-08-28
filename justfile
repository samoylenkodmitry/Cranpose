# Every gate this repo enforces, in one place.
#
# CI calls these recipes rather than spelling commands inline, so a gate cannot
# drift between what a developer runs locally and what a pull request is
# measured against. If you change a gate, change it here.
#
# Scope: developer-facing gates -- format, lint, test, docs, budgets, builds,
# robot, perf. Release plumbing (version syncing, `cargo publish`, tag moves,
# Pages deployment) stays inline in its workflow: nobody runs it locally, so
# wrapping it here would add indirection without removing any divergence.
#
# `just` alone lists every recipe.

# The pinned nightly, read from its toolchain file so the version lives in
# exactly one place. `sed -n 's/.../\1/p'` rather than `sed -nr`: BSD sed on the
# macOS runners has no `-r`.
nightly := `sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain-nightly.toml`
stable := `sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml`

default:
    @just --list --unsorted

# --- toolchain -------------------------------------------------------------

# Print the pinned stable toolchain.
rv:
    @echo '{{stable}}'

# Print the pinned nightly toolchain (used by fmt and dist-min).
rv-nightly:
    @echo '{{nightly}}'

# Install both pinned toolchains and the components the gates need.
toolchains:
    rustup toolchain install {{stable}} --profile minimal --component clippy --component rustfmt --component rust-src
    rustup toolchain install {{nightly}} --profile minimal --component rustfmt --component rust-src

# --- format ----------------------------------------------------------------

# rustfmt.toml sets unstable import options, so formatting is the one gate that
# runs on nightly rather than on the pinned stable.

# Format the workspace.
fmt:
    RUSTUP_TOOLCHAIN={{nightly}} cargo fmt --all
    RUSTUP_TOOLCHAIN={{nightly}} cargo fmt --all --manifest-path apps/isolated-demo/Cargo.toml

# Verify formatting without touching the tree. This is the CI gate.
fmt-check:
    RUSTUP_TOOLCHAIN={{nightly}} cargo fmt --all --check
    RUSTUP_TOOLCHAIN={{nightly}} cargo fmt --all --check --manifest-path apps/isolated-demo/Cargo.toml

# --- lint ------------------------------------------------------------------

# Lint the whole workspace, every target.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# A plain build does not catch `Arc<dyn Trait>` values that pay for
# synchronisation the single-threaded wasm target cannot use; only clippy does.

# Lint the exact package and feature set the web demo ships.
clippy-wasm:
    scripts/ci/with_host_lock.sh --shared \
      cargo clippy --target wasm32-unknown-unknown -p desktop-app-platform --no-default-features --features web,renderer-wgpu -- -D warnings

# Lint the iOS simulator binary.
clippy-ios:
    cargo clippy -p desktop-app --bin cranpose-ios --target aarch64-apple-ios-sim --no-default-features --features ios -- -D warnings

# Lint the exact package, features and ABIs the Android build ships.
#
# `just android` runs Gradle, which does not deny warnings, so Android was the
# one shipped target with no zero-warning gate: anything behind an
# android-only `cfg` reached main unlinted. The package, `--lib`,
# `--no-default-features` and the feature list mirror what
# `CranposeAndroidPlugin` passes to `cargo ndk` (its `features` convention is
# `android,renderer-wgpu` and `defaultFeatures` is false); `--platform` is the
# demo's `minSdk`. Change them together or this stops checking what ships.
#
# Every ABI `releaseAbis` builds under CI is linted, not just arm64. The
# 32-bit ones are not redundant: `libc::timespec::tv_sec` is `i64` on
# aarch64 and `i32` on armeabi-v7a and x86, so a width assumption that is
# invisible on a 64-bit ABI is a type error on a 32-bit one. An arm64-only
# gate reported an `as i64` on that field as an unnecessary cast, and taking
# that advice would have broken the shipped 32-bit build.
#
# `missing_const_for_thread_local` is allowed here only. `thread_local!`
# expands differently per target, and the Android expansion defeats the
# lint's const detection: on the same pinned toolchain and the same source it
# fires 16 times here and never on host, including on initializers already
# written as `const {}` and on one that cannot be const at all
# (`HashMap::default()`). It stays enabled everywhere else, so a genuine
# non-const initializer is still caught by `just clippy`.
clippy-android:
    cargo ndk --platform 24 -t arm64-v8a -t armeabi-v7a -t x86 -t x86_64 clippy -p desktop-app-platform --lib --no-default-features --features android,renderer-wgpu -- -D warnings -A clippy::missing_const_for_thread_local

# --- test ------------------------------------------------------------------

# `--profile ci` keeps the debuginfo that the local dev profile strips, so a
# backtrace from a failing gate is readable.

# The workspace test suite.
test:
    cargo test --profile ci --workspace

# Feature permutations of the core crate that the default build does not cover.
test-features:
    cargo test --profile ci -p cranpose-core --features std-hash
    cargo test --profile ci -p cranpose-core --features internal

# The deterministic slot-table model check, at the frame count CI uses.
test-property:
    cargo test --profile ci -p cranpose-core deterministic_model_render_frames_match_slot_table

# The same check at stress depth: release codegen and 10k frames.
test-property-stress frames="10000":
    CRANPOSE_SLOT_MODEL_STRESS_FRAMES={{frames}} cargo test --release -p cranpose-core deterministic_model_render_frames_match_slot_table -- --nocapture

# A broken intra-doc link is a broken published page. `--all-features` so the
# feature-gated APIs are covered here too, not just the default set.

# The demo apps are excluded: `desktop-app` and `desktop-app-platform` both
# build a lib called `desktop_app`, so rustdoc writes them to the same path and
# refuses. They are demos, not published API, so the gate loses nothing.

# Build the docs for the published crates, denying every rustdoc warning.
doc:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features --exclude desktop-app --exclude desktop-app-platform --exclude xtask

# Open the docs locally.
doc-open:
    RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --workspace --all-features --exclude desktop-app --exclude desktop-app-platform --exclude xtask --open

# --- architecture budgets --------------------------------------------------

# The winit adapter is the one exception: winit itself requires a windowing
# backend to be selected, so it is checked per backend instead.

# Check that every crate still builds with no features on.
featureless:
    cargo build --workspace --no-default-features --exclude cranpose-platform-desktop-winit
    cargo check -p cranpose-platform-desktop-winit --no-default-features --features x11
    cargo check -p cranpose-platform-desktop-winit --no-default-features --features wayland
    cargo check -p cranpose --no-default-features
    cargo check -p cranpose-app-shell --no-default-features
    cargo check -p cranpose-testing --no-default-features
    cargo check --workspace --all-features

# Duplicate-dependency budget, resolved for every shipped target so the
# verdict is identical on every host. Any duplicate version family must either
# be collapsed or recorded as upstream debt in xtask; stale debt entries fail
# the gate too.
dep-budget:
    cargo xtask dependency-budget --explain

# Desktop binary size ceiling, measured against the isolated demo.
size-budget:
    scripts/ci/with_host_lock.sh --shared \
      cargo xtask binary-size --manifest-path apps/isolated-demo/Cargo.toml --package isolated-demo --bin isolated-demo --profile release-small --patch-workspace-cranpose --max-bytes 15728640

# Spell-check prose and identifiers. Runs in about a tenth of a second; the
# domain-term allowlist is in _typos.toml.

# Spell-check the repository.
#
# Installed on demand, the way the workflow provisions sccache and just. A
# gate that assumes a tool is already on the machine is a gate that passes or
# fails on which machine ran it: this one failed on a self-hosted mac with
# `typos: command not found` while passing everywhere the tool happened to be
# installed.
typos:
    @command -v typos >/dev/null || cargo install typos-cli --locked
    typos

# Workspace, lockfile and isolated-demo versions must agree.
versions:
    python3 scripts/check_cranpose_versions.py

# Everything the architecture-budget job enforces.
# This is the heavy job on the machine the robot suite measures on --
# `featureless` alone builds the workspace three ways -- so it takes the
# shared side of the host lock for its whole run rather than per recipe.

# Every architecture budget.
budgets:
    scripts/ci/with_host_lock.sh --shared just budgets-here

# The budgets themselves, once `budgets` holds the host lock.
budgets-here: featureless dep-budget size-budget

# --- builds ----------------------------------------------------------------

# Its default features already select the desktop platform and the wgpu
# renderer, so no --features is needed.

# Run the workspace demo.
run:
    cargo run -p desktop-app

# Run the starter template that depends only on published crates.
run-isolated:
    cargo run --manifest-path apps/isolated-demo/Cargo.toml --features desktop,renderer-wgpu

# Always `--release`: the unoptimised path skips the wasm size budget entirely,
# so a "passing" fast build proves nothing that CI actually checks.

# Build the web demo.
web:
    scripts/ci/with_host_lock.sh --shared apps/desktop-demo/build-web.sh --release

# Build the starter template for web, the way the publish canary does.
web-isolated:
    apps/isolated-demo/build-web.sh

# `--no-daemon` keeps a shared Gradle daemon on the self-hosted boxes from
# serving a foreign project's build.

# Build the Android demo.
android:
    cd apps/android-demo/android && ../../../scripts/ci/with_host_lock.sh --shared \
      ./gradlew --no-daemon :app:assembleRelease

# Build the Android release artifact, with the Rust side fully optimised.
android-release:
    cd apps/android-demo/android && ../../../scripts/ci/with_host_lock.sh --shared \
      ./gradlew --no-daemon :app:assembleRelease -PrustFastRelease=false

# Build the starter template for Android, the way the publish canary does.
android-isolated:
    apps/isolated-demo/android/gradlew -p apps/isolated-demo/android :app:assembleRelease

# Build the iOS simulator app bundle.
ios-sim:
    apps/ios-demo/ios/build-app.sh aarch64-apple-ios-sim

# Build the iOS device binary.
ios-device:
    cargo build -p desktop-app --bin cranpose-ios --target aarch64-apple-ios --no-default-features --features ios

# Boot a simulator and run the iOS demo on it.
ios-run:
    apps/ios-demo/ios/run-sim.sh

# --- robot end-to-end ------------------------------------------------------

# The full robot suite, as documented for local runs.
robot:
    ./run_robot_test.sh --sequential

# Compile every robot example without running any of them.
robot-build:
    ./run_robot_test.sh --build-only

# One robot example by name.
robot-one example:
    ./run_robot_test.sh --sequential --example {{example}}

# The three external-framebuffer captures are excluded here: a GPU swapchain
# under Xvfb never lands pixels in the X server's buffer, so they can only read
# their screenshots on software present.

# CI's GPU half of the robot suite.
robot-gpu:
    xvfb-run -a -s "-screen 0 1280x800x24" ./run_robot_test.sh \
      --sequential \
      --skip robot_underline_screenshot \
      --skip robot_text_strikeout_presented \
      --skip robot_leetcodedaily_full_layout_scroll_stability

# CI's software-present half: exactly the three captures excluded above.
robot-captures:
    WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1 \
      xvfb-run -a -s "-screen 0 1600x1200x24" ./run_robot_test.sh \
      --sequential \
      --example robot_underline_screenshot \
      --example robot_text_strikeout_presented \
      --example robot_leetcodedaily_full_layout_scroll_stability

# Render the liquid-glass cheatsheet montages.
cheatsheets:
    ./liquid_cheatsheets.sh

# --- performance -----------------------------------------------------------

# Criterion sanity pass: run each benchmark body once, measure nothing.
bench-smoke:
    cargo bench --package cranpose-ui --bench slot_table_v2 -- --test

# The slot-table Criterion suite with stable measurement settings.
bench-slot *args:
    ./perf_slot_table_v2.sh {{args}}

# Regression check against a same-tree baseline.
bench-slot-stability:
    ./perf_slot_table_v2.sh --stability-check

# Frame-rate budgets across the perf scenarios.
perf-fps *args:
    ./perf_robot_fps.sh {{args}}

# The heaviest scene the harness has -- a 400-row lazy list where every visible
# row blurs its own backdrop, under a glass card whose offset and blur radius
# animate every frame -- measured with vsync off and no budget, to read the
# ceiling the renderer can reach.

# Measure maximum achievable frame rate on the glass lazy list.
perf-max-fps duration="15":
    ./perf_robot_fps.sh --scenario glass_lazy_scroll --duration {{duration}} --report-only

# CPU profile of a perf scenario.
perf-cpu *args:
    ./perf_robot_cpu.sh {{args}}

# Heap profile of a perf scenario.
perf-heap *args:
    ./perf_robot_heap.sh {{args}}

# --- aggregates ------------------------------------------------------------

# Excludes the jobs that need a GPU, an Android SDK or an iOS toolchain.

# What a pull request is gated on. Run this before pushing.
ci: fmt-check typos versions test clippy doc budgets

# Needs a Linux box with the X11 stack, an Android SDK and (on macOS) Xcode.

# Every gate, including the platform builds and the robot suite.
ci-full: ci clippy-wasm clippy-ios clippy-android web android robot
