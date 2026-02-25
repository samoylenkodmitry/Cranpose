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

## Continuation Findings (2026-02-25, rebuilt robot binary)

- Important measurement fix:
  - Running `target/debug/examples/robot_markdown_scrollbar` directly can use a stale binary after library-only changes.
  - Rebuilt explicitly with:
    - `cargo build -p desktop-app --example robot_markdown_scrollbar --features robot-app`
  - Post-rebuild measurements changed materially.

- Updated baseline no-drag (`hold 5s`, same 2.6MB fixture):
  - `fps_summary: fps=9.2 frame_ms=108.95 recompositions=3`
  - still only two startup lazy warnings (`index 8`, `index 23`), no steady-state warning spam.
  - This now meets the cycle `8+ FPS` baseline target.

- Updated direction-change viewport drag repro (`20 down + 20 up`, no telemetry):
  - `fps_summary: fps=37.4 frame_ms=26.74 recompositions=1719`
  - `after_viewport_up: deep_present=true`
  - `return_present=false` remains unresolved.

- Added richer text telemetry counters in `WgpuTextMeasurer`:
  - `measure_with_options_calls`
  - `prepare_with_options_calls`
  - `measure_fast_path_rate`
  - `prepare_fast_path_rate`
  - `prepared_layout_cache_hit_rate`

- What rebuilt telemetry now shows (`20 down + 20 up`, `CRANPOSE_TEXT_MEASURE_TELEMETRY=1`):
  - `measure_fast_path_rate ~= 97.3%`
  - `prepare_fast_path_rate ~= 98.1%`
  - `prepared_layout_cache_hit_rate ~= 99.6%`
  - `measure_calls ~= 1.8k` (not the prior `~570k` stale-binary signal)
  - `reshape_rate < 1%` with very high text-cache hit rate (~99%+) in this run.

- Current interpretation:
  - The large text-shaping churn seen earlier was not representative of the rebuilt binary path.
  - The remaining functional issue is directional return behavior (`return_present=false`), not catastrophic text-measure throughput in this robot profile.

## Continuation Findings (2026-02-25, drag/fling perf cycle)

- New optimization landed in `crates/cranpose-ui/src/text_modifier_node.rs`:
  - Added a per-node text measurement cache keyed by effective max width (`Option<f32>` bits).
  - Cache is invalidated when text/style/options are updated.
  - Goal: stop re-running `measure_text_with_options` for unchanged text nodes across repeated lazy-list passes.

- Validation:
  - `cargo test -p cranpose-ui` (all tests passed)
  - `cargo clippy -p cranpose-ui -- -D warnings` (clean)
  - `cargo fmt` (clean)

- Baseline no-drag check (2.6MB fixture, hold 5s):
  - still only startup warnings (`index 8`, `index 23`).
  - `fps_summary: fps=9.0 frame_ms=111.57 recompositions=3`.

- Drag + deep fling repro (`20 down + 20 up`, viewport drag path):
  - remains functionally reproducible: `return_present=false`.
  - one additional budget warning still appears during deep traversal (`index 132`).
  - fresh no-telemetry run measured:
    - `fps_summary: fps=42.8 frame_ms=23.37 recompositions=1718 recomps_per_sec=42`.

- Text telemetry delta on the same drag/fling repro:
  - before this change (previous cycle runs): `measure_with_options_calls ~= 68k`, `prepare_with_options_calls ~= 38k`.
  - after this change:
    - `measure_with_options_calls ~= 518`
    - `prepare_with_options_calls ~= 36.8k`
  - Interpretation:
    - layout-side text re-measure churn is largely eliminated;
    - remaining text work is mostly renderer prepare calls during active scrolling.

- Post-change `perf` sample for the same repro still shows font/shaping symbols present, but measurement call volume is now far lower by telemetry.
  - Remaining optimization surface is renderer prepare frequency and/or scroll/repass cadence, not repeated text measurement of unchanged nodes.

## Continuation Findings (2026-02-25, current chat)

- Re-ran the documented startup + drag probes on rebuilt binaries.
- Observed a new host/runtime profile in this session:
  - no-drag baseline (`hold 5s`) repeatedly reported:
    - `fps_summary: fps=3.4 frame_ms~295 recompositions=3`
    - startup warnings only (`index 9`, `index 24`)
  - viewport drag repro (`20 down + 20 up`) did not complete within `70s` timeout in this host state; logs consistently stopped after:
    - `viewport_drag start: deep_present=false return_present=true`

- Tried a renderer-side culling refactor for text prepare; it caused severe runtime regression in this workspace and was fully reverted (no net renderer behavior change kept from that experiment).

- Landed a safer lazy-input change in:
  - `crates/cranpose-foundation/src/lazy/lazy_list_state.rs`
  - `dispatch_scroll_delta` now clamps pending unconsumed backlog to:
    - `MAX_PENDING_SCROLL_DELTA = 2000.0`
  - when backlog is already clamped and additional same-direction deltas do not change pending value, invalidation is skipped to avoid no-op repass churn.
  - Existing direction-change backlog replacement logic remains in place.
  - Added tests:
    - `dispatch_scroll_delta_clamps_pending_backlog`
    - `dispatch_scroll_delta_skips_invalidate_when_clamped_value_is_unchanged`

