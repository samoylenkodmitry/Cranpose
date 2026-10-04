# Cranpose tests

Tools for Cranpose layout, semantics and app behavior through public entry
points. The crate provides semantic-tree queries,
robot interaction helpers, and accessibility audits.

## Robot runners

Enable `cranpose-testing/desktop-robot` in the runner package; this feature
enables `cranpose/robot`. A runner starts the app with
`AppLauncher::with_headless(true)` and supplies a
`with_test_driver` closure. The closure waits for the app, queries semantics,
injects input and exits. The root `run_robot_test.sh` script discovers and runs the
repository's robot examples; use `./run_robot_test.sh --help` for its current
options.

The convenience functions such as `find_in_semantics`, `find_text` and
`find_button` are gated by this crate's `desktop-robot` feature. The lower-level
`Robot` API is re-exported from `cranpose` when its `robot` feature is enabled.

## Accessibility audits

`assert_accessible` checks a placed semantics tree for issues such as unnamed
controls, duplicate names, small targets and missing screen titles. Use
`audit_accessibility` when the caller needs the issues as data instead of a
panic. `Robot::assert_accessible` and `ComposeTestRule::assert_accessible`
provide harness-specific entry points.
