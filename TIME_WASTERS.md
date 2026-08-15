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
- Robot thermal guard resume-knob trap (2026-07-11): the host-capacity guard has TWO knobs — trip `CRANPOSE_HOST_MAX_TEMP_C` (default 90) and `CRANPOSE_HOST_RESUME_TEMP_C` (default 85). Raising only MAX does not help on a host whose ambient sits above RESUME: one momentary spike over MAX arms the wait, which then demands cooling below RESUME that never comes, and the run dies `host_not_ready` after `CRANPOSE_HOST_MAX_WAIT_SECS` (default 300). On this desktop (browser + OBS keep Tctl ≈ 91-93C) run suites with `CRANPOSE_HOST_MAX_TEMP_C=97 CRANPOSE_HOST_RESUME_TEMP_C=93` and a longer max wait; three suite launches were lost to this across July 10-11, 2026.
- White-on-white robot diff trap (2026-07-11): `robot_liquid_motion_contract` reported "menu did not materialize" while its saved capture showed the menu fully open. The glass menu card is white-on-white over the page (max channel delta ≈ 10, below the d>12 diff threshold), so `diff_area` counted only text/icons/shadow (~2400 sampled px) against a 2500 floor — a coin flip that lost under suite load. Popup presence checks belong in semantics (`find_text_in_semantics` poll), with pixel floors kept only as drew-something sanity. Related: failing robot runners must not `process::exit(1)` from the driver thread — it races main-thread GPU teardown and dumps core (exit 139, masking the real failure); set a flag, call `robot.exit()`, and return the exit code from `main`.
- Publish-tag flow (re-verified releasing v0.1.87 on 2026-08-12): pushing an annotated `vX.Y.Z` tag at `main` HEAD is the whole release. `publish.yml` rewrites the versions itself, commits `release: vX.Y.Z` to `main`, moves the tag onto that commit, publishes to crates.io, and follows with `chore: point isolated demo at vX.Y.Z`. Do NOT bump versions by hand first; the tag commit does not need to carry them. Two constraints remain real: the tag must point at `main` HEAD ("Tag $tag must be created from main HEAD"), and a **manual `workflow_dispatch`** takes the other branch of that step, where `main` must already contain the release metadata or the run stops with "main must already contain the release metadata". An earlier version of this entry described that dispatch-only failure as the normal flow and prescribed a hand-bump that is not needed — the workflow gained the tag-push branch since. Deploy-Pages can still be cancelled by the tag-move and then needs `gh run rerun`, though it survived intact for v0.1.87.

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

## Robot suite vs. host thermal guard (2026-07-10)
`run_robot_test.sh` refuses to start while the CPU is above 85°C and gives up
after 300s. Any concurrent heavy cargo/gradle build keeps the package hot the
whole window, so the suite "fails" without running a single test. Schedule it
LAST, after every other build finished — not in parallel.

## Popup content updates (2026-07-10)
`Popup` content used to be remembered once — anything animated or stateful
inside popup content silently froze at its first-composition values (and a
child sized past the popup's bounds is culled from hit-testing, so oversized
"scrim" children never receive taps). Fixed: popup content re-registers per
caller recomposition, and outside-tap dismissal is first-class via
`PopupDismissable` (host-level scrim). Don't reintroduce hand-rolled scrims
inside popup content.

## "Recomposed value never reaches the screen" — probe ladder (2026-07-10)
A Text color change updated the modifier element, the measure pass, AND the
modifier-slices snapshot, yet the screen kept the old color for hours of
static code reading. The break was the LAST cache before the GPU: TextService
`prepared_cache` was keyed on `measurement_hash` (deliberately color-blind)
while storing the full visual style. Lesson: don't trace these top-down by
reading code — drop eprintln probes at the five stations (element update →
node measure → modifier_slices_snapshot → scene builder → renderer cache),
run once, and the first station still printing the OLD value brackets the
bug. `CRANPOSE_RENDER_PHASE_DIRTY_DIAG=1` prints per-frame rebuild paths
(skip/visual-update/update/rebuild) and dirty-node ids for free.

## Washed-out colors are a FORMAT bug, not a palette bug (2026-07-10)
Every color (shapes, gradients, text) rendered pale for the framework's whole
life: all four shells preferred an sRGB swapchain while the pipeline passes
sRGB bytes through — hardware re-encoded them (52,199,89)→(125,229,160).
Sampled images looked right (sRGB-texture decode cancels it), which hid the
bug and framed it as "gradients look off". If solids sample wrong but images
look fine, check `surface.get_capabilities().formats` selection FIRST;
`robot_color_fidelity` now pins byte-exact output. Beware tests that encode
the bug as truth: the micro-contract's expected_screenshot_rgb used to apply
linear→sRGB on purpose.

