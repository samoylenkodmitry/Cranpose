# Cranpose Test Tools

`cranpose-testing` provides public test helpers for Cranpose compositions and app behavior. `ComposeTestRule` composes UI in memory; semantic queries and accessibility audits inspect the current tree. The robot API drives input against a headless or desktop app shell.

## Test a composition

Add `cranpose-testing` as a development dependency with `cargo add cranpose-testing --dev`. Use `ComposeTestRule` for layout, state, semantics, and accessibility checks in an in-memory composition:

```rust
use cranpose::prelude::*;
use cranpose_testing::ComposeTestRule;

#[composable]
fn Greeting() {
    Text("Ready", Modifier::empty(), TextStyle::default());
}

let mut rule = ComposeTestRule::new();
rule.set_content(Greeting).expect("content composes");
let screen = rule
    .placed_semantics(Size::new(320.0, 120.0))
    .expect("layout succeeds")
    .expect("screen has content");
assert!(screen
    .flatten()
    .iter()
    .any(|node| node.label.as_deref() == Some("Ready")));
```

`placed_semantics` returns labels and bounds for a chosen viewport. `assert_accessible` checks the placed tree, and `audit_accessibility` returns structured issues for custom summaries.

## Run robot tests

The `desktop-robot` feature enables helper queries and assertions. The app test target also selects `cranpose/robot`, `desktop`, and a renderer such as `renderer-wgpu`. A robot runner starts `AppLauncher` with `with_headless(true)` and supplies a `with_test_driver` closure. The driver waits for app work, queries semantics, sends input, and exits the app.

```sh
cargo add cranpose-testing --dev --features desktop-robot
```

Linux desktop robot runs need a display server; CI can run the test through `xvfb-run`. Headless composition tests use an in-memory applier and run on the CPU. The repository's [`run_robot_test.sh`](https://github.com/samoylenkodmitry/Cranpose/blob/main/run_robot_test.sh) runs repository robot scenarios.

## Links

- [API documentation](https://docs.rs/cranpose-testing/latest/cranpose_testing/)
- [App shell test APIs](https://docs.rs/cranpose-app-shell/latest/cranpose_app_shell/)
- [Source](https://github.com/samoylenkodmitry/Cranpose/tree/main/crates/cranpose-testing)
- [Test guide](https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/guide.md#testing)
