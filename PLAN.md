# Cranpose Framework Ownership Plan

This plan covers Cranpose and its CranScan, CranOrbit, and CranAmp consumers. A
slice is complete only when the framework API, every affected consumer, tests,
and supported platform builds agree. Consumer applications must not subclass a
Cranpose activity, call JNI for framework services, compile framework Java
sources, poll platform result files, or recreate platform contracts.

## Review inventory

### CranScan

- `app/src/native_entry.rs`, the removed `app/src/android_bridge.rs`, and the
  removed Java activity, camera, billing, and background-service classes were
  host implementation. Activity setup, density, fonts, lifecycle, camera,
  billing, incoming shares, update installation, and background work belong to
  Cranpose.
- `app/src/app.rs` installed a second popup host and manually initiated bundled
  model copying. The shell now owns the popup layer and Cranpose now owns the
  versioned asset-set installation effect.
- `app/src/services.rs` dispatched its own workers, pumped camera frames,
  branched save/share per target, probed power, and adapted polling. Worker
  dispatch is now `launchBlocking`, the save path is one `SaveDocumentRequest`,
  power reads report capability, and the entitlement is derived from observable
  store state with purchase outcomes collected as an event stream.
- `app/src/ui/widgets.rs`, `library.rs`, and `document.rs` built dropdown data
  vectors and translated selected indices. They now use scoped composable menu
  content and item-owned actions.
- The hint field used string-only text-field decoration and now uses a
  composable decoration slot. System insets are now read through Cranpose
  modifiers and locals instead of being carried through the app context.
- Canvas use in charts, receipt editing, and scan graphics is retained because
  those surfaces are custom graphics rather than standard controls.

### CranOrbit

- The removed activity, `host.rs`, `lifecycle.rs`, `signals.rs`, and `wake.rs`
  recreated host, lifecycle, and scheduling contracts. Cranpose now owns the
  activity and typed application directories, but observable frame and durable
  lifecycle work still need the contracts below.
- `app_state.rs` and UI modules used shared mutable containers, revision
  fingerprints, copied derived values, and explicit frame wake requests. The
  fingerprints are gone in favour of comparable screen semantics, and frames are
  requested by observable state driving a frame-clock effect. The simulation
  arena keeps its interior mutability: a sixty-times-a-second physics step is
  not what observable state is for.
- Store event draining, blocking save-on-lifecycle code, and rotary slider
  input are framework contracts now: an event stream, a lifecycle-aware durable
  save, and the Cranpose slider.
- Arena/game Canvas drawing, simulation, cue mapping, haptic patterns, and the
  save schema remain product code.

### CranAmp

- The removed activity and `android_bridge.rs` implemented update installation,
  picker result transport, and Android callbacks. Cranpose now owns those
  Android contracts and exposes typed update state.
- `audio.rs` carried a rodio/symphonia backend, an `HTMLAudioElement` backend,
  and a hand-built browser file input. All three are gone: playback, seeking,
  audio focus, the media session, the equalizer and the analysis tap are
  `cranpose_services::media`, and picking is the same launcher on every target.
  Track discovery, titles and the visualiser's band curve stay product code.
- `winamp/mod.rs` wrote picker-resume marker files, polled results, wrote
  browser `localStorage` directly, and reached the host size through JavaScript
  globals. Picks are keyed launchers whose grants survive process death, saved
  state is framework preferences, and the surface size is the host-surface API.
  Modal layering and window attachment stay product code.
- Sprite rectangles, pressed visuals, slider input, equalizer controls, and the
  playlist scrollbar were app-built controls. Sprite rendering now uses Image
  composables with a bitmap-region painter; pressed state uses interaction
  sources; slider and scrollbar input use the Cranpose Slider.
- The visualizer Canvas is retained because it renders application-specific
  spectrum graphics. Skin parsing, sprite coordinates, playlist behavior,
  equalizer behavior, sync policy, and visual appearance remain product code.

### Framework APIs that exposed implementation details

Each of these made an application understand how the host works. All are
replaced. Most of the names below no longer exist anywhere in the four
repositories; `PowerMonitor` and `DeviceInfo` remain as the platform traits a
backend implements, reshaped so that what they report includes whether they can
report it at all.

- `FilePicker::take_resumed_picks`, `FolderStream::take_ready` and
  `FolderStream::is_finished` made applications poll and understand host
  recreation. Keyed launchers deliver a grant to whoever asked for it, across
  activity recreation and process death alike, and folder walks are async
  streams that wake their consumer.
- `StoreEffect` and `IncomingShareEffect` announced news and left the draining
  to the application. Store state is observable and purchases, incoming content
  and media commands are event streams.
