# Time Wasters

- Fix reproducible bugs with a failing regression test; remove their notes once the fix is verified.
- Put general agent rules in [AGENTS.md](AGENTS.md), with one short rule per line.
- Put build, shell and CI details in [development troubleshooting](docs/development_troubleshooting.md).
- Resolve the exact checkout in RustRover before semantic edits, pass the canonical `projectPath`, and verify the checkout's `git status`; use direct file tools for routine edits under the [code tool rules](AGENTS.md#code-tools).
- Workspace dependency patches do not reach crates.io consumers; validate required upstream fixes against the published dependency graph before tagging.
- Use a build cache from one compiler for compile-fail checks; newest-artifact lookup can otherwise select an incompatible proc macro.
- On samarch-1, export `RUSTUP_TOOLCHAIN` from `rust-toolchain.toml` and use a fresh target directory for a new checkout or a patched consumer app; registry crates otherwise build with the host default and fail with E0514. `cargo update` the patched crates, or the lockfile keeps the published ones.
- iOS keyboards can expose individual keys without a Keyboard container; verify editable focus, text entry and the saved value.
- Verify that VoiceOver receives audit commands; app-directed automation can insert their text while VoiceOver is enabled.
- Keep device evidence outside build caches; garbage collection must not erase the only speech transcript or regression result.
- Put robot and image-check details in [render verification](docs/render_verification.md).
- Put Android measurement details in [device measurement](docs/device_measurement.md).
- Reuse [mobile performance evidence](docs/mobile_watch_performance.md) before repeating experiments; keep one conclusion and evidence link per row.
- Keep unresolved, measurable work in GitHub issues; remove fixed incidents and duplicate advice from this file.
- Extract XCTest video by presentation timestamp; sparse idle frames require the previous recorded frame, not a seek to the next frame ([tab reference](apps/liquid-reference/README.md)).
- XCTest H.264 can repeat PTS and make FFmpeg switch its best-effort clock to DTS; retain original pts_time and verify alignment with a visible clock before labeling animation frames.

- Do not clear `UITabBar.selectedItem` inside a SwiftUI `TabView` capture; SwiftUI terminates the app when its selection invariant is broken.

- Compare optical contours after both shapes settle; transient shape exchange changes the join even when the optical field is unchanged.

- Native CAFilter experiments must copy the filter, mutate the copy, and replace the layer filter array; in-place mutations can leave rendered pixels unchanged.
- Reuse live native SDF models for isolated probes; archiving their portal layers loses the source shape.
- Native key/fill curvature controls a linear depth ramp with a separate cutoff, not an exponent.
- Check both axes with lossless step probes before inferring chromatic sampling from quadrature phase alone.
- Compare composed contact keyframes; an isolated spring can pass while its real animation starts two frames after the input event.

- Snapshot animated native filters before archiving them; lazy presentation reads during serialization invent phase differences between layers.
- Isolate native glass filters only after their geometry settles; moving an animated backdrop freezes its transient transform into every later kernel sample.
- Replay pointer targets through the composed component; draining animation callbacks before every test input hides retrospective target application between display frames.
- Pausing the window CALayer does not freeze all UIKit glass effects together; use captured presentation frames for contact comparisons.

- Match native presentation geometry to `CADisplayLink.timestamp`, not the next frame target; verify the choice against the recorded pixels.
- Check native font tracking as well as variation axes and kerning before adjusting caption widths.
- Trace the glyph portal separately from the glass backdrop; uniform content magnification omits its edge displacement.

- Inspect native layer bounds and ancestor transforms separately: resizing a capsule does not reproduce affine projection of its optics and content.
- Check CAPortalLayer matchesTransform before projecting foreground glyphs; native content can cancel an ancestor’s stretch.
- Validate a distance-field fit against the composed native image; a lower isolated field error can worsen refraction.
- Preserve both recordings' full time ranges and check contact/release bounce plus the complete drag rebound; a shared time intersection or short slide strip can conceal shape failures.
- Preserve all delivered touch samples and verify the direction sequence; reducing a scrub to one movement segment erases reversals.

- ProMotion tracing needs `CADisableMinimumFrameDurationOnPhone` in the actual application plist as well as the display-link frame-rate range.
- Track native selection layers by active/resting hierarchy and subtract common bar translation before interpreting strain.
- Capture the window tree for glass pane filters; the broad pane can live outside the tab bar subtree.
- Keep Cargo target directories separate for A/B checkouts and verify artifact hashes; a shared directory can reuse the other checkout's libraries or embedded shaders.

