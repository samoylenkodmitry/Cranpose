# Markdown Perf: Next Cycle Context + Plan

## Current Context (2026-02-25)

- Workload: remote markdown file at ~2.5MB (`2619541` bytes).
- Parser cost is acceptable: `markdown_to_blocks` profile run is ~`167.7ms` for the full file.
- The runtime bottleneck is not parser time; it is lazy-list item measurement + text shaping.
- Runtime signal: repeated `Lazy list measurement exceeded time budget (50ms)` warnings.
- Perf samples point heavily into `ttf_parser`/font layout paths during text measurement.
- Baseline runtime sample (headless robot): around `1 FPS` to `2 FPS` steady-state for this huge doc.
- Extra symptom to resolve: index progression appears even with no explicit drag loop in robot runs.

## Existing Instrumentation

- Parser profile test:
  - `MD_PROFILE_PATH=/tmp/markdown_profile.md cargo test -p desktop-app app::markdown::tests::profile_large_markdown_from_file -- --ignored --nocapture`
- Markdown robot runner (fixture + FPS):
  - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_HEADLESS=1 target/debug/examples/robot_markdown_scrollbar`
- CPU profiling:
  - `perf record -F 299 -g --call-graph fp -o /tmp/mdperf.data ...`
  - `perf report --stdio -i /tmp/mdperf.data`

## Changes Already Landed (for continuity)

- Markdown parser/list correctness fixes and regression tests.
- `Vec<MarkdownBlock>` -> `Rc<[MarkdownBlock]>` rendering path.
- Large markdown text block splitting (`MAX_MARKDOWN_BLOCK_BYTES=1200`).
- Markdown list `beyond_bounds_item_count = 0`.
- Scrollbar drag throttling/coalescing.
- WGPU resolver cache for style/weight resolution.
- Lazy item main-axis size clamp to minimum 1px.
- Profiling-capable markdown robot runner with env knobs and FPS output.

## Next Cycle Plan (Measure -> Plan -> Refactor -> Analyze -> Measure)

1. Measure phase A (high-fidelity telemetry)
- Add temporary counters/logs for each lazy measurement pass:
  - start index, measured item count, accumulated viewport fill height, elapsed time.
  - first visible index before/after pass, scroll delta, and whether pointer-driven.
- Add temporary counters for text measure/layout calls per frame and cache hit/miss rates.

2. Plan phase A (choose highest leverage refactor from measured truth)
- Decide if runaway cost is primarily:
  - a) unintended scroll/index drift,
  - b) underfilled viewport retries due many tiny measured items,
  - c) expensive text shaping per newly measured item,
  - d) mixed.

3. Refactor phase A (target highest root cause)
- If index drift exists with zero user scroll, fix scroll-position mutation source first.
- Introduce incremental measurement contract when time budget is hit:
  - prevent pathological repeated scan behavior.
  - keep stable first-visible anchor until viewport is truly filled.
- Add a markdown-specific fast-path for measurement:
  - use cached/approximate line metrics for first-pass size estimation.
  - defer expensive exact shaping until item is actually in visible draw set.

4. Analyze phase A
- Re-run identical fixture scenario.
- Compare:
  - FPS,
  - warnings/sec,
  - number of items measured per pass,
  - text-shaping calls/frame,
  - cache hit rates.

5. Measure phase B (regression + ceiling)
- Repeat robot + perf runs.
- Validate no behavior regressions (scrollbar drag, links, list rendering).
- Lock in with one performance regression test or thresholded benchmark.

## Success Criteria for the Cycle

- No uncontrolled first-visible index drift when there is no input.
- `Lazy list measurement exceeded time budget` warnings reduced to near-zero in steady state.
- Headless markdown fixture run shows clear FPS lift from current (~1-2 FPS) to at least `8+ FPS`.
- Interactions (dragging scrollbar) remain correct and stable.

## Cycle Findings (2026-02-25)

- Added lazy-pass telemetry (`CRANPOSE_LAZY_MEASURE_TELEMETRY=1`) in:
  - `LazyListState::scroll_to_item` / `dispatch_scroll_delta`
  - `ItemMeasurer::measure_all` pass summary
  - `measure_lazy_list` cycle summary
- Added text-cache telemetry (`CRANPOSE_TEXT_MEASURE_TELEMETRY=1`) in `WgpuTextMeasurer`:
  - `measure` / `layout` / `get_offset_for_position` call counters
  - size-cache hit/miss
  - text-buffer cache hit/miss
  - reshape vs reuse ratio

### What telemetry showed

- Deep index climb is input-driven during scrollbar drag:
  - Each climb aligns with explicit `scroll_to_item` requests from scrollbar move handling.
  - In `CRANPOSE_MARKDOWN_SCROLL_LOOPS=0` runs, first-visible position remains anchored at top; no uncontrolled internal drift was observed in this workspace.
- Text path remains expensive under deep jumps:
  - size-cache hit rate is high (~85%)
  - text-buffer cache hit rate is low (<1%)
  - reshape ratio is very high (~99%)
  - Interpretation: wrapping/measurement creates many unique text-shaping keys for previously unseen segments.

### Root-cause fix landed

- Fixed lazy-list time-budget truncation fallback when a pass times out before reaching any visible item:
  - Previous behavior could collapse offset to `0` at the measured start index.
  - New behavior carries forward unresolved progress (`next_index` + remaining offset) so the next pass continues scanning correctly.
- Added regression test:
  - `lazy::lazy_list_measure::tests::test_time_budget_progresses_when_visible_item_not_reached`
- Added a WGPU text measurement fast path for the common markdown case:
  - `measure_with_options` now uses a width-constrained glyphon pass directly when options are `soft_wrap + Clip + unlimited max_lines` and paragraph style is `Simple/None`.
  - Added regression test:
    - `tests::measure_with_options_fast_path_wraps_to_width`

## Follow-up Findings (2026-02-25, same cycle)

- Added markdown robot viewport-drag telemetry controls (for direct list drag repros):
  - `CRANPOSE_MARKDOWN_VIEWPORT_DRAG_DOWN_LOOPS`
  - `CRANPOSE_MARKDOWN_VIEWPORT_DRAG_UP_LOOPS`
  - `CRANPOSE_MARKDOWN_VIEWPORT_DRAG_FROM_FRAC`
  - `CRANPOSE_MARKDOWN_VIEWPORT_DRAG_TO_FRAC`
  - `CRANPOSE_MARKDOWN_STOP_ON_DEEP`
  - `CRANPOSE_MARKDOWN_RETURN_SENTINEL`
- Deep rail-jump + viewport-up probes show reverse drag is applied (index decreases), but can still require many drags when starting very deep.
- Implemented lazy scroll input backlog fix for direction reversal:
  - In `LazyListState::dispatch_scroll_delta`, when pending unconsumed delta sign differs from new gesture delta sign, stale backlog is dropped and replaced with the latest delta.
  - Rationale: avoid "snap back" from old-direction backlog on slow frames.
  - Added tests:
    - `dispatch_scroll_delta_accumulates_same_direction`
    - `dispatch_scroll_delta_drops_stale_backlog_on_direction_change`
- Updated no-drag baseline (`CRANPOSE_MARKDOWN_SCROLL_LOOPS=0`, hold 5s):
  - `fps_summary: fps=5.4` to `5.5`
  - `frame_ms ~= 183`
  - `recompositions=3` over 5 seconds
  - time-budget warnings observed only during initial settle (`2` warnings), no steady-state drift in this run.
