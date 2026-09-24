# Time Wasters

- Fix reproducible bugs with a failing regression test; remove their notes once the fix is verified.
- Put general agent rules in [AGENTS.md](AGENTS.md), with one short rule per line.
- Put build, shell and CI details in [development troubleshooting](docs/development_troubleshooting.md).
- RustRover MCP edits land in the project the IDE has open, whatever `projectPath` says; open a worktree in RustRover before editing through it and check `git status` in both trees.
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
- Refresh all incoming source timestamps when alternating checkouts on one Cargo target directory; otherwise cached embedded WGSL can survive a source switch.

- Move Settings sliders beyond touch slop before fine corrections; tiny drags can leave the thumb unchanged.
- Robot runners must forward the explicit reference-content settings and assert the fixture; a sanitized environment can silently turn a green-icon check into the default blue-icon scene.
- Shader-only render fixtures must request shader-owned coverage when their graph omits the material's outer clip.
- Diagnose single-channel pixel drift before rounding with shader bit probes; face/rim compositing can change fused multiply-add order even when both inputs match.
- Fetch `origin main` before building device A/B APKs; a stale local main re-measures bugs upstream already fixed and blames the change under test.
- Build both arms of a device A/B on one host: debug-signed APKs from different machines refuse `adb install -r` with `INSTALL_FAILED_UPDATE_INCOMPATIBLE`, and every leg of that arm records nothing.
- The composed-drag replay over `TabBackdrop::Flat` carries no glyph ink (transparent icons, empty labels): a byte-exact replay proves nothing about ink colour; audit ink changes on the phone checkerboard recording.
- Judge replay byte exactness on the second run after a rebuild: the first run draws up to 13 frames through the compile-time fallback pipelines, one level off, and reads as an inexact change.
- A three-content matrix recording runs past XCTest's two-minute execution allowance and keeps no movie; the device-run recipe disables test timeouts, and a Cranpose matrix run must replay the native `planned_events` through `TEST_RUNNER_REFERENCE_POINTER_PATHS`, since a content case's native button sits 1.5 pt elsewhere and the comparison rejects unequal paths.
- A CI board red on every mac step at once (`linking with cc failed: exit status 69`) is macm3's Xcode licence after an update, not the change; read one step's log before touching code.
- The browser-safe-time static test scans `cfg(test)` modules of the wasm-delivered crates too: import `web_time::{Duration, Instant}` in wgpu crate tests, never `std::time`.
- A composite rule gated on "translated" silently drops for a scaled child: the showcase star's pulse showed the glass band as a square because the rounded mask required translation; gate on the geometry the mask needs (uniform scale plus translation), and reproduce with a scratch showcase copy patched to the local crates.
- Android accessibility reconnect tests require no active user accessibility service; use a dedicated device when a shared control service keeps accessibility enabled.
