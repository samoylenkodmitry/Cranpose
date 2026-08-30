# Pre-existing complexity and duplication debt, first made visible by #539

`#539` added `just complexity-gate` and `just duplication-gate`, both scoped
to the lines a diff actually touches (`scripts/ci/gate_diff.py`). Scoped
gates only ever check what a PR's own diff reaches, by design -- so nothing
here is a regression from any specific commit. It is debt that predates the
gate and was never in scope for any earlier PR to fix, made visible for the
first time because `docs/comment-policy`'s repo-wide comment-strip diff
happens to touch a line inside nearly every function in the tree, including
these.

Confirmed genuine, not a scoping artifact: `fix/gate-diff-comment-scoping`
(#568) fixed the one real bug in the scoping heuristic itself (comment-only
line changes wrongly counted as touching a function) and re-ran both gates
against the same diff afterward. The list below is unchanged by that fix --
these functions and clones are pulled into scope by a real code-text change
next to the stripped comments, most often a trivial structural collapse an
earlier comment-removal pass left behind (an emptied `if` body merging to
`{}`), not by the comment removal itself.

## Complexity: 37 functions over the limit (20)

The two largest are worth naming on their own: `crates/cranpose/src/web.rs`'s
`run` at cyclomatic complexity **224**, and `crates/cranpose/src/android.rs`'s
`run` at **174** -- both roughly an order of magnitude over the ceiling every
other function here is held to.

```
apps/desktop-demo/robot-runners/robot_copy_paste.rs:7-284 main (46)
apps/desktop-demo/robot-runners/robot_copy_paste.rs:16-280 <anonymous> (44)
apps/desktop-demo/robot-runners/robot_fling.rs:10-215 main (35)
apps/desktop-demo/robot-runners/robot_fling.rs:21-211 <anonymous> (33)
apps/desktop-demo/robot-runners/robot_fling_edge_cases.rs:40-324 main (32)
apps/desktop-demo/robot-runners/robot_fling_edge_cases.rs:50-320 <anonymous> (30)
apps/desktop-demo/robot-runners/robot_fling_precise.rs:10-303 main (27)
apps/desktop-demo/robot-runners/robot_fling_precise.rs:20-299 <anonymous> (25)
apps/desktop-demo/robot-runners/robot_lazy_list_order_bug.rs:7-222 main (37)
apps/desktop-demo/robot-runners/robot_lazy_list_order_bug.rs:18-218 <anonymous> (35)
apps/desktop-demo/robot-runners/robot_lazy_tab_test.rs:7-201 main (34)
apps/desktop-demo/robot-runners/robot_lazy_tab_test.rs:15-199 <anonymous> (33)
apps/desktop-demo/robot-runners/robot_shader_rect.rs:150-315 main (28)
apps/desktop-demo/robot-runners/robot_shader_rect.rs:158-313 <anonymous> (27)
apps/desktop-demo/robot-runners/robot_subcompose_invalidation.rs:9-105 test_app (21)
apps/desktop-demo/robot-runners/robot_text_input.rs:7-647 main (114)
apps/desktop-demo/robot-runners/robot_text_input.rs:18-643 <anonymous> (112)
apps/desktop-demo/robot-runners/robot_text_loupe.rs:43-328 main (25)
apps/desktop-demo/robot-runners/robot_text_loupe.rs:57-319 <anonymous> (21)
crates/cranpose-app-shell/src/shell_frame.rs:186-329 run_layout_phase_in_context (33)
crates/cranpose-liquid/src/widgets/tab_bar.rs:517-757 LiquidTabBarLayout (33)
crates/cranpose-liquid/src/widgets/tab_bar.rs:537-755 <anonymous> (32)
crates/cranpose-liquid/src/widgets/tab_bar.rs:575-744 <anonymous> (21)
crates/cranpose-render/wgpu/src/render.rs:2456-2619 convert_shapes_into_outputs (22)
crates/cranpose-render/wgpu/src/render.rs:10513-10868 encode_shadow_draw (38)
crates/cranpose-ui/src/layout/mod.rs:1274-1529 try_measure_subcompose (37)
crates/cranpose-ui/src/layout/mod.rs:1682-1863 measure_layout_node (24)
crates/cranpose-ui/src/modifier/scroll.rs:426-556 on_move (31)
crates/cranpose-ui/src/tests/async_runtime_full_layout_test.rs:49-239 async_runtime_full_layout (22)
crates/cranpose-ui/src/tests/modifier_nodes_tests.rs:1015-1146 custom_layout_modifier_works_through_retained_chain (27)
crates/cranpose-ui/src/tests/modifier_nodes_tests.rs:1198-1325 stateful_measure_uses_live_retained_node_state (21)
crates/cranpose-ui/src/tests/renderer_tests.rs:101-225 renderer_translates_draw_commands (37)
crates/cranpose-ui/src/text_field_modifier_node.rs:453-645 create_handler (27)
crates/cranpose/src/android.rs:1494-2438 run (174)
crates/cranpose/src/android.rs:1737-2023 <anonymous> (50)
crates/cranpose/src/web.rs:111-924 run (224)
crates/cranpose/src/web.rs:592-666 <anonymous> (50)
```

