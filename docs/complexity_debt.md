# Pre-existing complexity and duplication debt, first made visible by #539

`#539` added `just complexity-gate` and `just duplication-gate`, both scoped
to the lines a diff actually touches (the `gate_diff` module in
`xtask/src/main.rs`). Scoped
gates only ever check what a PR's own diff reaches, by design -- so nothing
here is a regression from any specific commit. It is debt that predates the
gate and was never in scope for any earlier PR to fix, made visible for the
first time because `docs/comment-policy`'s repo-wide comment-strip diff
happens to touch a line inside nearly every function in the tree, including
these.

## Two gate bugs found and fixed while investigating this list

**Comment-only and collapse-only touches (`fix/gate-diff-comment-scoping`,
#568).** The original scoping counted any hunk with a code byte on its new
side as "touching" a function, including one where deleting a comment also
collapsed the surrounding code onto fewer lines (`}) {\n // comment\n}`
becoming `}) {}`). #568 replaced that with a token-level comparison of each
hunk's old and new side, so a hunk with no code-level difference -- comment
gone, whitespace collapsed, a grammar-insignificant trailing comma added or
dropped -- no longer counts as touching anything. Re-running against
`docs/comment-policy`'s diff after that fix took the count from 37/21 down
to 6/2: real progress, but not a full fix, because the remaining hunks are
genuinely-mandatory rewrites -- a `clippy::single_match` or
`clippy::collapsible_if` autofix forced once a branch becomes a no-op, or
`rustfmt` collapsing a now-single-expression block -- that #568, by design,
does not exempt: those *are* a real code-text difference, even though the
only reason this diff produced them is that the comment inside had to go.

**Absolute-limit scoping, and a double-count inside it
(`fix/complexity-duplication-before-after`).** Two further problems, found
while ranking that remaining 6/2 by how proportionate fixing each would be:

- The complexity gate was comparing a touched function's complexity against
  the limit directly, with no memory of what it was *before* the diff. A
  function already over the limit -- most of the list below -- failed
  every gate run that so much as brushed a nearby comment, demanding a
  refactor (in `android.rs::run`'s case, from 174 down to 20) as the price
  of deleting a comment, regardless of whether the diff made the function's
  own control flow any worse. The gate now compares before and after:
  unchanged-or-lower complexity passes regardless of the absolute number,
  higher fails regardless of it. Duplication got the same treatment --
  `jscpd` is re-run against the pre-change content of every file a
  candidate clone involves, and a clone already comparably present before
  the diff passes as existing debt rather than failing as newly introduced.
- Separately, and independent of that fix: `rust-code-analysis-cli` reports
  a closure as its own `function`-kind entry nested inside the function
  that defines it, and the gate was flattening that whole tree into a flat
  violation list -- so a function built almost entirely out of one large
  closure (`main` wrapping a `poll_events` callback, `run` wrapping an
  event-loop closure) was reported *twice*, once under its own name and
  once as `<anonymous>`, each treated as an independent violation to fix.
  It is one function, reported as two. The gate now stops descending into
  a closure once it is already inside some enclosing function (a named
  `fn` nested inside a closure body is still independently real code and
  still gets its own entry, however deep the nesting) so a closure with
  nothing of its own to fold into -- the rare top-level case -- is still
  reported.

That double-count is why the complexity list below shrank from the 37
originally measured to 24: 13 of the original 37 entries were an
`<anonymous>` nested inside a `main` or `run` already on the list, counted
as a second problem when it was the same one. The 24 remaining are each an
independently real function; none of them merged away.

With both fixes in place, `docs/comment-policy`'s diff passes
`complexity-gate` and `duplication-gate` cleanly (0 violations): it does not
raise any already-over-limit function's complexity, and it does not
introduce any new duplication between the files it touches. The debt below
is unaffected by that -- these functions and clones are exactly as complex
and exactly as duplicated as they were before any of this, still real,
still nobody's job to fix as a side effect of a comment sweep.

## Complexity: 24 functions over the limit (20)

The two largest are worth naming on their own: `crates/cranpose/src/web.rs`'s
`run` at cyclomatic complexity **224**, and `crates/cranpose/src/android.rs`'s
`run` at **174** -- both roughly an order of magnitude over the ceiling every
other function here is held to. Both numbers already include their own
nested closures' complexity (see above) -- there is no separate `<anonymous>`
entry to also fix, because there is no separate problem.

```
apps/desktop-demo/robot-runners/robot_copy_paste.rs:7-284 main (46)
apps/desktop-demo/robot-runners/robot_fling.rs:10-215 main (35)
apps/desktop-demo/robot-runners/robot_fling_edge_cases.rs:40-324 main (32)
apps/desktop-demo/robot-runners/robot_fling_precise.rs:10-303 main (27)
apps/desktop-demo/robot-runners/robot_lazy_list_order_bug.rs:7-222 main (37)
apps/desktop-demo/robot-runners/robot_lazy_tab_test.rs:7-201 main (34)
apps/desktop-demo/robot-runners/robot_shader_rect.rs:150-315 main (28)
apps/desktop-demo/robot-runners/robot_subcompose_invalidation.rs:9-105 test_app (21)
apps/desktop-demo/robot-runners/robot_text_input.rs:7-647 main (114)
apps/desktop-demo/robot-runners/robot_text_loupe.rs:43-328 main (25)
crates/cranpose-app-shell/src/shell_frame.rs:186-329 run_layout_phase_in_context (33)
crates/cranpose-liquid/src/widgets/tab_bar.rs:517-757 LiquidTabBarLayout (33)
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
crates/cranpose/src/web.rs:111-924 run (224)
```

(Line ranges are as measured on `docs/comment-policy`'s branch tip at the
time this was recorded, 2026-08-30; comment stripping shifts line numbers
relative to `main`, so re-measure with `just complexity-gate` against
whichever base is current rather than trusting these ranges to still line up.)

## Duplication: 21 clones touching the diff

Unaffected by the nested-closure double-count above (that bug was specific
to how the complexity gate flattened `rust-code-analysis-cli`'s function
tree; `jscpd`'s clone list has no equivalent structure to over-count). All
21 are still real, pre-existing duplication; two of them
(`pointer_hover_gradient_test.rs` / `pointer_input_end_to_end_test.rs`, and
the `policies.rs` `min_intrinsic_height` / `max_intrinsic_height` pair) are
specifically confirmed pre-existing rather than merely plausible: `jscpd`
run against `docs/comment-policy`'s pre-change content independently found
each pair already duplicating itself before the diff.

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
fixing 24 functions' control flow or de-duplicating 21 code blocks is a
separate, substantial architecture pass with its own review surface. Nor is
it something the gate should silently force: the gate compares before and
after specifically so that touching a line near one of these does not
retroactively demand fixing it. Whoever picks this debt up deliberately
should re-run `just complexity-gate --base <sha>` / `just duplication-gate
--base <sha>` against a throwaway diff (e.g. touch one blank line in each
file so the whole function is back in scope) to get current line numbers
before starting, rather than trusting the ranges above.
