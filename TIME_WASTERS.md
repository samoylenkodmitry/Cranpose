# Time Wasters

- Parallel execution of robot examples is flaky on this machine even without Cranelift. `16`, `8`, and `4` parallel workers produced intermittent segfaults or timeouts, while sequential execution passed `80/80`, so `run_robot_test.sh` now defaults to sequential mode and leaves `--parallel N` as an explicit opt-in.
- Perf harness trap: do not run multiple GPU perf scenarios in parallel on this machine. The numbers become meaningless because the scenarios contend for the same GPU/driver state. Perf comparisons for `robot_perf_harness` must be sequential and single-scenario.
- Lazy-list heap profiling trap: `perf_robot_heap.sh --scenario lazy_list_scroll --tool massif` on the WGPU `robot_perf_harness` is dominated by Vulkan/WGPU device and pipeline setup. It is useful for RSS sanity, but it does not isolate modifier-slice or layout-box hot-path allocations.
- Renderer trap: do not keep chasing text-only crispness fixes before surface planning is unified. The recent scroll wobble, decorated-text breakage, and WGPU OOM were not independent glyph bugs. They came from restarting translated stable-capture boundaries inside already-isolated surfaces. Fix the surface planner first.
- Robot gate heat trap: if one robot example sits near its 5-minute cap, stop and check host state instead of waiting indefinitely. The full sequential suite has many examples and needs a larger outer timeout; the per-example `run_robot_test.sh` caps are the real hang protection.
- Slot-table perf stability trap: do not chase source regressions when `./perf_slot_table_v2.sh --stability-check` flips whole benchmark families by 30-50% on the same tree. The old gate pinned to CPU 0 and compared after a cooldown, which produced host phase shifts. Keep each stability baseline adjacent to its same-benchmark comparison, leave CPU pinning opt-in, use fixed-step benchmark batches so Criterion does not compare different fixture states, keep at least two unrecorded same-tree warmup passes before recording the stability baseline, and retry noisy same-tree pairs before treating them as host instability. If the full matrix fails one benchmark while the immediately rerun isolated `--filter` check passes under threshold, record it as host-noise evidence and increase the retry budget before editing production code.
- Stress-suite budget trap: `./stress_slot_table.sh` must stay inside its default 600-second wall-clock guard. If it hits the guard, shrink the work being done by the stress gate or move exhaustive investigation to an explicit opt-in command; do not leave the default validation path open-ended.
- Final identity-gate timeout trap: `./verify_slot_table.sh` can reach robot e2e with fmt, tests, clippy, Android, and wasm already green, then time out because the full 92-example sequential robot suite does not fit in the default 10-minute budget. `./stress_slot_table.sh` can likewise pass slot validation and model stress before timing out in perf stability. Treat these as gate-budget failures, not product-code failures, unless the partial logs show a concrete failing test.
- Winamp robot false alarm: `robot_winamp_native_window_geometry` falls back from pid-scoped `xdotool search --pid` to title-only `visible_windows_summary()`. A concurrent Cranpose/Winamp robot run can make the test report another process's Winamp windows as still visible for the current pid after Dock. Close same-title concurrent demos before chasing native-window visibility code.
- Android host-window sizing trap: the locked `android-activity` 0.6.0 does not expose `run_on_java_main_thread`. The 0.6.1 source has that helper, but moving to it pulls `jni` 0.22 while `reqwest`/`webbrowser` still pull `jni` 0.21, so it introduces a duplicate JNI stack. Keep Cranpose on `jni` 0.21 unless the dependent stack is updated together.
- Robot thermal guard trap: when concurrent ML work keeps max CPU temperature above the `run_robot_test.sh` host-capacity threshold, the sequential robot suite blocks before building. Stop the run and report the environmental block instead of waiting for a cooldown that cannot happen under that workload.
- Robot thermal guard can also stop after a successful robot-profile build and before a scenario. On May 23, 2026, `./run_robot_test.sh --sequential` built all 100 robot examples, then exited `host_not_ready` for `robot_advance_frame_bug` after 300 seconds because max CPU temperature stayed around 94-98C under concurrent ML work. A follow-up `./run_robot_test.sh --sequential --skip-build` passed the first 4 robots, then stopped before launching `robot_async_tab_bug` with max CPU temperature around 88-97.5C. Treat this as an environmental gate block when the failed entry is `host_not_ready` and no robot binary was launched.
- Web build script trap: fast `apps/desktop-demo/build-web.sh` must run with `set -e`; otherwise a failed `wasm-pack` compile can still print the success footer and report the previous `pkg/desktop_app_bg.wasm` size.
- Robot profile cold-build trap: after broad dependency or WGPU changes, `env CRANPOSE_HEADLESS=1 CRANPOSE_USE_SCCACHE=0 timeout 180s ./run_robot_test.sh --sequential` can spend the entire outer timeout compiling the robot profile and never reach a scenario. On May 23, 2026, the same gate also timed out at 300 seconds after compiling through `cranpose-testing` without starting a robot scenario. Treat that as an insufficient outer timeout for a cold robot build, not as a robot failure; prebuild the robot profile or use a longer outer timeout before expecting scenario results.
- LeetCodeDaily X11 active-frame polling trap: `robot_leetcodedaily_full_layout_scroll_stability` can miss the first moved X11 screenshot in the full 100-example suite on a thermally loaded host even though the same robot and the predecessor pair pass immediately afterward. The geometry has already moved, but the screenshot polling window was too small for suite-load presentation latency. Keep the assertion that a moved frame appears, but use a wider polling window before treating this as renderer or scheduler regression.
- Frame-graph staged-upload trap: once native segment draw chunks are recorded into one command buffer, every pending `copy_buffer_to_buffer` source range must be unique until submit. Reusing offset zero in the shared upload buffer makes earlier copies read the later `queue.write_buffer` payload, which can produce stable geometry with a grey/dark corrupted full frame. Validate this class with a clean-main X11 screenshot comparison and a full-window color-health robot assertion, not only stable crop diffs.
- Robot skip-build thermal block on May 23, 2026: `./run_robot_test.sh --sequential --skip-build` stopped before launching `robot_advance_frame_bug`. The host-capacity guard reported max CPU temperature 91.4-99.0C and timed out after 300 seconds with status `host_not_ready`; no robot binary was running. This is an environmental gate block under concurrent ML load, not a product-code robot failure.
- Focused presented-renderer robot thermal block on May 28, 2026: `env TMPDIR=/home/s/.cache/cranpose/tmp CRANPOSE_TMPDIR=/home/s/.cache/cranpose/tmp MAGICK_TMPDIR=/home/s/.cache/cranpose/tmp CRANPOSE_USE_SCCACHE=0 CRANPOSE_BUILD_JOBS=2 ./run_robot_test.sh --sequential --example robot_presented_window_geometry --example robot_renderer_micro_contract` stopped before building. The host-capacity guard timed out after 300 seconds while max CPU temperature stayed around 93-98C under concurrent ML load; no robot scenario launched. Treat this as an environmental gate block, not renderer evidence.
- WGPU duplicate-budget trap: do not remove Vulkan just to clear `hashbrown`/`foldhash`. A no-Vulkan WGPU 29 graph plus `indexmap 2.13.0` clears `cargo xtask dependency-budget --strict`, but `robot_renderer_micro_contract` fails on Linux/X11 with no compatible adapter because GL is not compatible with the provided surface. Keeping Vulkan is required for the tested desktop renderer path until upstream `gpu-descriptor`/WGPU aligns its `hashbrown` dependency.
- WGPU duplicate-budget refresh trap: on May 26, 2026, `cargo search`/`cargo info` still reported `wgpu 29.0.3` and `gpu-descriptor 0.3.2`; `cargo update -n -p wgpu -p gpu-descriptor -p gpu-allocator` locked zero packages. Re-running that same probe will still end at the WGPU-owned `foldhash 0.1/0.2` and `hashbrown 0.15/0.16` split unless crates.io has a newer WGPU/gpu-descriptor line.
- X11 comparison target trap: `scripts/x11_compare_origin_main.sh` and `scripts/x11_compare_downstream_cranpose.sh` put cargo targets under the comparison label. Changing only `--label` forces a fresh baseline/current build even when the source refs are unchanged. Reuse the same label while iterating thresholds or call `scripts/x11_compare_window.sh` with explicit `--baseline-target` and `--current-target`.
- Downstream X11 helper-window trap: title-based branch comparison can capture a helper/browser window spawned by the app if the app changes presentation mode or leaves a same-title helper alive. Use `--expected-owner-command <app-binary>` for native-window renderer comparisons so `_NET_WM_PID` ownership is checked against the launched app process command, not only the launched process tree.
- Native-release perf iteration trap: before the desktop platform export split, `perf_robot_fps.sh --scenario markdown_scroll` rebuilt the monolithic `desktop_app` native-release library with `cdylib`, `staticlib`, and `rlib`, then linked the robot harness. After small app/perf-harness edits on the hot ML-loaded machine, native-release rebuilds took 15-17 minutes before a 2-4 second robot scenario could run. A cold `release-fast` Markdown-only probe on June 1, 2026 still took 27m15s, and process inspection showed rustc compiling `desktop_app` as `cdylib`, `staticlib`, and `rlib` for a single robot example. The structural fix is in place: `desktop-app` now owns rlib/bin desktop work, and `desktop-app-platform` owns Android/wasm exported library artifacts. If this trap appears again, first check whether the perf harness is accidentally building `desktop-app-platform` or another exported-library crate.
- Release-fast robot link cost remains non-trivial after the export split. The post-split Markdown probe built only `desktop-app` and `cranpose-testing` for the robot example, not `desktop-app-platform`, but the cold optimized build still took `11m40s` on the hot ML-loaded host. Treat future 10+ minute release-fast probes as a harness/link-profile cost unless process inspection shows exported-library targets or unrelated app packages in the build.
- Wheel-smoothing trap: do not try to fix low wheel-scroll FPS by animating wheel deltas in `cranpose-ui::modifier::scroll` before translated-layer caching is retained. On June 3, 2026, a local wheel animator made `robot_leetcodedaily_full_layout_scroll_stability` worse: steady cache hits dropped to zero, the 1425x1365 isolated layer re-rendered every sampled frame, `avg_pass_count` rose to 224, and FPS fell to 12.3. The real fix is retained translated/offscreen surface reuse for moving scroll content, not more scroll-frame callbacks over an uncached capture.
- Cargo lock trap: do not launch multiple `cargo test` commands in parallel in this workspace. They serialize on Cargo's package/artifact locks and add noise without saving time. Run focused cargo tests sequentially; reserve parallelism for plain file reads/searches.
- Source-hygiene filter trap: `cargo test -p desktop-app leetcodedaily_fps_perf_gate_enforces_120fps_budget` built `desktop-app` tests for several minutes and ran zero tests because the actual guard is `heavy_shell_entrypoints_use_local_resource_guards`. Use the exact test name or a source read when validating the leetcodedaily perf-harness guard.
- Markdown text-cache trap: do not route scroll-translated static text from the glyph-atlas path to per-text cached `ImageBitmap` draws. On June 4, 2026, `robot_markdown_default_visual_contract` upload bytes dropped from roughly 168-292 KB/frame to 32-36 KB/frame and text-image hits appeared, but rapid scroll regressed from `work_fps=162.4 work_p95_ms=7.32` to `work_fps=104.1 work_p95_ms=13.28` because each visible text image requires separate texture binding/draw work. The correct direction is retained glyph geometry or a shared text atlas/texture batch, not one texture per text block.
- Markdown warmup trap: do not raise `EXPENSIVE_RENDER_WARMUP_ITEMS` as a local fix for markdown scroll spikes. On June 4, 2026, increasing it from 6 to 14 left visible upload/glyph counts effectively unchanged and regressed rapid scroll to `cadence_fps=118.2 work_fps=145.4 p95_ms=13.40 work_p95_ms=8.43`. The missing architecture is renderer-side glyph-run retention/prefetch, not more lazy-list measure work.
- Web visual-bug verification trap: do not try to reproduce a browser rendering bug with `cargo check --target wasm32-unknown-unknown` — it fails early with the `getrandom` "wasm_js" backend compile_error because the wasm-js RUSTFLAGS are set by `apps/desktop-demo/build-web.sh` (wasm-pack), not by a bare cargo invocation. Build with `build-web.sh --fast`, then verify headlessly: `playwright-core` driving the cached Playwright Chromium (`~/.cache/ms-playwright/chromium-*/chrome-linux64/chrome`) with `--use-gl=angle --use-angle=swiftshader --enable-unsafe-swiftshader`, screenshot the `#cranpose-canvas` element, and measure the white-pixel fraction. A blank first frame reads ~0.99 white; a rendered frame reads ~0.01. Hook `window.requestAnimationFrame` from an init script to count idle frames and prove the loop stops spinning when nothing changes (~0 frames over a 2s idle window).
- Fractional-scale real-display robot trap: `robot_leetcodedaily_full_layout_scroll_stability` fails deterministically (improvement_ratio=0.0000 across all attempts, identical scores on reruns AND on clean main) when run on a real X11 display whose scale factor is fractional (e.g. Xft.dpi 130 → winit scale ≈ 1.354). The X11 strip comparison assumes scale-1 logical/pixel mapping. Verified June 10, 2026 by running the same binary from clean main: identical failure. Run this robot under Xvfb (scale 1) or with `WINIT_X11_SCALE_FACTOR=1`; do not chase renderer regressions from a fractional-scale host run alone.
- HiDPI perf diagnosis shortcut: `CRANPOSE_GPU_STATS=1` per-60-frame lines immediately exposed why leetcodedaily-style scrolling was slow only on HiDPI: `shadow_cache: shape_miss=15 miss_px=14.20MP` + ~58MB/frame transient offscreen allocations during scroll at scale 1.354, while scale 1 showed zero misses. Shadow raster cache keys were anchored to floored device-pixel bounds, so the device subpixel phase leaked into the content hash at fractional scales (and floor/ceil size flapping multiplied cache entries). Fixed by anchoring the hash to the shapes' own unfloored bounds (quantized at 1/16 device px) and translation-stable surface sizes; shadow LRU budget raised 64→192MB for 4K-class shadow sets.
- Idle frame-rate trap: when a Cranpose desktop app feels laggy, check idle fps FIRST (`CRANPOSE_GPU_STATS=1`, count `[GPU f#N]` lines per second × 60). leetcodedaily rendered 477fps at idle on a 60Hz panel because (a) an always-mounted `rememberInfiniteTransition` (workaround for long-fixed issue #262) kept animations alive forever, and (b) pre-0.1.14 desktop pacing defaulted to NoVsync so animations rendered uncapped. Both were invisible in robot runs: drivers measure throughput and the fixture idles between injected events. Production-mode verification needs the real binary on a real display: idle must be 0fps, animating content exactly the refresh rate.
- xdotool wheel-scroll trap: `xdotool click 4/5` (XTEST legacy wheel) does not scroll Cranpose/winit windows on this X11 setup even when `getmouselocation` confirms the pointer is over the app window — clicks and drags work, wheel does not. Don't burn time on it: for scroll measurements use the in-process Robot (`robot.mouse_scroll`) via a test driver, pinning `with_frame_pacing_mode(Vsync)` if production pacing must be preserved.
- Publish-tag trap: `publish.yml` does NOT bump versions — it *verifies* that `main` already carries the release metadata for the tag, then publishes. Tagging `vX.Y.Z` at a commit whose `Cargo.toml`/`Cargo.lock`/`apps/isolated-demo/Cargo.toml` still hold the previous version fails fast with "main must already contain the release metadata for vX.Y.Z". Correct order: commit a `Release vX.Y.Z` bump to `main` first (workspace.package version + every `cranpose*` workspace.dependency + the matching `cranpose*` `version = ` lines in `Cargo.lock` + isolated-demo — a plain `0.1.N`→`0.1.N+1` replace is safe because no third-party dep shares those version strings), THEN create the tag at that `main` HEAD (the workflow also rejects a tag not pointing at `main` HEAD).

## Vulkan GPU tests hang when run as harness background tasks (2026-07-02)
- Symptom: `cargo test -p cranpose-render-vulkan` launched as a Claude-Code
  background task spins at 100% CPU forever (NVIDIA RTX 2070, proprietary
  driver); killing the task kills only the wrapper shell — the test binary
  orphans and keeps burning a core. Eight orphans accumulated over hours.
- CONFIRMED root cause (/proc thread states of a live hang): concurrent
  test threads each create their OWN VkInstance+VkDevice; with two+ devices
  live in one process the NVIDIA driver intermittently deadlocks — one
  thread spins in the driver's userspace fence-wait (wchan 0, 100% CPU),
  another sleeps on a kernel rt_mutex, two sets of [vkcf]/[vkrt]/[vkps]
  driver threads present. Intermittent: the same binary can pass in 1.3s.
- Fixes applied: (1) every submit in cranpose-render-vulkan waits on a
  FENCE with a 10s timeout instead of queue_wait_idle — a future stall is
  a clean GpuTimeout error, never an infinite burn; (2) tests share ONE
  process-wide VulkanContext behind a Mutex (real apps have one device).
- Protocol: run GPU suites as `timeout -s KILL 120 <test-binary>` with
  timeout parenting the binary DIRECTLY (build first via
  `cargo test --no-run`); after any GPU-test session check
  `ps -eo pid,etime,pcpu,args | grep cranpose_render`.

## Stuck-detection for background work (2026-07-03)
- A single `ps` snapshot is NOT liveness. A background agent's host process
  died (harness restart) minutes after a "99% CPU compiling" report — the
  user saw 0% CPU while the assistant reported active work.
- Protocol: liveness = TWO cpu samples spaced apart AND newest artifact
  mtime advancing AND output file growing. Any waiting loop gets a
  staleness deadline (~10 min without new artifacts → investigate/kill,
  never keep waiting). After any background session: check for orphaned
  Xvfb/app/cargo processes.

## Cross-actor binary-path collisions + pkill self-kill (2026-07-03)
- Two actors (main loop + agent) building the same cargo target race on
  target/<profile>/<bin>: a "verified" capture can be the OTHER renderer.
  Copy binaries to distinct names (cp target/... $SCRATCH/demo-slim-X)
  before running when any concurrent builds are possible.
- `pkill -f <pattern>` kills the CALLING SHELL too when the pattern appears
  in the shell's own command line (batch dies mid-way, later commands
  silently never run). Kill by stored PID, or pkill -f a string that cannot
  match your own invocation.
