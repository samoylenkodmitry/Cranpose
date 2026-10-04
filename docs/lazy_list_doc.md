# Lazy lists

`LazyColumn` and `LazyRow` compose and measure the items needed for the current
viewport and the list's beyond-bounds window. The public widgets and item DSL
are in [`lazy_list.rs`](../crates/cranpose-ui/src/widgets/lazy_list.rs) and
[`lazy_list_scope.rs`](../crates/cranpose-foundation/src/lazy/lazy_list_scope.rs).

## State, measurement and reuse

`LazyListState` holds the scroll position, visible-item information and
scroll-to-item requests. The measure policy reads intervals from the content,
measures the visible range, fills the beyond-bounds window and places the
result. Item keys preserve identity when items move; content types constrain
which subcompose slots can be reused. The measure algorithm and state live in
[`lazy_list_measure.rs`](../crates/cranpose-foundation/src/lazy/lazy_list_measure.rs)
and [`lazy_list_state.rs`](../crates/cranpose-foundation/src/lazy/lazy_list_state.rs).
Subcomposition owns slot reuse and disposal in
[`subcompose.rs`](../crates/cranpose-core/src/subcompose.rs).

Layout submits prefetch requests, and the app shell services them during
eligible idle time through the ordinary list measurement path. The app shell
schedules frames. See [`lazy_prefetch.rs`](../crates/cranpose-ui/src/lazy_prefetch.rs)
and [`shell_frame.rs`](../crates/cranpose-app-shell/src/shell_frame.rs).

## Verification entry points

The desktop robot runner accepts named examples through `run_robot_test.sh`.
For example:

```sh
./run_robot_test.sh --sequential --example robot_lazy_list --example robot_lazy_lifecycle
```

The integration cases for list composition are under
[`crates/cranpose-ui/tests`](../crates/cranpose-ui/tests), while shell-level
scroll and lifecycle behavior is covered under
[`crates/cranpose-app-shell/src/tests`](../crates/cranpose-app-shell/src/tests).
Use the repository's current host/build workflow for either suite.
