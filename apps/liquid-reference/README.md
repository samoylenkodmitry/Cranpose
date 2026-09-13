# Native floating tab bar reference

`LiquidReference` is a SwiftUI app using the system `TabView`, with an iOS 26 deployment target. The running OS supplies its material, lighting and touch animations. `CranposeLiquidReference` renders the same four destinations with `LiquidTabBar`. Both have an 8-point rainbow checkerboard extending behind the bar.

The native app's **Inspect animation keyframes** button opens a paired frame inspector with a slider, previous/next frame buttons, phase jumps, and 0.1×–1× playback. Build the comparison resources before testing this inspector. Its screenshots are captured evidence; the live bar remains the actual system control.

`REFERENCE_SCHEME=dark` and `REFERENCE_BACKDROP=solid` select the other surface checks. `REFERENCE_RECORDING=1` enables a clock barcode during automated captures. Cranpose runs a frame clock and a transparent, non-consuming input observer while recording. This instrumentation makes the fixture unsuitable for production frame-rate measurements. The native live probe records touch samples and presentation-layer geometry in its Documents directory. Undefined native corner radii remain explicit `NaN` values in the JSON.

## Build and record

Use Xcode 26, an iOS 26 simulator, `just`, Python 3 and FFmpeg. The Rust simulator target is `aarch64-apple-ios-sim`. Run these recipes from the repository root:

```sh
just liquid-reference-build
just liquid-cranpose-build
xcrun simctl install booted target/aarch64-apple-ios-sim/release/CranposeLiquidReference.app
just liquid-reference-test 'platform=iOS Simulator,id=YOUR_SIMULATOR_UDID' target/native.xcresult TabBarTests
just liquid-reference-traces target/native-traces
just liquid-reference-test 'platform=iOS Simulator,id=YOUR_SIMULATOR_UDID' target/cranpose.xcresult CranposeTabBarTests
just liquid-reference-traces target/cranpose-traces io.cranpose.liquid-cranpose
just liquid-reference-keyframes target/native.xcresult target/native-frames target/native-traces
just liquid-reference-keyframes target/cranpose.xcresult target/cranpose-frames target/cranpose-traces CranposeTabBarTests
just liquid-reference-bundle target/native-frames target/cranpose-frames target/tab-comparison
just liquid-reference-build
just liquid-reference-test 'platform=iOS Simulator,id=YOUR_SIMULATOR_UDID' target/inspector.xcresult KeyframeInspectorTests
just test-liquid-reference-tools
```

Open `target/tab-comparison/index.html` for the browser inspector. `liquid-reference-compare` creates this report without bundling its frames into the native app. `liquid-reference-bundle` also copies PNG crops from the screen recording and their manifest into the ignored `LiquidReference/CapturedFrames` directory. Keep result bundles and output directories unique; the tools refuse to overwrite recorded evidence. Preserve an existing `CapturedFrames` directory under a different name outside the application source before bundling a new comparison.

The Rust builder accepts `aarch64-apple-ios` for a device binary. Physical installation additionally needs signing and provisioning. Device UI automation requires USB and Settings → Developer → Enable UI Automation. Simulator recordings do not establish physical-display FPS, hardware input latency, or haptic parity.

## Interaction protocol

The same XCTest implementation runs against both applications. Each recording includes both directions at both 250 and 1,000 points/second: touch down, hold for 800 ms, slide, stop, hold for 1.2 seconds, then release. The test waits another 800 ms after release. Native and Cranpose record the observed positions and input times; the synthesized path is retained separately. Additional tests cover taps, repeated selection, dragging outside, holding an unselected tab, and light/dark surfaces on both backgrounds. Destination and committed accessibility selection are asserted after release.

The extractor retains every decoded frame and its original `pts_time`, including duplicates and small reversals in decoder output order. The timeline sorts presentation times while preserving source ordinals. Do not substitute FFmpeg's `best_effort_timestamp_time`: XCTest recordings can have repeated presentation timestamps, causing the decoder to switch to decode timestamps partway through a movie and shift the apparent action by more than a second. Cropped image export assigns a monotonically increasing output sequence only to avoid muxer timestamp errors; the JSON retains each source frame's original presentation timestamp.

A barcode at screen coordinates (16, 100), 128×8 points, contains an 8-bit magic value and the low 24 bits of the wall clock in milliseconds. Changing markers establish the movie-to-input clock offset. The extractor rejects missing or inconsistent markers and incomplete touch paths. Each report exposes clock sample counts and the 95th-percentile alignment residual. Both timelines use application touch delivery time. Native traces retain UIKit event time separately; mixing that earlier time with Cranpose delivery time creates a false latency difference. Native recordings made without delivery timestamps must be recorded again. Phase boundaries come from observed input, including the first and last moving samples. Their precision is limited by input sampling, rendering, recording cadence and the reported clock residual.

The paired timeline merges both sets of source frames, retaining duplicate timestamps as separate steps. It compares equal elapsed time after touch down and never aligns against the first visible lens response. Step through individual frames to inspect transitions; use the pixel-difference view to expose spatial and material differences.

