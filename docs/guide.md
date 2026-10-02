# Cranpose guide

This is the guide bundled into the demo's Documentation tab. Its chapters work
offline; links open the full reference in your browser. The examples and source
references were checked against the 0.9 preparation tree on 2026-10-02.

## Welcome

**Build native and browser interfaces in Rust.**

Cranpose is a declarative UI framework inspired by Jetpack Compose.
A composable describes its current UI from state. The runtime remembers values,
tracks reads, recomposes affected content, measures modifier chains, and renders
the result through wgpu.

The 0.9 release line is preparation for 1.0. Public APIs can still change before
1.0; the [release readiness ledger](release_readiness.md) records the evidence
and remaining acceptance work. A version number does not establish platform
quality or complete Compose parity.

### Explore this application

Documentation is the first desktop tab. Every existing demonstration is still
available in the tab strip, or through the Tabs picker on a narrow window.
Use Counter App for state, CompositionLocal for scoped values, Async Runtime for
effects, Layout and Lazy List for measurement and scrolling, Text Input and
Text for editing and typography, and Liquid UI for the glass component library.

Other examples include animations, images, shaders, floating windows, platform
file picking, rotary input, Wear UI, Hacker News, Markdown, and Winamp.
Show source opens the selected example's implementation from its build revision.
Network examples need network access; this guide does not.

### The framework's layers

- **cranpose** is the application facade: launcher, platform integration and prelude.
- **cranpose-core** owns composition, remembered state, snapshots and effects.
- **cranpose-ui**, **cranpose-foundation**, **cranpose-ui-layout** and
  **cranpose-ui-graphics** provide widgets, layout, gestures, text and drawing.
- **cranpose-animation** provides transitions, springs and tweens.
- **cranpose-liquid** is the glass component library.
- **cranpose-services** exposes platform capabilities.
- **coroflow**, **cranpose-coroflow** and **cranpose-navigation** provide flows,
  composition integration, view models and navigation.
- **cranpose-testing** provides composition and real-window robot tests.

Continue with Get started, then State and effects.

## Get started

### Create an application

