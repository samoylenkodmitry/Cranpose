# Time Wasters

- Fix reproducible bugs with a failing regression test; remove their notes once the fix is verified.
- Put general agent rules in [AGENTS.md](AGENTS.md), with one short rule per line.
- Put build, shell and CI details in [development troubleshooting](docs/development_troubleshooting.md).
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