- `FrameSignal` exposed scheduler wake mechanics. Frames are requested by
  observable state driving a frame-clock effect.
- `PowerMonitor` and `DeviceInfo` were snapshot queries. Both report capability
  and are observable, so a desktop with no battery reads as *no battery* rather
  than as *empty*.
- `BundledAssets::read` buffered a whole asset; large sets install declaratively
  and read as streams.
- `Icon`, `IconButton`, `Scaffold`, popup positioning, scaling lists and swipe
  state took Compose-shaped slots, modifiers, identity, semantics and observable
  state.

## 1. Host and packaging

- [x] Publish Cranpose Android code and manifest contributions as an AAR.
- [x] Add a Cranpose Gradle plugin that configures native builds, ABIs, profiles,
      JNI library packaging, optional service modules, and manifest metadata.
- [x] Generate the Android native entry from a declarative application spec.
- [x] Remove direct Cranpose Java source directories and repeated native build
      tasks from CranScan, CranOrbit, and CranAmp.
- [x] Remove every consumer Activity subclass and consumer JNI bridge.
- [x] Expose host density, viewport, system-font resolution, and window controls
      without target-specific application code.

## 2. Content, storage, and persistence

- [x] Introduce a streaming `ContentHandle` shared by file picking, incoming
      shares, document intents, dropped files, and media sources.
- [x] Replace polling folder enumeration with an async stream that wakes its
      collector.
- [x] Add composition-owned open-file, open-files, open-folder, save-document,
      and writable-folder launchers with framework-owned result redelivery.
- [x] Remove resumed-pick inbox draining and application marker files.
- [x] Use the same picker contracts on Android, iOS, desktop, and web.
- [x] Add typed data, config, cache, documents, temporary, and shared directory
      access through the host.
- [x] Add preferences plus saveable-state and saver support.
- [x] Add display metadata and streaming operations to writable folders.

## 3. Observable services and structured work

- [x] Expose lifecycle state through a composition local and observable state.
- [x] Expose store state as observable state and purchases as an event stream.
- [x] Expose incoming content as a stream instead of observer-plus-drain calls.
- [x] Add async-stream collection, producer state, delay, interval, and blocking
      work helpers scoped to composition.
- [x] Replace `FrameSignal` coupling with frame-clock effects driven by observable
      state and explicit active state.
- [x] Add lifecycle-aware durable-save completion with host deadline semantics.
- [x] Replace global background-work booleans with reference-counted leases and
      a composable effect.

## 4. Compose-shaped UI primitives

- [x] Remove `InteractiveCanvas` and `CanvasControl`; controls are composables.
- [x] Keep `BasicTextField` primitive and add composable decoration slots through
      a higher-level text field API.
- [x] Add layout-coordinate-driven popup positioning and make the popup host
      internal to the application shell.
- [x] Replace vector/index input with scoped composable content across the menu,
      dropdown, tab bar, segmented control, and icon button group.
- [x] Add a modal dialog primitive with focus, dismissal, semantics, and insets.
- [x] Add interaction sources plus horizontal and vertical sliders with
      composable visual content, pressed state, completion, and rotary input.
- [x] Add general draggable state and explicit scrollbar components.
- [x] Add sprite-region, filtering, tiling, and nine-patch painter support.
- [x] Complete Scaffold slots, colors, RTL-aware padding, and content insets.
- [x] Add safe-area, IME, and generic window-inset modifiers.
- [x] Complete Icon and IconButton modifiers, semantics, enabled state, colors,
      interaction state, and touch sizing.
- [x] Move swipe identity to lazy-item keys and expose swipe state/progress.
- [x] Add keys, content types, observable layout state, and programmatic scrolling
      to Wear scaling lists.

## 5. Platform services

- [x] Replace camera polling and preview JPEG transport with an observable camera
      controller, native/GPU preview, bounded analysis stream, and async capture.
- [x] Add a cross-platform media player with observable playback, seeking, audio
      focus, lifecycle handling, media sessions, and optional analysis samples.
- [x] Make power and device information capability-aware and explicit about
      unsupported or unknown values. Power is observable as well —
      `publish_power_state` / `observe_power_state` / `rememberPowerState`,
      because thermal pressure and battery level are things the platform
      *tells* an application about. Device information is deliberately a
      snapshot: `device_info()` answers what the process is using right now,
      and there is no platform event to publish from, so an observable mirror
      would be a poller with a cadence the framework invented. An application
      that wants a live reading samples it on its own frame.
- [x] Make HTTP operations genuinely async with streaming, progress, cancellation,
      and resumable downloads.
- [x] Add declarative versioned installation for bundled asset sets with
      per-file atomic replacement and a final commit stamp.