## find_text_in_semantics is a SUBSTRING match (2026-07-10)
A walkthrough clicked "Discover" and teleported to another demo tab; two
hours of hit-graph/pointer-capture spelunking later, the truth: the query
matched "**Discover** updates to Swift…" (a session subtitle that had
scrolled under the strip), so the click landed on the top tab strip. The
renderer/hit-testing was innocent. For interactive lookups always use
`find_button_exact_in_semantics` (exact + role-filtered); reserve substring
`find_text_in_semantics` for asserting mere presence. Symptom signature:
"clicks land on a widget nowhere near the coordinates you thought you used"
— print the resolved bounds BEFORE suspecting dispatch.

## Shader composites leak pass state (fixed, guarded) (2026-07-10)
`draw_prepared_shader_src_over` draws into a caller-owned fused pass and set
a sub-viewport for its dest rect without restoring — every later draw in the
same segment chunk (text after fire-shader boxes) got remapped into that
sub-rect and vanished at most scroll offsets ("SDF Halo Border" header).
Restore full viewport+scissor after any composite that shares a pass.
Guarded by robot_regression_fused_viewport_contract (also covers the
scrolled-tab-switch ghost).

## Oversize overlay nodes get silently clamped by parent constraints (2026-07-11)
The toggle's press lens (a 78×59 glass node inside the 63×28 track Box) came
out "shifted up with its bottom sliced at the track's middle" — hours of
suspecting the shader/SDF math. Truth: `Modifier::size()` COERCES into the
incoming constraints, so the node measured 78×28 while the morph geometry
assumed 59 tall. Any overlay that must exceed its parent (lenses, badges,
glow halos) needs `Modifier::required_size()`. Symptom signature: an effect
looks "cropped/mis-anchored on one axis only" and the amount equals the
parent's max constraint.

## Screenshot-based ghost hunting can't see the presented surface (2026-07-11)
`robot.screenshot()` re-renders the retained scene offscreen
(`capture_frame_with_scale`) — it can NEVER show swapchain/present-path
artifacts the user sees in the real window. For "stale pixels after tab
switch" reports, capture the REAL window: run the app windowed
(`examples/ghost_presented_probe.rs`) and grab it externally with
`import -window $(xdotool search --name …)`. Also remember the demo's tab
strip SCROLLS: semantics can locate an off-screen strip tab and the click
lands on whatever is at those coords instead (use the set-tab hook).

## Judging visual work with stale/cropped captures wastes review rounds (2026-07-11)
Two external-judge rounds were partially spent on artifacts of MY captures:
a 45px-tall crop cut the lens bottom ("mis-anchored" verdict) and one set
predated the fix it was judging. Before spending a judge round: re-capture
AFTER the last code change, and crop generously around the component
(include full overflow extents).

## 2026-07-11: glass geometry density-vs-render-scale (hours)
- Symptom: toggle/tab lens rendered as a flat white slab + tiny displaced
  green blob; user saw it as "glass distorts wrong" and "bubble freezes".
- Root: `GlassMorph` uniforms were packed as dp×`current_density()` (1.354
  here via Xft.dpi 130 — even HEADLESS runs get it because DISPLAY=:0.0 is
  set in every shell) while robot captures render at root_scale 1.0. Geometry
  landed 1.354× off. It looked correct the previous evening only because those
  runs matched scale by accident.
- Debug path that finally worked: minimal `lens_probe` example over a
  two-tone background + shader-output dumps (`return vec4(uv,0,1)`, dumping
  uniforms as colors). Pixel-measuring the SDF center/size vs expected gave
  the ×1.354 ratio immediately. Hours were lost before that on renderer
  composite-path forensics (backdrop capture/scissor/cache all innocent).
- Fix: geometry in dp + container = node size dp (shader derives px-per-dp
  per axis from the injected node pixel rect). Guard test:
  `morph_glass_packs_dp_geometry_with_node_size_container`.
- Meta-lessons:
  - `git stash` on a tree carrying YESTERDAY'S uncommitted session buries the
    baseline you wanted to compare against (stash swallowed 70 files) AND
    destroys mtime forensics (pop rewrites files). Diff experiments on a
    dirty pre-alpha tree need env-var kill-switches, not stashes.
  - X11 multi-monitor: xdotool getdisplaygeometry lied (1920x1080) vs the
    real 5760x2160 virtual screen with a DEAD ZONE at top-left where the WM
    parks windows: grabs "work" there but the pointer CANNOT enter (pinned at
    x=1920), so clicks silently miss. Always windowmove onto a real monitor
    (HDMI-0 at +1920+0) before driving input.
  - The demo release binary has NO logger (`logging` feature off): log::warn
    diagnostics are invisible; build robot profile with
    `--features desktop,logging` or use eprintln-based env-gated diags
    (`CRANPOSE_BACKDROP_DIAG`).

