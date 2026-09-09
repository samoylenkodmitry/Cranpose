# Render verification

- Keep measured wheel routes inside scroll bounds; elastic overscroll consumes reverse input, so verify the actual start and return positions.

## Harness and capture

- Start from [robot testing](ROBOT_TESTING.md); run placement-sensitive runners through `run_robot_test.sh`.
- Linux robot event loops require DISPLAY even headlessly; use CI's Xvfb configuration for correctness and a physical display for FPS.
- Set explicit Xvfb screen dimensions and test fractional scale separately; scale-1 assertions do not cover Xft.dpi-derived density.
- macOS windowed tests need an awake compositor; distinguish present waits from unsettled composition using the diagnostic fields.
- External X11 captures must verify `_NET_WM_PID` and position the window on an accessible monitor before input.
- Native wheel injection and split press/move/release sequences have separate failure modes; verify the actual gesture changed content.
- Reuse comparison targets with explicit `--baseline-target` and `--current-target`; changing labels otherwise rebuilds both versions.
- `robot.screenshot()` redraws offscreen; inspect swapchain artifacts with a windowed runner and an external capture.
- Use exact button semantics for clicks and log resolved bounds; substring presence matches can select unrelated offscreen text.
- Assert popup presence through semantics; translucent white-on-white pixels do not support arbitrary difference floors.
- Set explicit frame pacing before `with_test_driver`; its NoVsync default invalidates normal-VSync FPS measurements and mode-switch tests.
- Use deterministic animation clocks; large screenshot captures and sequential move/capture loops disturb continuous motion.
- Check scene state and motion before cache counters; a scroll clamped at offset zero can make every cache assertion pass.
- Compare each incrementally scrolled picture with a fresh render at the same position; featureless glass lanes cannot establish motion.
- Reproduce the actual failing page headlessly at CI density before inventing simplified fixtures; see `liquid_scroll_phase.rs`.
- Lock GPU tests before changing process-global debug toggles and restore them before releasing `support::gpu_test_lock`.
- Treat GPU-driver failures as hypotheses until the same binary, adapter and host conditions are checked; a clean-main failure alone proves no cause.

## Pixels and renderer contracts

- Measure original captures at native scale with a common internal anchor; crops, shadows and resized comparison sheets can mislead.
- Verify capture dimensions and recapture after the final edit; one experiment uses one immutable output directory.
- Use scale 2 or higher when a glass fixture needs several pixels across its refraction band.
- Identify a diagnostic node by size, position and frame before reasoning about its cache or geometry.
- Trace state values through update, layout, scene construction and rendering to locate the first stale result.
- Inspect attachment formats when solids wash out but sampled images match; preserve the byte-exact color contract.
- `Modifier::size` obeys incoming constraints; intentional overflow uses `required_size`.
- Validate WGSL through the renderer's shader-cache tests; UI-only tests do not compile the shader.
- Compare optimizations with frozen independent shader references; two paths sharing changed arithmetic can agree on the same wrong pixels.
- Keep format-matched references: float-attachment goldens cannot validate forced 8-bit output by widening their tolerance.
- Half-float bilinear identity captures can round; use `textureLoad` only when the fixture's contract is an exact identity read.
- Blurred-backdrop mutants expose missing reads that destination blending can reconstruct over a sharp background.
- Assert positive frost and zero activity independently when testing unused substrates; removing a substrate can still change capture geometry.
- Shader-stage changes, triangle interpolation and neutral modifier nodes can alter fractional-scale rounding; require exact device parity.
- Verify specialized pipelines actually drew before comparing them with general pipelines.
- Ownership checks must include both command recordings and retained shape columns; outer ownership alone does not prove reusable storage.
- Make invalidation bypasses impossible through ownership and private fields; test moved solid siblings without full-rebuild fallback.
- Proc-macro-generated syntax uses the macro crate's edition; mixed-site hygiene does not isolate bindings from call-site constants.
- Test composable doctests with their required crate-root imports and explicit main; downstream examples have a different expansion context.