- Validation run set for this landed change:
  - `cargo test -p cranpose-foundation lazy::lazy_list_state::tests::` passed (3 tests).
  - `cargo clippy -p cranpose-foundation -p cranpose-render-wgpu -p desktop-app` passed.
  - Note: `cargo clippy -p cranpose-foundation -- -D warnings` currently fails due pre-existing unrelated `dead_code` warnings in `crates/cranpose-core/src/frame_clock.rs`.

- Lazy telemetry spot-check (`1` viewport down drag, timeout-bounded) showed pending deltas logged as `-12.77` per dispatch in this slow host state; clamp was not hit in that short run.
- Post-change no-drag baseline remained in the same host-limited range (`fps ~3.4`), so this change is currently validated by unit tests and telemetry semantics rather than a measurable FPS uplift in this environment.

## Fresh Chat Handoff (next perf-opt cycle)

### Workspace state to continue from

- Modified files:
  - `crates/cranpose-foundation/src/lazy/lazy_list_state.rs`
  - `docs/MARKDOWN_PERF_NEXT_CYCLE.md`
- Untracked artifacts:
  - `clippy_out.txt`
  - `robot_out.txt`
  - `test_out.txt`

### What is ready

- Lazy scroll direction-reversal backlog mitigation is implemented and test-covered.
- Markdown robot runner can now drive viewport drags directly and emit sentinel position traces.
- Baseline no-drag run does not show uncontrolled index drift in this workspace and now measures `~9.2 FPS` on the 2.6MB fixture.
- WGPU `measure_with_options`/`prepare_with_options` fast paths and prepared-layout cache are active and telemetry-visible.
- Robot binary rebuild command is now explicit in the workflow to avoid stale measurements.
- Lazy scroll pending backlog is now clamped to `±2000px` and test-covered.

### What remains open

- User-reported manual bug: after dragging down to around `"19.02.2026"`, upward drag can snap back and feel blocked.
- Robot probes still end with `return_present=false` after `20` up-drags in this synthetic scenario.
- Manual verification on the exact user gesture path is still required (robot and manual may diverge in drag kinematics).
- Next bottleneck work should focus on scroll-position/gesture semantics and renderer prepare frequency during active scroll.
- Current host measurements in this chat are much slower than earlier same-day runs; reproduce on a stable host profile before drawing regression conclusions from absolute FPS.

### Recommended first commands in next chat

1. Rebuild the robot example binary (important before measurements):
   - `cargo build -p desktop-app --example robot_markdown_scrollbar --features robot-app`
2. Build and run baseline no-drag profile:
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_MARKDOWN_TOP_SENTINEL="Daily leetcode challenge" CRANPOSE_MARKDOWN_DEEP_SENTINEL="" CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_HOLD_SECS=5 CRANPOSE_LAZY_MEASURE_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`
3. Reproduce drag direction behavior with viewport drags:
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_MARKDOWN_TOP_SENTINEL="Daily leetcode challenge" CRANPOSE_MARKDOWN_DEEP_SENTINEL="19.02.2026" CRANPOSE_MARKDOWN_RETURN_SENTINEL="Daily leetcode challenge" CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_VIEWPORT_DRAG_DOWN_LOOPS=20 CRANPOSE_MARKDOWN_VIEWPORT_DRAG_UP_LOOPS=20 CRANPOSE_MARKDOWN_WAIT_IDLE_AFTER_DRAG=0 CRANPOSE_LAZY_MEASURE_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`
4. If still stuck, capture CPU profile for that exact run:
   - `perf record -F 299 -g --call-graph fp -o /tmp/mdperf_dirchange.data bash -lc '...robot_markdown_scrollbar env from step 2...'`
   - `perf report --stdio -i /tmp/mdperf_dirchange.data`
5. Run startup/steady-state measurement probes:
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_HOLD_SECS=8 CRANPOSE_TEXT_MEASURE_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`
   - `CRANPOSE_MARKDOWN_FIXTURE_PATH=/tmp/markdown_profile.md CRANPOSE_HEADLESS=1 CRANPOSE_MARKDOWN_SCROLL_LOOPS=0 CRANPOSE_MARKDOWN_HOLD_SECS=8 CRANPOSE_TEXT_RENDER_TELEMETRY=1 target/debug/examples/robot_markdown_scrollbar`

### Priority for next cycle

- Keep measurement-first flow:
  - verify if remaining jump-back/blocked-return comes from queued deltas, fling handoff, or scrollbar `scroll_to_item` interference;
  - instrument app-shell frame phases (`layout`, queue dispatch, scene rebuild, renderer submit) on rebuilt binaries for manual-path parity.