## Optical probes

Run `just liquid-reference-optical-capture YOUR_SIMULATOR_UDID target/optical-probes` in one terminal, then `just liquid-reference-test 'platform=iOS Simulator,id=YOUR_SIMULATOR_UDID' target/optical-probes.xcresult NativeOpticsTests` in another. The collector follows the installed app's current data container, captures all 35 lossless PNGs during stable holds, and retains each pattern, capture interval and native presentation tree. It rejects missing frames and screenshots that cross the hold interval. Each pattern is visible before touch-down; each hold starts with Discover selected and presses Account. The normal application still defaults to the rainbow checkerboard.

Analyze with `just liquid-reference-optical-analyze target/optical-probes target/optical-analysis`. The analysis requires NumPy and Pillow; pass a Python executable as the recipe's third argument when those packages live in a separate runtime. It verifies capture completeness, geometry stability and the unobstructed source patterns, hashes its inputs, and exports RGB quadrature responses and phase-equivalent source coordinates. A phase-equivalent coordinate is not necessarily one optical ray: filtering and shared color contributions can move it as pattern frequency changes. The checkerboard is held out of phase recovery.

Diagnostic suites isolate individual stages. `NativeKernelOpticsTests/testForegroundX0` through `testForegroundY3` capture the chromatic filter over quadrature waves; `testSpectralStepX0` through `testSpectralStepY3` capture quarter-pixel step phases; `testBackgroundX0` through `testBackgroundY3` capture the inner warp. Select one family per output directory because each uses indices 0…7. Pass `--count 8` directly to `capture-optical-probes.py`, or a smaller count when selecting individual XCTest cases. `NativeLayerOpticsTests` removes individual filters or lights; `NativeSDFOpticsTests` exposes the live distance-field effects. `NativePaneKernelOpticsTests` isolates the broad pane over eight quadrature waves. `NativeContactKernelOpticsTests` records ten step images at five fixed contact states. `NativeContactFilterTests` records filter values at nine input times from 25 ms to one second; these JSON files store the capture interval and do not require the screenshot collector. `NativeAdaptiveOpticsTests` and `NativeDarkAdaptiveOpticsTests` capture seven flat luminance controls. `NativeAdaptiveBoundaryTests` and `NativeFineAdaptiveBoundaryTests`, with their dark variants, capture eight controls around tone transitions. `NativeChannelOpticsTests` captures individual red, green and blue quadrature waves. `NativeBackgroundOpacityTests` isolates background-face opacity at three values over two wave axes. `NativeGlowMaskTests`, `NativeInnerShadowOpticsTests`, and `NativeLensShadowOpticsTests` isolate lighting masks and shadows; `NativePaneImageOpticsTests` captures ten pane inputs without the raised lens. Capture success requires both the expected PNGs and passing XCTest results. Preserve the installed binary hash and Swift source hashes alongside each capture before rebuilding.

## Implementation and remaining differences

### Continuous reversals and physical iOS 27 calibration

Select `NativeReversalTests/testInteractionKeyframes` or `CranposeReversalTests/testInteractionKeyframes` for one uninterrupted left → right → left path. It starts on Account, holds for 0.8 s, moves at 1,000 / 250 / 1,000 pt/s, holds for 1.2 s, then releases and records two seconds of settling. `REFERENCE_INITIAL_DESTINATION` selects the starting tab in both fixtures. The XCTest target synthesizes a single pointer path and attaches its archive; chaining three public drag calls would lift the finger between legs. The extractor retains every delivered touch sample and rejects missing reversals or incorrect endpoints. It stores the device and OS from the result bundle, and comparison rejects different capture environments, viewports or planned paths.

Physical iPhone tests require USB and Settings → Developer → Enable UI Automation. `liquid-reference-device-test DEVICE TEAM RESULTS SUITE` signs, builds and runs the requested suite. `liquid-reference-device-traces DEVICE OUTPUT [BUNDLE]` copies native or Cranpose touch/layer traces from the device using CoreDevice. `SystemGlassTests/testInspectSettings` captures the actual Settings screen and accessibility tree for inspection.

The iOS 27 validation matrix is 0%, 20%, 50%, 75% and 100% in the system **Tint Amount** setting. `NativeMatrix*Tests` set and assert the actual Settings value before capture; `CranposeMatrix*Tests` assert the corresponding framework theme value. Each test repeats the continuous left–right–left gesture with the original content, changed labels and symbol assignments with a magenta accent and a different color palette, and another label/symbol set with a green accent and a grayscale backdrop. The same `REFERENCE_CONTENT` JSON configures both apps and is retained in the capture metadata. Restore the user's 25% preference afterward. These capture and control checks establish coverage; the exact pixel audit determines image equality. Existing iOS 26.5 optical fixtures do not establish iOS 27 behavior.