- [x] Add streaming reads for bundled assets that do not fit in memory.
- [x] Add a typed application update service with progress, verification, and
      platform installer confirmation. Android discovers a release and installs
      it. Desktop and iOS discover one and cannot install it — App Store Review
      Guideline 3.3.2 forbids an iOS application from replacing its own binary,
      and the framework owns no desktop installer — so `AppUpdateCapabilities`
      splits `check` from `install` and those two hosts answer `check: true,
      install: false` rather than registering an installer that can only fail.
      The check itself needs an HTTP client, which is the application's own
      opt-in (`cranpose-services/http-native`); without it the backend reports
      `check: false` rather than claiming a request it cannot make. The feed
      reader is covered by a test for the one thing a version comparison gets
      wrong on its own — `0.1.10` is newer than `0.1.9`, which sorting strings
      says it is not — and its field names were checked against the live
      release feed rather than against a fixture written from the same reading
      of the documentation that produced the parser. Forwarding
      that feature through the `cranpose` umbrella the way every other service
      feature is forwarded would need one feature per target, which Cargo cannot
      express — so the opt-in stays where every application already writes it.
- [x] Add an observable web host surface size and resize request API.

## 6. Consumer conversions

### CranScan

- [x] Remove native entry setup, application directory logic, manual popup host,
      manually threaded system insets, callback event draining, duplicate IO
      dispatch, camera pumping, platform memory probes, share/save branches,
      folder-handle parsing, and bundled-asset installation mechanics.
- [x] Retain OCR, segmentation, document processing, data schema, model policy,
      scan analysis, charts, edit graphics, and product power policy.

### CranOrbit

- [x] Remove native entry density/font setup and application directory probing.
- [x] Replace revision hashes, copied derived states, and manual frame wake
      calls with observable state and frame-clock collection. The arena keeps
      its `Rc<RefCell<AppState>>`: a simulation that mutates every field on
      every one of sixty frames a second is not what observable state is for,
      and it publishes what the interface reads at the frame boundary instead.
- [x] Replace manual store draining, blocking lifecycle persistence, and rotary
      slider input with framework contracts.
- [x] Retain simulation, game transitions, arena drawing, cue mapping, haptic
      patterns, save format, and product Wear styling.

### CranAmp

- [x] Remove `CranampActivity`, `android_bridge`, JNI, platform result files,
      direct browser picker code, direct native dialog use, manual platform
      directories, direct browser storage, and browser host callbacks.
- [x] Replace custom media backends, playback timers, pressed-state handlers,
      sliders, scrollbars, modal layers, and Canvas sprites with Cranpose APIs.
- [x] Retain skin parsing, sprite coordinates, playlist rules, sync conflict
      policy, equalizer behavior, visualizer appearance, and window attachment.

## 7. Verification and review

- [~] Unit-test every public framework function and method, and delete the ones
      no caller wants. This is **not** achieved, and the number that said it was
      came from a broken measure.

      `python3 scripts/public_api_test_coverage.py` reported 2628/2689 (97.7%)
      with 61 functions untested. It built its test corpus by taking each file
      from its first `#[cfg(test)]` to the end. That attribute also introduces
      test-only imports and conditional debug lines, and 138 files here carry
      one near the top — `render.rs` at 0.1% of the file, handing 971KB of
      production source to the "test corpus". Every public function below such
      a marker certified itself as tested by its own source position. The
      corpus is now the braced body of each `#[cfg(test)] mod`, and the honest
      figure is **3031/3396 (89.3%), with 365 public functions untested**. The
      denominator moved because 700 of those functions were being read as test
      code rather than counted as API at all.

      That was the third defect in this one script: it also matched names as
      substrings, so `with_timeout` read as covered because `exit_with_timeout`
      exists, and it excluded the robot suite, which is the only exercise much
      of the robot driver API gets. A proxy metric that is quietly wrong sends
      work to the wrong places for as long as nobody reads it.

      The 365 are not being closed by writing 365 tests. A test written to
      raise a number tests the implementation it was written against. The tests
      this pass did add were written the other way round: each pins a defect
      found first — a seek that played from up to a packet early, a dialog that
      did not trap focus — or covers a new platform module's own decisions,
      which is where a path that escapes its asset root or a version that sorts
      `0.1.10` before `0.1.9` would otherwise ship unnoticed. The gap is
      recorded here as a gap.

      Sixty-one public functions had no caller anywhere and were deleted, two
      of them placeholders that answered `true` regardless of the screen, along
      with a second never-instantiated focus layer whose `request_focus`
      returned `true` without doing anything. That deletion rule does not
      extend to the Compose-shaped surface: `rememberSaveable`,
      `ProvideLifecycle`, `rememberLifecycleState`, `DurableSaveEffect` and
      `interval` have no consumer calling them today and are kept, because what
      makes them API is that an application written against Compose expects
      them to exist, not that one of ours has reached for them yet.