(Line ranges are as measured on `docs/comment-policy`'s branch tip at the
time this was recorded, 2026-08-30; comment stripping shifts line numbers
relative to `main`, so re-measure with `just complexity-gate` against
whichever base is current rather than trusting these ranges to still line up.)

## Duplication: 21 clones touching the diff

```
apps/desktop-demo/robot-runners/robot_double_click.rs:57-71 <-> apps/desktop-demo/robot-runners/robot_double_click.rs:106-118 (15 lines)
apps/desktop-demo/robot-runners/robot_lazy_items_rc.rs:18-30 <-> apps/desktop-demo/robot-runners/robot_lazy_varheight_lifecycle.rs:40-52 (13 lines)
apps/desktop-demo/robot-runners/robot_lazy_list.rs:24-38 <-> apps/desktop-demo/robot-runners/robot_tab_navigation.rs:20-34 (15 lines)
apps/desktop-demo/robot-runners/robot_lazy_tab_test.rs:12-28 <-> apps/desktop-demo/robot-runners/robot_tab_navigation.rs:12-33 (17 lines)
apps/desktop-demo/robot-runners/robot_shader_rect.rs:258-269 <-> apps/desktop-demo/robot-runners/robot_shader_rect.rs:277-288 (12 lines)
crates/cranpose-animation/src/tests/animation_tests.rs:62-73 <-> crates/cranpose-animation/src/tests/color_tests.rs:104-115 (12 lines)
crates/cranpose-core/src/snapshot_v2/global.rs:66-78 <-> crates/cranpose-core/src/snapshot_v2/mutable.rs:142-154 (13 lines)
crates/cranpose-core/src/snapshot_v2/global.rs:93-121 <-> crates/cranpose-core/src/snapshot_v2/nested.rs:186-212 (29 lines)
crates/cranpose-core/src/snapshot_v2/global.rs:95-109 <-> crates/cranpose-core/src/snapshot_v2/mutable.rs:197-211 (15 lines)
crates/cranpose-core/src/snapshot_v2/mutable.rs:451-464 <-> crates/cranpose-core/src/snapshot_v2/nested.rs:278-292 (14 lines)
crates/cranpose-core/src/snapshot_v2/readonly.rs:59-85 <-> crates/cranpose-core/src/snapshot_v2/transparent.rs:261-288 (27 lines)
crates/cranpose-core/src/tests/composer_applier_tests.rs:573-584 <-> crates/cranpose-core/src/tests/composer_applier_tests.rs:1622-1634 (12 lines)
crates/cranpose-core/src/tests/composer_applier_tests.rs:1618-1628 <-> crates/cranpose-core/src/tests/composer_applier_tests.rs:1644-1654 (11 lines)
crates/cranpose-render/pixels/src/style.rs:23-34 <-> crates/cranpose-render/wgpu/src/pipeline/style.rs:22-33 (12 lines)
crates/cranpose-render/wgpu/src/render.rs:10817-10841 <-> crates/cranpose-render/wgpu/src/render.rs:11110-11134 (25 lines)
crates/cranpose-testing/tests/pointer_hover_gradient_test.rs:41-60 <-> crates/cranpose-testing/tests/pointer_input_end_to_end_test.rs:37-56 (20 lines)
crates/cranpose-ui/src/layout/policies.rs:538-549 <-> crates/cranpose-ui/src/layout/policies.rs:561-572 (12 lines)
crates/cranpose-ui/src/modifier_nodes.rs:1885-1904 <-> crates/cranpose-ui/src/modifier_nodes.rs:2020-2039 (20 lines)
crates/cranpose-ui/src/subcompose_layout.rs:1264-1298 <-> crates/cranpose-ui/src/widgets/nodes/layout_node.rs:962-996 (35 lines)
crates/cranpose-ui/src/tests/anchor_async_tests.rs:64-96 <-> crates/cranpose-ui/src/tests/async_runtime_full_layout_test.rs:79-116 (33 lines)
crates/cranpose-ui/src/tests/swipe_to_dismiss_tests.rs:277-287 <-> crates/cranpose-ui/src/tests/swipe_to_dismiss_tests.rs:315-323 (11 lines)
```

## What this is not

Not `docs/comment-policy`'s job to fix -- that PR strips comments, and
fixing 37 functions' control flow or de-duplicating 21 code blocks is a
separate, substantial architecture pass with its own review surface.
Whoever picks this up should re-run `just complexity-gate --base <sha>` /
`just duplication-gate --base <sha>` against a throwaway diff (e.g. touch one
blank line in each file so the whole function is back in scope) to get
current line numbers before starting, rather than trusting the ranges above.