[Showcase Cranpose](https://github.com/samoylenkodmitry/cranpose-showcase) provides
desktop, Android, iOS and web shells. Create a repository from that template,
or add Cranpose to an existing Rust application:

```toml
[dependencies]
cranpose = { version = "0.9", features = ["desktop", "renderer-wgpu"] }
```

The 0.9 dependency becomes available when the v0.9.0 publishing workflow finishes.
Before that release, use the currently published version shown on
[crates.io](https://crates.io/crates/cranpose).

### A complete counter

```rust
use cranpose::prelude::*;

#[composable]
fn Counter() {
    let count = rememberMutableStateOf(|| 0_i32);
    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default(),
        move || {
            Text(
                format!("Count: {}", count.value()),
                Modifier::empty(),
                TextStyle::default(),
            );
            Button(
                Modifier::empty().padding(12.0),
                ButtonSpec::default(),
                move || count.update(|value| *value += 1),
                || {
                    Text("Increment", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    AppLauncher::new()
        .with_title("My Cranpose app")
        .with_size(640, 480)
        .try_run(Counter)?;
    Ok(())
}
```

Run a desktop application with `cargo run`. For this repository's full demo,
use `cargo run -p desktop-app --features desktop --release`.
Pass `docs` as the application's first argument to open the documentation
explicitly. Existing startup aliases continue to select their examples.

### Build configuration belongs to your application

Set the release profile in your application's workspace. Dependency profiles
do not configure the final binary:

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
strip = true
panic = "abort"
```

Choose features deliberately. `desktop` enables the desktop shell;
`desktop-x11` and `desktop-wayland` can select a display backend.
`renderer-wgpu-gles` adds the optional desktop GL fallback.
See the [README](../README.md) for platform build commands and font options.

## State and effects

### Composition is a description

A `#[composable]` function can run again when state it reads changes.
Keep file I/O, network requests and resource acquisition out of the function
body. Put ongoing work into effects with an explicit lifetime.

`rememberMutableStateOf` creates observable state at the call's composition
position. Read it with `.value()` or `.get()`, replace it with `.set(value)`,
or edit it through `.update(|value| ...)`. Capture the state handle in
callbacks so they read the current value.

```rust
let count = rememberMutableStateOf(|| 0_i32);
let label = format!("Count: {}", count.value());
count.update(|value| *value += 1);
```

Perform the update from an event callback or effect, rather than on every
composition pass.

### Remembering ordinary values

`remember(|| value)` returns `Owned<T>`. Its `.with(|value| ...)` method
borrows the remembered value for the closure. This Rust ownership model differs
from Kotlin's direct return of T.

Use `rememberKeyed(key, |key| ...)` when the value depends on changing inputs.
The initializer receives the key, and the remembered result is cloned when read;
use an inexpensive shared handle for a large immutable result.

Keep stable keys for dynamic content. An item's identity should follow the
item when it moves; its position is not a durable identity.

### Effects and asynchronous work

`LaunchedEffect(key, |scope| ...)` owns work for the keyed composition lifetime.
The scope supports background work and delivery back to the UI. Respect
cancellation, and report errors as UI state.

`DisposableEffect` registers cleanup for resources acquired by an effect.
Use `rememberCoroutineScope` for work started by a user action.
The Async Runtime tab demonstrates these patterns.

Composition locals provide values to a subtree. Use them for scoped services,
theme and other environment values. Avoid using a local as an unstructured
global store.

For the precise contracts, read the
[core API](https://docs.rs/cranpose-core/latest/cranpose_core/) and
[slot-table invariants](slot_table_invariants.md).

## Layout and input

### Modifiers describe ordered operations

Use `Row`, `Column`, `Box` and `Spacer` to arrange content.
Each container accepts a modifier, a layout specification and a content closure.
Specify arrangement, alignment and spacing through its spec.

Modifier order affects layout, painting and hit testing. Padding before a
background does not describe the same bounds as padding after it.
Use the Modifier Showcase tab and [modifier reference](MODIFIERS.md) to inspect
the result.

Use `fill_max_width`, `fill_max_size`, `weight` and constraint-aware layouts
instead of assuming a fixed screen size. Keep a finite viewport around scrollable
content. A lazy list nested in an unbounded vertical scroll is usually the wrong
layout.

### Long lists

`LazyColumn` composes the items needed for the viewport. Hold its state with
`rememberLazyListState` and supply a `LazyColumnSpec`.
Use stable item keys and share immutable backing data when ownership crosses
into retained item closures. Avoid recreating an entire large dataset each frame.

See the Lazy List tab and [lazy-list guide](lazy_list_doc.md).

### Pointer and keyboard input

`Modifier::clickable` receives a pointer position. Its Rust callback signature
is deliberately different from Compose's no-argument click callback.
Use a Button for ordinary button behavior, and lower-level pointer input for
custom gestures.

Provide names, roles and actions for custom controls. Pointer-only behavior is
insufficient for keyboard and screen-reader activation. Focusable controls
participate in Tab navigation; focus reveal scrolls the focused item into view.

The [accessibility guide](accessibility.md) covers focus, semantic actions,
canvas children, ranges and selection groups.

## Text and accessibility

### Plain and styled text

`TextStyle` contains `SpanStyle` and `ParagraphStyle`. Use the span style for
font, color, brush and decoration; use the paragraph style for alignment,
direction and line behavior.

`AnnotatedString` supports styled spans, paragraph runs, string annotations
and links. `LinkedText` dispatches URL links through a supplied callback and
supports custom clickable annotations. Rich text is implemented; the
[text status document](text.md) records source and test evidence.

Fonts must cover the scripts your application displays. Explicitly register
application fonts where reproducible appearance matters. Font fallback, shaping,
line breaking and platform input behavior need testing with your actual content.

### Editing

Use `TextFieldState` with `BasicTextField` or
`BasicTextFieldWithOptions`. Exercise composition through an IME, selections,
clipboard operations, multiline editing and password protection on the target
platform. A desktop keyboard test does not establish mobile keyboard behavior.

The Text Input tab is an interactive starting point.
[Text input architecture](TEXT_INPUT_ARCHITECTURE.md) explains the platform bridge.

### Accessibility is observable behavior

Give each screen a `pane_title`. Prefer standard widgets, which supply useful
semantics. Add `content_description` when visible text is insufficient.
Mark decorative images as decorative. Keep interactive targets large enough,
and expose selected, toggled, disabled and adjustable states.

Test both keyboard navigation and native reader actions. The framework has
desktop AccessKit, Android, iOS and browser bridges; automated semantic-tree
checks establish only the interactions they execute.

The [validation guide](accessibility_validation.md) separates build checks,
native robot results and reader usability evidence. Existing dated reports
remain dated reports; 0.9 is not an accessibility conformance certificate.

## Animation and graphics

### Animate state and drawing

`cranpose-animation` provides `animateFloatAsState`, springs, tweens,
transitions and infinite transitions. A finite transition should settle.
Use frame-driven state deliberately, and avoid waking an otherwise idle screen.

Use `Canvas`, drawing modifiers and `graphics_layer` for custom rendering.
Keep stable geometry and material state reusable. Changing a value only needed
during drawing should not rebuild an unrelated screen.

### Liquid components

`cranpose-liquid` provides glass materials and interactive components.
The Liquid UI, Glass Feed and Glass Tiles tabs demonstrate the library.
Rendering cost depends on screen area, blur, refraction, overlapping surfaces
and the device GPU.

The framework includes render regression tests and native-reference fixtures.
Those establish named visual and motion contracts, not universal equivalence
with every Apple control.

### Measure the visible result

Use release builds, named physical devices and the actual presentation path.
Track frame-time tails, memory and power as well as average FPS.
Preserve accepted pixels when optimizing.

Read the [performance guide](performance_coding_guide.md) and
[render verification guide](render_verification.md) for the project protocols.

## Navigation and services

### Navigation and view models

`cranpose-navigation` provides `NavHost`, a typed back stack and a view-model
store per screen. Model routes as application data and give each screen a
clear lifecycle.

`cranpose-coroflow` integrates view models and flows with composition.
Use `collectAsState` to expose a stream as UI state; scope collection and
side effects to the screen that owns them.

The [navigation reference](navigation_plan.md) and
[coroflow reference](coroflow_plan.md) describe the available operations.

### Platform capabilities

Services include HTTP, URI opening, file selection, clipboard, sharing,
notifications, haptics, camera, media, power and purchases.
Enable the matching facade features and use the service's capability or
support result. Unsupported and temporarily unavailable are different states.

The [capability matrix](capability_parity.md) documents platform differences.
Examples include local-file-only media on desktop and iOS, no iOS network
monitor yet, and camera availability that depends on the platform backend.

Treat cancellation of a picker as cancellation. Surface a failed operation
to the user. Do not grant a purchase entitlement because a platform has no
store backend.

## Platforms and tooling

### Desktop

Linux uses Vulkan, macOS uses Metal, and Windows uses DX12 or Vulkan through
wgpu. Linux has the continuous GPU robot suite; macOS has build and test jobs.
Windows is cross-checked and distributed, but continuous Windows runtime
coverage remains a release-readiness item.

### Android and iOS

The repository's Android recipes build a release APK. Device tests separately
cover runtime behavior. Wear OS uses the Android integration and adds rotary
and watch-oriented UI.

iOS uses the UIKit integration through winit-uikit. Simulator and device builds
are distinct from physical-device interaction and accessibility checks.
See the [iOS build guide](../apps/ios-demo/README.md).

### Web

The demo defaults to WebGL2. Request WebGPU with `?backend=webgpu` where
supported. Use `just web` for the repository's release browser build.

Browser capabilities depend on secure contexts, permissions and browser support.
Test packaged applications, text input, focus and accessibility in the browsers
you intend to support.

### Cranpose Studio

[Cranpose Studio](https://plugins.jetbrains.com/plugin/34594-cranpose) provides
JetBrains IDE integration with previews, inspection and platform build tooling.
See the [IDE guide](intellij.md). Embedded applications are a separately
documented experimental feature.

## Testing and performance

### Test what a user can observe

Composition tests exercise public widget behavior. Robot tests drive a window,
find controls, send input and capture frames. Native accessibility robots inspect
the tree exposed to the operating system and invoke its actions.

Add a regression that fails for the original behavior before fixing a defect.
Verify layout, text, focus, callbacks and pixels rather than private storage
details. Put test bodies under test directories.

### Repository checks

```bash
just ci
just robot
just web
just android
```

`just ci` covers formatting, package alignment, tests, lint, rustdoc and
architecture budgets, plus the repository's contract checks.
Platform builds and the robot suite add evidence beyond those checks.
Use the recipes in the [justfile](../justfile); they define the shipped features.

### Performance evidence

Use a representative consuming application and its release profile.
Separate CPU time, GPU work, allocations, process memory and actual presentation.
Record source revision, device, settings, sample duration and artifact hashes.

The [performance tracker](https://github.com/samoylenkodmitry/Cranpose/issues/902)
contains dated measurements and remaining work. A site-specific allocation
saving does not by itself establish a whole-application FPS or memory improvement.

## Road to 1.0

**0.9 is the stabilization release line.**

Before 1.0, define the supported public surface and finish intended breaking
changes. Then require compatibility for the supported 1.x API.
Document supported platform versions, features and the minimum Rust version.

Each supported platform needs runtime evidence for startup, layout, input,
focus, scrolling, lifecycle and recovery. Accessibility needs native reader
testing. Performance needs explicit budgets on reference devices.

Validate candidate packages in real applications outside this workspace.
Keep installation, examples, packaging and upgrade guidance current.
The published isolated demo already serves as an external-consumer check.

The [release readiness ledger](release_readiness.md) records what is confirmed,
what is historical and what still needs validation. Optional service parity and
every Compose API are separate scope decisions.

Release from green main using the [release runbook](release.md).
The tag workflow updates package versions, publishes crates in dependency order
and then updates and builds the isolated consumer.