- [x] Add integration coverage for host recreation, picker redelivery, lifecycle,
      observable services, controls, accessibility, and platform capability
      state. The widget libraries are composed and measured rather than only
      called: `cranpose-liquid` and `cranpose-ui` each have a suite that runs
      widgets through a real composition, which is what catches a component
      that reads a local nobody provided. Not every exported widget is in those
      suites — `LazyRow` has no composition test anywhere, and several others
      are covered instead by the robot suite or by tests next to their own
      module. "Every widget they export" was too strong.
- [x] Run `cargo fmt`, full workspace tests, and clippy with zero warnings.
      This was recorded as done while clippy reported 28 warnings. The tree had
      been linted under an older toolchain; 1.98 added
      `chunks_exact_to_as_chunks`, and 27 sites across the framework, the demo
      and the robot runners tripped it, plus `unnecessary_cast` and
      `manual_slice_fill`. All are fixed. The three consumer applications were
      carrying warnings of their own — CranScan 371, mostly `.clone()` on
      framework types that had become `Copy`, and CranOrbit 27 — and both are
      now at zero, with the `too_many_arguments` cases resolved by giving the
      repeated parameter groups a type rather than by silencing the lint.
- [x] Build the web targets, and the desktop and iOS targets of every consumer.
      The web build runs on a second machine, which is also where the wasm
      toolchain lives.
- [x] Build the Android release applications: the `cranpose-android` AAR and the
      service modules, the Gradle plugin, and a release APK that links the
      framework's native library and its Java bridge. The demo's release APK
      builds from a clean tree — 155 tasks, all executed, none from cache.
- [x] Run sequential headless robot tests and relevant performance scripts. The
      suite discovers 155 robot examples and is run on two machines. On the
      second host all 122 that ran passed, with 33 skipped; here 119 of 121
      passed, with 34 skipped.

      Both hosts skip the same 33, which need an X11 session with `xdotool` and
      are therefore only exercised on Linux. The 34th is
      `robot_leetcodedaily_code_scroll_pixel_drift`, which needs Python Pillow
      for its pixel comparison; the second host has Pillow, so that comparison
      is covered there.

      The two that failed here are `robot_shader_rect` and
      `robot_shader_backdrop_drag`. Both call `.with_headless(false)` because
      they verify real GPU presentation, and this host's display was not
      compositing, so the window never presented a frame. The suite's
      capability skips gate on X11 plus `xdotool`, which these two do not need,
      so they run wherever the suite runs and fail where nothing can be
      presented.

      That reading is the host's own record, not an inference. `pmset -g log`
      on this machine reports the panel turned off at 02:59 and back on at
      05:05, and the two failures are timestamped 04:38 — inside that window,
      with the machine cycling through DarkWake. The same host had passed all
      121, both of these included, the previous evening with the panel on.

      Running the same two examples from a worktree at the base revision and
      from this branch back to back on the one machine, with the panel on,
      separates the display from the code: base and branch both pass both
      examples here, and the base revision also passes on the second host.
      Nothing in the presentation path regressed. What did change is that this
      branch bounds the robot present wait; at the base revision the same
      sleeping display parks the driver until the harness's own 180s deadline
      and reports nothing about why.
- [x] Review all four repositories for target-specific framework work, duplicate
      implementations, unused APIs, unfinished states, and untracked artifacts.

### What the review found

Three defects and one unusable API, each found by running a verification rather
than by reading the code that claimed to have passed one.

- **A text field could not un-extend a selection.** `extend_selection_left`
  moved the range's `start` and `extend_selection_right` moved its `end`, so the
  two grew from opposite ends and shift-left followed by shift-right left a
  character selected instead of collapsing. `start` is now the anchor and `end`
  is the cursor, which makes the pair inverses; a reverse range is what carries
  an anchor to the right of the cursor.
- **Two robot finder arms were placeholders.** `exists()` on a clickable query
  answered `true` whatever was on screen, and `bounds()` on a position query
  answered `None` whatever was under the point. The clickable query had no
  producer and is gone; the position query now answers with the innermost rect
  covering the point.
- **`ModifierLocalKey` was not exported.** `Modifier::modifier_local_provider`
  is public and takes one, so no application outside this repository could call
  it. It is exported now.
- **An application decided which platforms grant background execution.**
  CranScan gated its work lease behind `#[cfg(any(target_os = "android",
  target_os = "ios"))]`, which is the framework's question: a lease with no
  platform backend behind it already costs nothing.

All three applications now declare their application id, so a desktop build
scopes its storage by the identifier it ships under rather than by whatever the
executable happens to be named.