## 2026-07-11: Visual judge-loop traps (text-selection chrome match)

- **Eyeballed calibration cost three judge rounds.** The loupe's
  magnification was first read off a 160%-scaled crop as "1.7x" and the
  whole optic (dome profile, fold reach, knob plateau) was engineered
  around it. Precise glyph-height measurement on the raw frame gave a
  UNIFORM ~1.25x — at which point the dome, the plateau and the baseline
  arc all became unnecessary and the fold placement fell out naturally.
  Rule: never calibrate a shader to an eyeballed number; measure glyph
  extents pixel-precise on the raw reference before writing the first
  uniform.
- **`screenshot_with_scale(3.0)` stalls the driver ~200 ms.** In-flight
  animation keyframes sampled through it are all ~200 ms late; a judged
  "no grow animation" verdict was purely this. Capture motion sequences
  as one-capture-per-repeated-gesture (press → sleep to offset → capture
  once → release), never several captures inside one gesture.
- **Vision-judge reports need pixel arbitration.** Across six rounds,
  judges (a) measured a shadow halo as capsule height, (b) inverted the
  prose spec ("bottom ~90% of top" → "bottom should be brighter"),
  (c) called a risen bubble "glued to the baseline" from a crop framing,
  and (d) flip-flopped between rounds on birth aspect. Every MISMATCH
  that survived was confirmed by scripted pixel measurement first; every
  fix applied on a judge's word alone was wasted. Judges find WHERE to
  look; numbers decide WHAT is true.
- **Robot touch events carried no PointerSource.** Touch-gated UI
  (finger handles, loupe) silently never armed under the robot until
  desktop.rs tagged `PointerSource::Touch` on the Touch* commands.
- **Handle gestures ate hover moves.** `PointerEventKind::Move` without
  a preceding press drove `on_drag` — a hovering mouse moved the caret,
  and the post-release synthesized hover re-armed the loupe (bubble
  reappearing ~140 ms after release). Gesture loops must gate Move/Up on
  a live press.

## Wall-clock keyframing of sub-100ms animations (robot, 2026-07-12)

Symptom: dissolve keyframes captured via sleep(offset)+capture were
byte-identical "gone" frames at +8/+25 while +42 caught a real pose —
looked like a widget bug, was three compounding harness/runtime traps:

1. The headless loop FREE-RUNS (~5ms real/frame) and scale-3 captures
   stall 25-60ms unpredictably — sleep choreography samples a 55ms
   tween anywhere including past its end. Fix: `capture_keyframes`
   (atomic exact-clock advances; commit 37cfd6cb). Never add retry
   loops around racy captures — make the clock deterministic instead.
2. Presence probes whose band touches glyph ascenders are tautologies:
   they pass forever and hide that the thing never rendered. Bands must
   sit strictly above ALL other chrome (menu capsule + halo included).
3. The real bug underneath: recompose callbacks were consumed per
   direct recompose and healed by ancestor promotion NEXT frame — a
   tween's final write had no next frame, so the loupe froze and only
   an arg change resurrected it. Wall-clock runs masked this for the
   entire project lifetime (plus one extra ancestor recompose per
   animation frame everywhere). See recompose.rs / animation_frame_pump
   test.

Debug method that worked: env-gated eprintln tracing layer-by-layer
(watchers → pending scopes → recompose branches → callback set/take),
diffing one working frame against one dead frame in the SAME run.

## Robot screenshot output variables are filtered (2026-07-13)

`run_robot_test.sh` starts each example through `env -i` and
`robot_process_env`; the allowlist forwards `CRANPOSE_*` and a small set of
platform variables, but not `ROBOT_SHOT_DIR`. Several visual runners document
`ROBOT_SHOT_DIR`, so invoking them through the suite appears to honor a custom
directory while silently writing to `target/liquid-*`. This caused two full
recaptures to be filed under the wrong path. When running through the suite,
move the default output directory after the runner exits or add the variable
to the explicit environment allowlist; direct `cargo run` does honor it.

## Component crops must share an internal anchor (2026-07-13)

