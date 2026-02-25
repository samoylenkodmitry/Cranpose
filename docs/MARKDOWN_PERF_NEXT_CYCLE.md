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

## Continuation Findings (2026-02-25, later run)

- Re-ran the documented first-command sequence:
  - no-drag baseline (`hold 5s`) still reports `fps ~5.5`, `frame_ms ~181-183`.
  - only the initial two lazy time-budget warnings appear (`index 8`, `index 23`), then no steady-state warning spam.
  - direction-change viewport drag repro still ends with:
    - `after_viewport_up: deep_present=true`
    - `return_present=false`
    - `fps_summary ~36-40`.
  - `perf record/report` for the direction-change run still shows hot symbols dominated by text/font parsing paths (`ttf_parser`-heavy symbols remain present in top rows).

- Added/kept WGPU text optimizations and probes:
  - `prepare_with_options` prepared-layout cache (already landed in this branch) remains active and test-covered.
  - Added a WGPU `prepare_with_options` fast path for **plain annotated text** in the common markdown config (`soft_wrap + Clip + unlimited max_lines + Simple/None`):
    - wraps using glyphon line runs directly;
    - falls back to existing generic algorithm when constraints are not matched.
  - Added renderer-side text prepare telemetry hook (env gated):
    - `CRANPOSE_TEXT_RENDER_TELEMETRY=1`.
  - Added text-batch signature reuse path in `GpuRenderer::prepare_text_for_render` to skip identical consecutive prepares.

- What the new telemetry showed:
  - `CRANPOSE_TEXT_MEASURE_TELEMETRY=1` (no-drag) reported at startup:
    - `measure_calls=200`
    - `size_hit_rate=64.5%`
    - `text_cache_hit_rate=1.4%`
    - `reshape_rate=98.6%`
  - `CRANPOSE_TEXT_RENDER_TELEMETRY=1` did not emit periodic logs in the no-drag hold runs, which suggests renderer-side text-prepare path is not the dominant steady-state loop in this robot scenario (or is hit far less than expected).

- Control check:
  - Small markdown fixture (`/tmp/markdown_small.md`) reaches ~`17.6 FPS` in the same headless robot, confirming the large-fixture path is still the limiting workload.

## Fresh Chat Handoff (next perf-opt cycle)

### Workspace state to continue from

- Modified files:
  - `crates/cranpose-render/wgpu/src/lib.rs`
  - `crates/cranpose-render/wgpu/src/render.rs`
  - `crates/cranpose-ui/src/text/measure.rs`
  - `crates/cranpose-render/common/src/software_text_raster.rs` (pre-existing in workspace)
  - `crates/cranpose-render/common/src/text_hyphenation.rs` (pre-existing in workspace)
  - `crates/cranpose-render/pixels/src/draw.rs` (pre-existing in workspace)
  - `docs/MARKDOWN_PERF_NEXT_CYCLE.md`
- Untracked artifacts:
  - `clippy_out.txt`
  - `robot_out.txt`
  - `test_out.txt`

### What is ready

- Lazy scroll direction-reversal backlog mitigation is implemented and test-covered.
- Markdown robot runner can now drive viewport drags directly and emit sentinel position traces.
- Baseline no-drag run does not show uncontrolled index drift in this workspace.
- WGPU has additional prepare/layout fast paths plus env-gated renderer telemetry for the next measurement pass.

### What remains open

- User-reported manual bug: after dragging down to around `"19.02.2026"`, upward drag can snap back and feel blocked.
- Robot probes did not reproduce a hard lock after the backlog fix; manual verification on the exact user gesture path is still required.
- Main runtime bottleneck is still text shaping/layout at deep content positions; FPS target (`8+`) is not yet met in no-drag baseline.
- New uncertainty: no-drag steady-state cost likely is not dominated by repeated renderer-side `prepare_text_for_render`; next cycle should instrument app-shell frame phases to isolate where the `~181ms` frame time is spent after initial settle.

### Recommended first commands in next chat

1. Build and run baseline no-drag profile:
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_MARKDOWN_TOP_SENTINEL="Daily leetcode challenge" CRANPOSE_MARKDOWN_DEEP_SENTINEL="" CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_HOLD_SECS=5 CRANPOSE_LAZY_MEASURE_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`
2. Reproduce drag direction behavior with viewport drags:
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_MARKDOWN_TOP_SENTINEL="Daily leetcode challenge" CRANPOSE_MARKDOWN_DEEP_SENTINEL="19.02.2026" CRANPOSE_MARKDOWN_RETURN_SENTINEL="Daily leetcode challenge" CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_VIEWPORT_DRAG_DOWN_LOOPS=20 CRANPOSE_MARKDOWN_VIEWPORT_DRAG_UP_LOOPS=20 CRANPOSE_MARKDOWN_WAIT_IDLE_AFTER_DRAG=0 CRANPOSE_LAZY_MEASURE_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`
3. If still stuck, capture CPU profile for that exact run:
   - `perf record -F 299 -g --call-graph fp -o /tmp/mdperf_dirchange.data bash -lc '...robot_markdown_scrollbar env from step 2...'`
   - `perf report --stdio -i /tmp/mdperf_dirchange.data`
4. Run startup/steady-state measurement probes:
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_HOLD_SECS=8 CRANPOSE_TEXT_MEASURE_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_HOLD_SECS=8 CRANPOSE_TEXT_RENDER_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`

### Priority for next cycle

- Keep measurement-first flow:
  - verify if any remaining jump-back comes from queued deltas, fling handoff, or scrollbar `scroll_to_item` interference;
  - then isolate steady-state frame time inside app-shell phases (`layout`, queue dispatch, scene rebuild, renderer submit), since current evidence suggests startup text shaping is still expensive but may not explain all of steady-state `frame_ms ~181`.