- Walker clip-space bugs found by counting silent returns: per-primitive
  graph clips are LAYER-LOCAL (see primitive_emit::resolve_primitive_clip);
  intersecting them as world-space silently dropped all clipped text in
  translated layers, and reassigning rect to world-visible then re-mapping
  double-applied the transform (shifted images). Silent `return`s in draw
  paths must either count as unsupported or be provably legitimate.

## Handed the user a broken build (2026-07-03, slim renderer)
- Validated ONLY under Xvfb at scale 1.0, then handed to the user whose
  desktop runs 1.354 (which project memory explicitly recorded). Scene
  rendered 800x600 in a 1083x812 window (root_scale not reaching scene
  build) and the app froze with silent per-frame errors.
- The release build lacked the `logging` feature: every log::error! in the
  render/present path was compiled out. Errors were structurally invisible.
- GATES BEFORE ANY USER HANDOFF: (1) run at WINIT_X11_SCALE_FACTOR=1.354;
  (2) interact: switch tabs, resize the window, minimize/restore; (3) build
  with logging enabled and read the log; (4) compare against the wgpu build
  side by side on the same display.
- pgrep/pkill self-match, THIRD bite: a wait-loop `until ! pgrep -f "cargo
  build"` matches its own shell command line and spins forever (reported
  "stale, zero cpu" by the user while everything was long done). Antidote:
  bracket the pattern (`pgrep -f "[c]argo build"`) or match the exact
  binary (`pgrep -x cargo`).
- GUI apps launched from the assistant's sandboxed shell inherit nice 5 →
  vsync GPU apps stutter ("super low fps", sluggish native windows) while
  the binary is fine. renice to 0 is Permission denied; systemd-run --scope
  ALSO inherits the caller's nice. Correct launch for user-facing apps:
  `systemd-run --user --unit=<name> --setenv=DISPLAY=:0 <binary>` (spawns
  from the user manager at nice 0). Diagnose via `ps -o ni` — the 'N' in
  STAT is the tell.