Comparing equal-size target/current crops is still invalid when the component
lands at a different Y inside each crop. A toggle lens was mismeasured as 48dp
instead of 39dp because the current track center sat 26 physical pixels below
the target track center. Align a stable internal feature first (the track
center, bar top, or text baseline), keep both images at native device scale,
and only then measure the optical silhouette. Shadows and white-on-white glass
edges are not reliable geometry anchors.

## Editing liquid_glass.wgsl: validate with render-wgpu tests FIRST

`cargo test -p cranpose-ui-graphics -p cranpose-liquid` does NOT parse the
WGSL — the naga validation lives in
`cargo test -p cranpose-render-wgpu --lib shader_cache`. A shader that fails
validation does not panic the robot runners: every glass effect silently
disappears, so morphs render as full-rect cards, lenses stop following the
finger, and the loupe never raises. If several unrelated liquid robots start
failing with "frozen geometry" symptoms at once, suspect a WGSL identifier
error before debugging any widget.

## robot_adaptive_frost red on GitHub-hosted CI only (lavapipe vulkan)
Since ~2026-07-16 the Rust workflow's robot shard 1 fails robot_adaptive_frost
with DETERMINISTIC numbers (adaptive 142.9 vs plain 138.8): the "dark
backdrop" scenario reads LIGHT on CI. Reproduction attempts: NVIDIA vulkan
and llvmpipe GL (LIBGL_ALWAYS_SOFTWARE=1 WGPU_BACKEND=gl) both render
byte-identically and PASS (white 88.1/148.7, black 144.1/24.0) — the
divergence is exclusive to lavapipe VULKAN (CI: WGPU_BACKEND=vulkan +
mesa software driver). plain=138.8 vs local plain=24 suggests the dark
backdrop never reaches the capture (sRGB double-encode of ~24 ≈ 84-154, or
a failed backdrop composite). The heavy-selfhosted workflow (real GPU) is
green, and the local suite passes 129/129. DO NOT debug this blind through
30-minute CI cycles: install `vulkan-swrast` (Arch) or run the Ubuntu
mesa-vulkan-drivers stack in a container first, then chase the headless
target/surface format path (surface_format.rs prefers non-sRGB but headless
offscreen targets pick their format elsewhere).

## robot_regression_shader_visual_contract only means something under the harness
One-off `cargo run --example robot_regression_shader_visual_contract`
invocations fail DETERMINISTICALLY (left_blue 2924 vs right_blue 1466,
bit-identical on workstation and builder) because this non-headless X11
test's drags depend on WM window placement, which differs outside
run_robot_test.sh. Under `./run_robot_test.sh --sequential --example ...`
it passes on the same tree. Hours were spent bisecting shader commits that
were provably innocent (the metrics never changed across variants). Judge
this test ONLY through the harness.

## Discrete move+capture loops CANNOT verify continuous-gesture physics
Interleaving `xdotool mousemove_relative` with `import` screenshots inserts
a ~150ms pause per capture — any spring/tracker catches up during the pause
and the sequence looks like perfect following even when the live behavior
freezes during real continuous motion (the loupe-follow bug shipped
"verified" this way). To verify tracking: run the mouse stream in a
BACKGROUND loop (`( for i in $(seq 1 120); do xdotool mousemove_relative -- -2 0; done ) &`)
and capture frames concurrently in the foreground, logging
`getmouselocation` before each capture to measure the live trail.

## Animatable retargets used to starve springs (fixed, keep the test)
`animateTo` reset `last_frame_nanos`, so a spring retargeted every frame
(continuous gesture tracking) integrated dt=0 forever and froze until the
gesture stopped. Fixed by preserving the spring frame chain across
retargets; `spring_retargeted_every_frame_tracks_a_moving_target` in
cranpose-animation pins it. If a widget tracks a moving target, retarget
per move with `animate_to_with_velocity` — no rate limiting needed.
- Progressive-fps-decay hunts: when EVERY tracked counter is flat (nodes,
  scopes, caches, frame callbacks, heap bytes) but frame work grows
  monotonically across interaction cycles, suspect the snapshot record
  chains — they had no length counter and every state READ walks the full
  chain (`readable_record_for`). Method that pinpointed it in minutes:
  drive N repeated cycles headless (robot runner), then `perf record
  -p <pid> --call-graph dwarf` an early window vs a late window and diff
  the top self-cost symbols (18% → 51% told the whole story). The robot
  binary keeps its symbol table (robot profile debug=0 but not stripped),
  so perf resolves names without a rebuild. Also: a "45s cooldown" is NOT
  frame-free in robot/NoVsync mode — the render loop free-runs, so don't
  use it to reason about frame-drained accumulators.

