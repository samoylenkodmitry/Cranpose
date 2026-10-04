# Robot tests

Cranpose's desktop robot drives a real app through `AppLauncher` and exposes
rendered semantics to the driver. Headless end-to-end runners check observable
app behavior through public entry points.

## Run the demo runners

All runners are modules of the `robot` Cargo example in
`apps/desktop-demo/robot-runners/`. `main.rs` registers their names, and the
root `run_robot_test.sh` builds and launches each runner in the configured
headless test environment:

```sh
# Run one runner
./run_robot_test.sh --example robot_interactive

# Run the full suite sequentially
./run_robot_test.sh --sequential

# List runner scheduling classes
./run_robot_test.sh --list-classes
```

`run_robot_test.sh` acquires its host lock. Run `./run_robot_test.sh --help`
for its other options. `cargo xtask test-layout` checks runner modules against
the registration table.

The crate-level `Robot` API and the `cranpose-testing` E2E helpers appear in
[`cranpose-testing`](../crates/cranpose-testing/README.md). The demo runner
target uses `desktop`, `renderer-wgpu`, and `robot-app`; the
`cranpose-testing` dev dependency enables Cranpose's `robot` feature.

## Add a runner

Add a `robot_<name>.rs` module under `apps/desktop-demo/robot-runners/` and
add the runner name to `main.rs`. Launch the app with `robot_launch::launch`, interact
through semantics or coordinates, check visible state, and exit cleanly. Use
exact semantic matches when labels overlap. For example:

```rust
robot.click_by_text("Increment")?;
robot.validate_content("Counter: 1")?;
robot.exit()?;
```

`click_by_text` finds a clickable semantic node with the requested text
and clicks its center. Use `find_button_bounds_exact` for an exact label,
`get_semantics` to inspect the accessibility tree. Call
`set_semantics_enabled(true)` before `spoken_tree` to inspect screen-reader
order and actions. Use `wait_for_idle` for settled screens. For animated
content, poll for the target semantic state.

`Robot::screenshot` redraws the scene offscreen. For windowed presentation or
external-capture assertions, use the existing windowed robot patterns and
capture tools described in [render verification](render_verification.md).