- Move Settings sliders beyond touch slop before fine corrections; tiny drags can leave the thumb unchanged.
- Robot runners must forward the explicit reference-content settings and assert the fixture; a sanitized environment can silently turn a green-icon check into the default blue-icon scene.
- Shader-only render fixtures must request shader-owned coverage when their graph omits the material's outer clip.
- Diagnose single-channel pixel drift before rounding with shader bit probes; face/rim compositing can change fused multiply-add order even when both inputs match.
- Fetch `origin main` before building device A/B APKs; a stale local main re-measures bugs upstream already fixed and blames the change under test.
- Build both arms of a device A/B on one host: debug-signed APKs from different machines refuse `adb install -r` with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, and every leg of that arm records nothing.
- The composed-drag replay over `TabBackdrop::Flat` carries no glyph ink (transparent icons, empty labels): a byte-exact replay proves nothing about ink colour; audit ink changes on the phone checkerboard recording.
- Judge replay byte exactness on the second run after a rebuild: the first run draws up to 13 frames through the compile-time fallback pipelines, one level off, and reads as an inexact change.
- `capture_reference_tests.sh` runs the twin already installed on the simulator: build it with `apps/liquid-reference/build-cranpose.sh` and `xcrun simctl install` it first. A stale twin exits with status 1 on a component it does not know, and every test then fails at launch. Keep each XCTest under its two-minute allowance: one launch takes about 8 seconds.
- A three-content matrix recording runs past XCTest's two-minute execution allowance and keeps no movie; the device-run recipe disables test timeouts, and a Cranpose matrix run must replay the native `planned_events` through `TEST_RUNNER_REFERENCE_POINTER_PATHS`, since a content case's native button sits 1.5 pt elsewhere and the comparison rejects unequal paths.
- A CI board red on every mac step at once (`linking with cc failed: exit status 69`) is macm3's Xcode licence after an update, not the change; read one step's log before touching code.
- The browser-safe-time static test scans `cfg(test)` modules of the wasm-delivered crates too: import `web_time::{Duration, Instant}` in wgpu crate tests, never `std::time`.
- A composite rule gated on "translated" silently drops for a scaled child: the showcase star's pulse showed the glass band as a square because the rounded mask required translation; gate on the geometry the mask needs (uniform scale plus translation), and reproduce with a scratch showcase copy patched to the local crates.
- Android accessibility reconnect tests require no active user accessibility service; use a dedicated device when a shared control service keeps accessibility enabled.
- android-activity's `AndroidApp::config()` keeps the configuration NativeActivity read at launch; read a value that changes while the app runs, such as night mode, from the activity's `Resources.getConfiguration()`.
- Stopping `scripts/ci/with_host_lock.sh` by its PID leaves its `host_flock.py` child with the host lock: every CI job and build on the host waits behind it. Stop that child by its own PID too.
- A fresh Chrome profile fetches a variations seed on its first start. The seed invalidates V8's wasm code cache, so the bench's second start compiles the wasm again. A trace that needs a cached second start passes `--variations-server-url=http://127.0.0.1:9/`.
- `cargo fmt --all` fails in a worktree under `.claude/worktrees`: cargo looks above the worktree for the workspace of `forks/wgpu/wgpu-hal` and finds the main checkout's `Cargo.toml`. Run `cargo fmt` without `--all` there; `just fmt` and the `just precommit` hook fail there for this reason.
- `espup install` repoints `~/.espup/esp-clang`, which esp-idf-sys reads instead of LIBCLANG_PATH; the ESP workspaces set `ESP_IDF_ESPUP_CLANG_SYMLINK = "ignore"`. A fresh target that fails with `no field _stdin on type _reent` took libclang 22 through bindgen 0.71: repoint the link, update embuild to 0.33.7 (bindgen 0.72) and run `cargo clean -p esp-idf-sys`.
- Under LTO a small helper shared by two hot paths can stop inlining: one shared by the modifier chain's build and reconcile cost 16.7 M instructions on the wall's first frame. Count the callgrind harness again after a refactor that shares a hot helper.
- `python3 -I` leaves the script's own folder off `sys.path`: `benchmarks/compose-vs-cranpose/ab.py` then fails to import `measure`. Run the bench's own scripts without `-I`.
- zsh reads `$name:first` as a variable with a history modifier and stops with "bad substitution": write `${name}:first`, or run the command from a bash script.
- One A/B round of four pairs on the Mate can show a 1 % CPU-a-frame difference that the next round reverses (#2225, #2226). Run a second round before you call a 1 % change real.
- An iOS simulator's Safari gives a page no WebGPU adapter, so it tests the WebGL path only. `xcrun simctl spawn <udid> defaults write -g LDMGlobalEnabled -bool YES` turns Lockdown Mode on (no WebAssembly, WebGL or WebGPU); `defaults write com.apple.mobilesafari WebKitJavaScriptEnabled -bool NO` turns JavaScript off.
- Three agents building at once filled the M5's disk from 43 GB free to none in about an hour: each worktree's target grew past 25 GB with clippy for every target, and Claude Code then started no command at all. Check `df -h` before a build on the M5 and start none below 30 GB free; put repeated trees on samarch-1 or on macm3's `/Volumes/files`.
- The Mate launches the plain bench package `dev.perfcompare.cranpose` 8 to 16 ms later than `.release` with the same APK (#2059). A launch A/B must place each build in both packages, or run an A/A, before it calls a difference real.
- A wait loop `until ! pgrep -f "PATTERN"; do sleep 5; done` run through `zsh -c` or `bash -c` never ends: `pgrep -f` matches the shell whose own command line holds PATTERN. One ran 8 hours on samarch-1. Wait on a PID with `wait` or `tail --pid`, or put the pattern in a script file.
- Chrome A/B runs and builds on macm3 beside CI pushed two jobs on `dmitriis-mac-Cranpose` past their 20-minute limit. Before a run on macm3, check that `gh run list` shows no job on its runners, or run on samarch-1.
- This repository sets `rebase.updateRefs`: a rebase also moves every other local branch that points into the rebased commits, even from a detached worktree. Pass `--no-update-refs` when those branches belong to another agent.
- Add a `use` line above an attributed `use`, never between the attribute and its item: the attribute moves to the new line, and `just fmt` merges the stripped item into the neighbouring list. A host build with the attribute's feature on passes; the ESP32 and wasm builds fail.
- `rsync -a` keeps the source's file times. A file that git wrote before the last build on the far host ended then looks older than that build's outputs. Cargo then builds and tests the old code there: the test log shows no new test. Sync a source tree with `rsync -rlpc` (no `-t`), as `scripts/perf/steady_ledger.sh` does, or extract it with `tar -m`.