- Bubble-"misplacement" hunts: MEASURE THE REFERENCE FIRST. Hours went
  into an inboard end-clamp model invented from screenshots; one pixel
  scan of the reference frame (bubble center vs cell center, width vs
  pitch) settled the contract in minutes: cell-centered, width 1.10x
  pitch. Also two traps from the same hunt: (1) the "crown above the
  bar" was the tile artwork's own pale gradient + content seen through
  translucent glass — proven only by rendering the scene with the bar
  REMOVED; when an artifact survives every component-disable experiment,
  render the scene without the component before blaming the renderer.
  (2) Robot offscreen captures were geometry-faithful the whole time;
  the suspected presented-vs-offscreen divergence was an eyeballing
  error over busy artwork. Numeric scans on flat backgrounds beat zoomed
  eyeballing over gradients.

## macOS robot hangs need a portable timeout and exit must stop scheduling

macOS does not ship GNU `timeout`, so a continuously animating robot scenario
can otherwise consume a core indefinitely. Use the portable timeout in
`run_robot_test.sh`, cap focused debugging with
`CRANPOSE_ROBOT_TEST_TIMEOUT_CAP_SECS`, and reuse a warm build with
`--skip-build`. Also, after handling `RobotCommand::Exit`, return from the event
callback immediately: setting `ControlFlow::Poll` later in the same callback
keeps the animation loop alive even though the robot received `Ok(())`.

## Universal Android verification needs every declared Rust target

The release Gradle task builds arm64, x86_64, and armeabi-v7a in separate
`cargo ndk` passes. A machine can finish both expensive 64-bit AI builds and
then fail at the last pass with `can't find crate for core` if
`armv7-linux-androideabi` is missing. Before starting the release task, run
`rustup target add aarch64-linux-android x86_64-linux-android
armv7-linux-androideabi` and ensure `ANDROID_NDK_HOME` points at the installed
NDK. Also repair broken `~/.cargo/bin/{cargo,rustup}` proxies first; otherwise
Gradle's nested shell reports misleading `cargo: command not found` failures.

## Prefer in-process GPU telemetry for repeated physical-iOS profiling

Detaching an Instruments `xctrace` recording can leave the phone listed as
offline to `xctrace` even while CoreDevice and `devicectl` still communicate
with it. For renderer iteration, launch through `devicectl --console` with
`CRANPOSE_GPU_STATS=1`; it reports pass, isolated-layer, cache, and retained
texture counts without repeatedly attaching Instruments. When Instruments is
needed, use a unique `.trace` output path because `xctrace` will not overwrite
an existing bundle.

## Keep Android Vulkan-to-GL fallback in separate WGPU instances

An Android API 37 arm64 emulator can expose a Vulkan loader but no usable
render node. If Vulkan and GL are enabled in the same WGPU instance, adapter
selection falls through to GL after Vulkan probing and EGL can fail with
`native_window_api_connect ... already connected to another API`, followed by
`EGL_BAD_ALLOC`, an invalid surface, and `SIGABRT`. This looks like an app or
activity lifecycle failure, but it happens during the first
`Surface::configure`. Probe with a Vulkan-only instance first; on
`RequestAdapter` failure, drop that instance and create a fresh GL-only
instance. The GL software path can take about 20 seconds to initialize on this
emulator, so wait for `Rendering initialized successfully` before treating an
initial black frame as another failure.

## Wrong-node red herring in the menu gray-panel hunt (2026-07-18)

Hours were lost chasing "stale popup layer bounds" because backdrop-diag
lines for nodes sized 204.96x139.5 were assumed to be the open menu
popup. They were IN-PAGE glass cards from other stages; the popup's own
plan lines (580x282 -> 580x488) were correct and live the whole time.
Lessons:
- Before chasing a scene-staleness theory, CONFIRM NODE IDENTITY: match
  the diag rect against the widget's real spec size AND its window
  position, not just "a glass node with a backdrop".
- `sort -u` over a whole-run diag log mixes stages and frames — filter
  to the frames around one capture timestamp first.
- The probes that actually cracked it, in order of decisiveness:
  pure-red tint (is the material visible at all?), shader silhouette
  probe (where does the SDF land?), plan-size vs backdrop-size diff in
  CRANPOSE_BACKDROP_DIAG (`copied=false` every frame = the bug).
- An off-by-one CAN silently disable a whole material pipeline: the
  fallback path "worked" (no error, plausible blur), so nothing logged.
  When a fast path has an equality guard against a separately computed
  value, suspect the guard first and diff the two computations.

## Strip-downscale lies in cheatsheet judging (2026-07-18)