The component uses overlapping native cell geometry, spring-following drag position, contact growth, and independent width/height responses fitted to native presentation traces. The raised material now composes two native displacement profiles, seven chromatic samples, and separate key/fill lighting. The native lens's central background remains unscaled; Cranpose refracts the glyph mask through its separate 14-point content field and enlarges it by the native 1.16× content ratio. Surface refraction remains active after release. The reference fixture uses native template symbols and the system sans-serif font at UIKit’s exported optical-size and weight-axis coordinates, including its size-dependent tracking.

Foreground content is inserted after the inner warp and face tint, before chromatic filtering and edge lighting. This lets the same seven-tap spectrum separate icon and caption channels when the rim approaches them. The settled tint stays below the glyphs. The material cache shares both layer configurations and their geometry; ordinary glass retains its complete effect chain.

The batched blur renderer consumes physical radii without multiplying display density twice. A separate renderer fix makes queued text draws retain the atlas binding used for their glyph coordinates, so atlas growth cannot make earlier labels disappear. Tests cover the native cell bounds, full gesture sequences, surface refraction, motion traces, exact shape settling, and shader specialization parity.

The comparison does not establish pixel parity. Remaining differences include thin contour bands, the spatial blur profile, caption rasterization, and fast motion. Light/dark tone and foreground color transfer have independent native calibration fixtures. [Recorded measurements](MEASUREMENTS.md) distinguish the measurements, fitted model and remaining acceptance criteria.

Apple's [Liquid Glass overview](https://developer.apple.com/documentation/TechnologyOverviews/adopting-liquid-glass) explains the use of standard system components; its [materials guidance](https://developer.apple.com/design/human-interface-guidelines/materials) describes their foreground/background relationship.

The physical geometry trace requests the display maximum frame rate and enables `CADisableMinimumFrameDurationOnPhone` in the application plist. Median captured cadence is 8.334 ms. Pixel recordings are 30 fps. The inspector retains every original pixel frame, shows its timestamp and alignment gap, and includes touch-down, both reversals, stop, hold, touch-up and settling. Do not present reconstructed 120 Hz geometry as 120 fps captured imagery.

The Cranpose reference exposes 0/20/25/50/75/100 percent buttons and a live tint label. Cranpose tint tests set `REFERENCE_MATERIAL_PROFILE` and assert the framework label; native tests verify the system Settings slider. The comparison percentage identifies the component setting in either renderer. Changing the phone setting is unnecessary for the custom Cranpose shader. `CranposeAppearanceTests/testTintButtonsUpdateTheFrameworkTheme` checks the live control and leaves it at 25%.

## Exact pixel audit

Run `just liquid-reference-pixel-audit target/tab-comparison target/tab-pixel-audit PYTHON` with a Python executable that has Pillow installed. The command exits nonzero for any unequal RGBA pixel or uncovered timeline step. Optional named regions can be supplied through the Python command's `--regions` JSON argument; they cannot replace the full-frame comparison.

Serve the output and its source comparison from the same local HTTP root to use the canvas pixel inspector. The viewer has previous/next controls, a frame slider, native/Cranpose blink, absolute differences and integer pixel zoom. It reports distinct source frames and their cadence so a dense paired timeline cannot be mistaken for additional captured display frames. Lossy recordings and unequal timestamps remain explicit limitations.

The selected bubble gains backdrop blur after release. The native layer capture shows a separate Gaussian backdrop layer with radius 2 points, opacity zero while held and one after release. A lossless step-image probe measures its standard deviation at 5.94–6.02 physical pixels on the 3× display. Cranpose's blur radius is twice its Gaussian standard deviation, so this material uses radius 4 points. The native layer follows the outer displacement and precedes the inner displacement and foreground. `Glass::backdrop_blur` mixes this fixed-radius backdrop into the edge-lens chain; the content mask remains sharp. Runtime shader chains resolve each stage’s declared blurred, block-average and mean substrates from that stage’s input. The native and Cranpose kernels retain different truncation and sampling behavior; the unit conversion alone does not establish pixel parity.

The exact pixel audit uses integer RGBA histograms for all channels, including alpha. It retains the same one-channel, one-pixel rejection rule and forbids fractional crop coordinates. The generated reports retain every source frame at its original timestamp; the recorded video cadence still limits visible microframes.

`REFERENCE_POINTER_PATHS` in the XCTest runner environment accepts one recorded event array per content case, using the native timeline's `planned_events` entries. This replays the same coordinates and offsets when the native layout changes with the content. The runner validates event order, finite coordinates, timestamps and destination hit targets. The comparison still rejects unequal paths; it does not register images or conceal layout differences.

## Release validation

`just test-liquid-vibrancy` runs the required composition and material regressions. Nine native matching targets remain unmet and are retained as ignored tests with their original assertions and fixtures. Run `just audit-liquid-native-parity` to execute all nine explicitly; this audit currently fails and does not gate the approved appearance release. Their failures cover motion shape, pane tone/color/coverage, lens joins/lighting/spectrum, and the dark settled selection. This separation does not establish native pixel parity. The recorded frame and exact RGBA audits retain all differences.