Two wrong verdicts in one round came from judging the composed TARGET|ACTUAL
strips instead of raw 2x capture frames: the segmented lens read as "milky
capsule washing the label out" (raw frames: face +2 gray levels over the
track, text crisp — the reference-matching state), and the touched-up held
ball read "dull teal" (raw probe: (0,217,234) electric cyan). The strip
downscale smears sub-pixel optics and mutes saturation. Always crop/zoom the
raw `shots/capture` frames and pixel-probe with magick before concluding a
material is wrong.

## Liquid-glass optics judged at capture scale 1 (2026-07-19, ~2 ticks lost)
The toggle dome's refraction band is `inradius * refraction_depth` ≈ 2-4px
at scale-1 robot captures. With ≤2 intermediate SDF pixels, `pow(interior,
refraction_curve)` has nothing to shape — curve, dispersion and zoom read
as byte-identical no-ops, which looks exactly like "the shader path is
dead". It is not: the reference recordings run ~5.4 px/dp. ALWAYS capture
glass-optics stages with CRANPOSE_ROBOT_CAPTURE_SCALE=2+ before judging
or tuning rim materials.

Compounding wasters from the same session, now protocol:
- Never mv/reuse capture dirs mid-bisection: one experiment = one uniquely
  named dir + a grep'd state echo beside it. Shuffled stale dirs produced
  false "knob does nothing" byte-diffs (a red-tracer frame even ended up
  in a dir labeled as a clean run).
- python str.replace silently no-ops on a missed needle: assert the needle
  is in the file before writing.
- Byte-diff two captures before reasoning about any knob's effect; never
  trust "looks identical" at small scales.

## Remote validation directories must be source-exact (2026-08-01)

Copying only `git ls-files` into a reused remote checkout leaves untracked
source files behind. `run_robot_test.sh` discovers every Rust runner in
`apps/desktop-demo/robot-runners`, so three runners from an earlier checkout
silently expanded a 140-test suite to 143 and produced a failure that could
not exist in the local tree. Matching hashes for tracked files does not catch
this condition.

Create a fresh remote source directory or compare its source-file inventory
against `git ls-files` before validation. Preserve the prior directory under
an `_old` name when cleanup is needed. Reuse expensive build artifacts through
an explicit target directory, not by reusing an unaudited source tree.
Do not combine an excluded `target` directory with `rsync --delete-excluded`:
that option deletes the build cache it appears to protect. A fresh source tree
with an explicit shared target directory makes source cleanup and artifact
retention independent.

When syncing several files from different source directories, never give one
directory destination to a single `rsync` invocation. Basename placement can
create a plausible stray `mod.rs` or module source in the wrong directory and
invalidate the remote build. Sync each file to its exact destination path, then
compare the remote source inventory before the expensive command.

## Score the exact cheatsheet tile, not a visually similar raw crop (2026-08-02)

The Liquid cheatsheet target sheets contain registered image bands, labels,
and grid gutters. A manually cropped frame from a nearby raw gesture strip
looked identical at normal zoom but placed the selected artwork eight pixels
lower than the tile consumed by `extract_reference_tiles`. Tuning against that
crop made the correct content placement measurably worse.

For RMSE work, extract the exact tile from the checked-in target sheet with the
same band/column registration as the robot contract, then normalize the actual
frame with the same dimensions and filter. Confirm the two candidate target
images by pixel comparison before interpreting geometry or content offsets.

## Native point-grid mismatches masquerade as material errors (2026-08-02)

The bottom-bar form runner registered two states from `tab_center - 70dp` and
two from the exact stage bounds. This introduced a repeatable 4–6 physical-px
shift at 3× and kept the sharp component score near 0.20 despite close-looking
material. The tab-swipe fixture separately declared a 132dp stage for a
1320×400 reference, producing a 1320×396 capture that was silently resized.
Always derive capture bounds from one semantic stage owner and express odd
native dimensions as physical-pixel ratios (`400.0 / 3.0`), then assert output
dimensions before tuning optics.

A zero-valued correction is not necessarily a no-op in a modifier-based layout
engine. Installing `.offset(0, 0)` for every tab changed raster rounding enough
to move a strict transfer RMSE across its gate. Optional optical corrections
must omit their modifier node when they are exactly neutral; numeric equality
inside an installed node does not guarantee structural or pixel equality.

## Visible shader contracts need a production presentation path (2026-08-09)

Bare `xvfb-run` creates a 640×480 display with roughly 30 Hz presentation on
this host. External shader drag, animation, and performance contracts can fail
there even when the renderer is correct because the demo window is clipped and
the expected presented-frame cadence is unavailable. Use the real X11 display
for production visual contracts. If isolation is necessary, explicitly create
an adequately sized Xvfb screen such as `-screen 0 1920x1080x24`, and do not use
its timing as production-performance evidence.

- Robot frame-pacing measurement trap (2026-08-12): a frame rate read from a
  robot run is not the app's unless the test pins the mode. `with_test_driver`
  lifts the pacing mode to `NoVsync` unless the harness called
  `with_frame_pacing_mode` (which sets `frame_pacing_explicit`), so a pacing
  test that starts in the default mode is already in the mode it means to
  switch into, and cannot tell a working control from a dead one. Pin the mode
  at launch, and check an absolute cap (`60fps` reads ~60) rather than only
  "NoVSync is fast": fast is also what a run does when nothing happened.
- Xvfb presents in tens of milliseconds, and the suite used to make you say so
  (2026-08-12): five presented-cadence robots --
  `robot_markdown_default_visual_contract`, `robot_shader_backdrop_drag`,
  `robot_shader_external_x11_drag`, `robot_shader_full_demo_external_perf`,
  `robot_shader_rect_external_animation` -- failed under
  `xvfb-run -a -s "-screen 0 1600x1200x24"` with `p95_present_ms≈25-30` and
  `cadence_fps≈35-45` against 120/150Hz contracts while `work_fps` stayed in the
  hundreds or thousands. They failed identically on a clean tree, which made them
  look environmental; they were not. CI passed because both robot jobs set
  `CRANPOSE_ROBOT_SOFTWARE_RENDERER=1`, which is what relaxes those assertions,
  and a hand-run suite did not. `run_robot_test.sh` now owns that decision
  (`CRANPOSE_ROBOT_FORCE_HARDWARE_PERF_CONTRACTS=1` enforces them), so a plain
  `./run_robot_test.sh --sequential` is green and means the same thing as CI.
  Do not conclude "environmental" from "fails on clean main too" -- a shared
  harness mistake fails on clean main as well. The presentation cost is real
  though: `robot_novsync_free_runs` reads ~160fps under Xvfb and ~2500fps on a
  real display with the loop in pure `Poll` (`CRANPOSE_PACING_DIAG=1` shows
  `poll=55 wait=0` — 55 iterations a second because each waits out a present),
  so judge loop pacing on the cadence-to-`work_fps` ratio, never on cadence.
- Windowed Fifo on this box blocks a whole second per frame (2026-08-12):
  running a robot example against `DISPLAY=:0` in `VSync` (Fifo) crawls at ~1fps
  with `present_ms≈998` in `CRANPOSE_DESKTOP_FRAME_TELEMETRY_MS=1` telemetry --
  the X display has no consumer for the swapchain, so every present waits out
  its timeout. It looks exactly like a pacing bug and is not one. Run robot
  examples under `xvfb-run -a -s "-screen 0 1600x1200x24"` (as CI does);
  `run_robot_test.sh` needs a DISPLAY even for `CRANPOSE_HEADLESS=1`, because
  winit still builds an event loop. A windowed `NoVsync` probe is unaffected
  (Immediate never blocks), which is why only the capped modes hang.
- Robot-driven cadence numbers changed with the #377 fix (2026-08-12): a driven
  run no longer pins `ControlFlow::Poll` for its lifetime, so external perf
  contracts now measure what production does rather than the harness spinning.
  `robot_shader_external_x11_drag` went from `frames=892 fps=1215.9` to a
  deterministic `frames=84 fps≈310-355` (2 frames per xdotool motion step) with
  identical per-frame work (`p95_total_ms` 0.81 -> 0.86). Both clear the
  contract (>=48 frames, >=150fps, p95 <= 6.67ms); the margin is smaller because
  the number is now real. Do not "restore" the old figure by re-forcing polling.
- Backdrop-under-a-transform bugs are invisible from the source (2026-08-13):
  the bottom bar's press lift showed the world behind its glass shrunk ~20% and
  slid toward the bar's corner. Reading the underlay path end to end argued the
  code was *correct* — the underlay is copied from the child's post-transform
  dest quad and squeezed into its logical rect, which the composite unsqueezes,
  so registration looked like it cancelled out. What actually cracked it, in
  order: an A/B on the running app (zero the `graphics_layer` lift, rebuild,
  re-measure: the drift vanished), then `CRANPOSE_BACKDROP_DIAG=1` on the real
  binary, whose `prepare node=…` lines showed the same backdrop being prepared
  in *root* space at rest and in *surface-local* space (`y: 31.31`) while
  pressed, with a scissor implying scale 1.693 against a root scale of 1.354 —
  the missing 1.25 is `magnifying_layer_scale`, which quantizes a scaling
  layer's surface UP to the next quarter step. The underlay was built at the
  parent's scale and addressed at the child's. Measure the displacement before
  theorizing: two background lines through the glass gave scale 0.688 about a
  fixed point, which is what identified the culprit as a *rect ratio* rather
  than the 1.03 lift everyone would suspect.
- A magnifying layer needs BOTH halves in a renderer test (2026-08-13): the
  first regression test for the above passed against the bug. `test_support::
  layer_node` takes `transform_to_parent` and `graphics_layer` independently, and
  the surface plan reads them from different places — `NonTranslationTransform`
  (which is what makes the layer take a surface at all) comes from the
  *transform matrix* via `direct_translation`, while the magnification factor
  comes from `graphics_layer.scale` via `layer_surface_scale`. A
  `GraphicsLayer { scale: 1.25 }` beside a plain translation transform is a
  layer that never isolates, so the whole path under test is skipped. Build the
  transform with `layer_transform_to_parent(bounds, placement, &layer)` — the
  same call the scene builder makes — and pass the same `GraphicsLayer`.
- Cancelling `publish.yml` mid-flight leaves main bumped and the tag stale
  (2026-08-14): the workflow's first act is to rewrite the workspace versions,
  commit `release: vX.Y.Z` to `main` and move the tag onto it; crates.io comes
  later. Cancelling between those — the right call when the commit being
  released has an unexplained CI failure — leaves `main` at the new version,
  crates.io at the old one, and the tag pointing at the PRE-bump commit, which
  is no longer `main` HEAD. Re-pushing that tag then fails the workflow's own
  "Tag must be created from main HEAD" check.
  The recovery is not a manual dispatch: move the tag onto the `release:`
  commit (`git tag -f -a vX.Y.Z <release-commit>`, `git push -f origin vX.Y.Z`)
  and let the tag-push path run again. The version rewrite is idempotent, so the
  second run hits `git diff --quiet`, prints "No version changes needed", and
  goes straight to publishing; the publish job checks out the default branch
  rather than the tag, so it builds the bumped tree either way. Ends with the
  tag on the `release:` commit, which is where every other release's tag sits.
- `robot_text_handle_cycle_stability` flakes under full-suite load (2026-08-14):
  it failed the `main` robot job on the v0.1.89 commit and passed on a rerun of
  the same job on the same box, and passes locally in isolation. Its final
  assertion is `late_work > early_work * 1.4 + 0.15` over select/sweep/release
  cycles — an *accumulation ratio*, so host contention late in a long suite trips
  it while a constant per-frame cost (the GPOS kerning landing in the same
  commit) lifts both sides equally and cannot. Rerun the job before treating a
  failure here as a regression, and do not "fix" it by widening the bound: the
  ratio is the guard.
- No local gate compiles the browser, and a release is where you find out
  (2026-08-14/15): v0.1.91 published to crates.io with `cranpose-render-wgpu`
  failing to build for wasm32. `cargo test --workspace`, `cargo clippy
  --workspace --all-targets` and `cargo check --workspace --all-features` all
  build the HOST target and were green. The only thing in the tree that compiles
  the browser is `apps/desktop-demo/build-web.sh`, which runs in the `wasm build
  (linux)` CI job — so the break surfaced from the Pages deploy of a release
  that had already published. Two causes, same shape: `init_gpu` grew a
  `DownlevelFlags` argument and every host caller was updated except `web.rs`,
  and `static_span_stats` read a `#[cfg(not(target_arch = "wasm32"))]` field
  without carrying the gate its siblings all have.
  Run `./build-web.sh --fast` before every tag. And note that `gh pr merge
  --admin` bypasses the `wasm build (linux)` job, which is exactly the check
  that would otherwise catch this class.
- The release tag must be at `main` HEAD when the workflow READS it, not when
  you create it (2026-08-14/15): merges land continuously here, and three
  separate releases hit "Tag vX.Y.Z must be created from main HEAD" because
  something merged in the seconds or minutes after tagging. The recovery is
  cheap — verify the new HEAD, `git tag -f -a`, `git push -f origin <tag>` —
  and the workflow's version rewrite is idempotent, so the second run
  short-circuits at "No version changes needed" and publishes. The trap is
  assuming a queued Publish means it is fine. Also watch for a branch that
  hand-bumps the workspace version (#427 carried "Version 0.1.92 across the
  workspace"): harmless while the numbers agree with the tag, a conflict the
  moment they do not.
