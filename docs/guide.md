# Build apps with Cranpose

## Welcome

**Build apps with Compose in Rust.**

Cranpose brings the Jetpack Compose model to Rust. Describe a screen with
composable functions, keep changing values in state, and handle actions in
callbacks. Cranpose updates the affected UI and handles the platform window,
rendering, input and device services.

If you know Compose, you can use the same ideas: state flows down, events flow
up, modifiers describe layout and appearance, and effects own work with a
lifetime. You write Rust functions and closures in place of Kotlin functions
and lambdas. You can share those functions across desktop, Android, iOS and web.

If you are new to Compose, start with the counter in **Get started**. It shows
the whole cycle: read a value, draw it, change it on a click. The following
chapters build on that pattern.

### Bring your Compose knowledge

- **Composable functions:** replace `@Composable fun Screen()` with
  `#[composable] fn Screen()`.
- **State:** replace `remember { mutableStateOf(0) }` with
  `rememberMutableStateOf(|| 0)`. Read with `count.get()`; write with `count.set(value)`.
- **Layout:** replace `Column { ... }` with
  `Column(modifier, spec, move || { ... })`.
- **Modifiers:** replace `Modifier.padding(16.dp)` with
  `Modifier::empty().padding(16.0)`.
- **Lazy lists:** use `LazyColumn(modifier, state, spec, move |scope| { ... })`.
- **Effects:** use `LaunchedEffect` or `LaunchedEffectAsync` for tasks and
  `DisposableEffect` with `DisposableEffectResult` for cleanup.
- **Scoped values:** use `CompositionLocalProvider`.
- **Navigation:** use `NavHost` and `rememberNavController`.
- **Flows:** use `StateFlowCollect::collectAsStateWithLifecycle()`.

These are corresponding concepts, not a promise that Kotlin signatures translate
word for word. Rust uses explicit types and ownership. Container options live in
`ColumnSpec`, `RowSpec` and other spec values. The examples show those differences.

### Choose a starting point

Read **Get started** to make a new app. Use **Compose integration** to add a Rust
screen to an existing Android Compose app. Use **Native views** to place a
platform control inside a Cranpose screen.

The guide is available offline in this tab. Source and API links open in a
browser. The other demo tabs let you try the controls as you read.

## Get started

### Create an application

Install Rust, then make a desktop project:

```sh
cargo new my-app
cd my-app
cargo add cranpose --features desktop,renderer-wgpu
cargo add anyhow
```

Cargo writes the dependency versions into your manifest. For a project with
Android, iOS and web entry points already wired up, use the
[showcase template](https://github.com/samoylenkodmitry/cranpose-showcase).

Replace `src/main.rs` with this program:

```rust
use cranpose::prelude::*;

#[composable]
fn Counter() {
    let count = rememberMutableStateOf(|| 0_i32);
    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            Text(
                format!("Count: {}", count.get()),
                Modifier::empty(),
                TextStyle::default(),
            );
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || count.update(|value| *value += 1),
                || {
                    Text("Increment", Modifier::empty(), TextStyle::default());
                },
            );
        },
    );
}

fn main() -> anyhow::Result<()> {
    AppLauncher::new()
        .with_title("My app")
        .with_size(640, 480)
        .try_run(Counter)?;
    Ok(())
}
```

Run `cargo run`. Each click changes `count`; the text updates to match.

`#[composable]` lets Cranpose track the function's state reads.
`rememberMutableStateOf` keeps the value between calls. The `move` closures
capture the state handle so the column content and the button can use it later.
A state handle is `Copy`: passing it into both closures does not copy the value
it stores.

Use `cargo run --release` when judging animation or scrolling. Put release
settings in your application's `Cargo.toml`; a dependency's profile does not
configure your binary:

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
```

### Keep one UI and give each platform an entry point

Move `Counter` into your library as a public composable when adding another
platform. Each entry point starts that same UI through the platform launcher.
The showcase has working [entry points and build settings](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/Cargo.toml)
for this arrangement.

Enable `renderer-wgpu` together with the feature for your target:

- **`desktop`:** call `AppLauncher::try_run` from `main`.
- **`android`:** build a native library and package it with the Android Gradle host.
- **`ios`:** start the UIKit host from the iOS entry point; package and sign through Xcode tools.
- **`web`:** build for `wasm32-unknown-unknown` and load it from the HTML shell.

Follow the template's [Android project](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/android),
[iOS scripts](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/ios)
and [web build](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/build-web.sh)
when packaging. Keep platform entry points small; put screens and application
logic in the shared library.

## State and effects

### Give state an owner

Keep state in the lowest common parent that needs to read or change it. Pass
values and callbacks to children. This is state hoisting, just as in Compose:

```rust
use cranpose::prelude::*;

#[composable]
fn Stepper(value: i32, on_increment: impl Fn() + 'static) {
    Button(
        Modifier::empty(),
        ButtonSpec::default(),
        on_increment,
        move || {
            Text(
                format!("Add one: {value}"),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}

#[composable]
fn StepperScreen() {
    let count = rememberMutableStateOf(|| 0);
    Stepper(count.get(), move || count.update(|value| *value += 1));
}
```

`Stepper` can now work with local state, a view model or data supplied by a native
host. It does not choose where the value lives.

Read a small `Copy` value with `.get()`. For a larger state value, use
`.with(|value| ...)` to borrow it rather than cloning a collection just to inspect
it. Change state in a callback or effect, not unconditionally while describing
the screen.

`remember(|| value)` keeps an ordinary value and returns an `Owned<T>` handle.
Use `.with(...)` to borrow it. Unlike observable state, mutating a remembered
ordinary value does not itself request recomposition. `rememberKeyed` recreates
a remembered result when its key changes. Use `key` for a group whose identity
must follow an item when it moves.

Remembered state lasts while its composition position exists. Save a document
in your data layer or preferences when it must survive closing the app.

### Load data when an input changes

Use `LaunchedEffectAsync` for asynchronous work owned by a composable. This
example loads a text response whenever the URL changes:

```rust
use cranpose::prelude::*;

#[composable]
fn RemoteText(url: String) {
    let result = rememberMutableStateOf(|| String::from("Loading…"));
    let client = local_http_client().current();
    LaunchedEffectAsync(url.clone(), move |_| {
        Box::pin(async move {
            result.set(String::from("Loading…"));
            result.set(match client.get_text(&url).await {
                Ok(text) => text,
                Err(error) => format!("Could not load: {error}"),
            });
        })
    });
    Text(result.get(), Modifier::empty(), TextStyle::default());
}
```

Add `cranpose-services` with `http-native` on desktop, Android or iOS; use
`web-http` for the browser. Choose those features in target-specific dependency
sections when building both. Android also needs the manifest's `INTERNET`
permission; browser requests must satisfy the server's CORS policy.

The URL is both the effect's key and input, so this example owns a copy for each
role. The old task is cancelled when the key changes or the composable leaves.
The async body runs on the UI runtime: awaiting I/O is fine, but decoding a large
image there would block input.

Use `LaunchedEffect(key, |scope| ...)` with `scope.launch_background(work, on_ui)`
for work that belongs on a worker. Return the result to `on_ui`; keep UI state
handles out of the worker closure. For work started by a button,
`rememberCoroutineScope()` supplies a scope that is cancelled when its owner
leaves the composition.

### Release a resource when its screen closes

Acquire a resource in `DisposableEffect` and return its cleanup. A camera screen
can start and stop its session this way:

```rust
use cranpose::prelude::*;

#[composable]
fn CameraSession() {
    let error = rememberMutableStateOf(|| None::<String>);
    DisposableEffect((), move |_| {
        if let Err(failure) = start_camera() {
            error.set(Some(failure.to_string()));
        }
        DisposableEffectResult::new(stop_camera)
    });
    if let Some(message) = error.get() {
        Text(message, Modifier::empty(), TextStyle::default());
    }
}
```

Mount `CameraSession` only while the capture screen needs the camera. Its cleanup
runs when it leaves composition. Camera setup and frames are covered in
**Platform services**.

Use `rememberUpdatedState` when a long-lived effect needs the latest callback
without restarting. Use `SideEffect` to publish a value after composition
commits. Use composition locals for values that apply to a subtree, such as a
theme or service; ordinary screen inputs are easier to follow as parameters.

## Layout and lists

### Arrange controls

Use `Column` for a vertical stack, `Row` for a horizontal stack, and `Box` for
content that overlaps. A container takes a modifier, a spec, and a content
closure. `weight` divides the remaining space along its parent's main axis:

```rust
use cranpose::prelude::*;

#[composable]
fn AccountRow() {
    Row(
        Modifier::empty().fill_max_width().padding(16.0),
        RowSpec::default()
            .vertical_alignment(VerticalAlignment::CenterVertically)
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
        || {
            Text("Ada", Modifier::empty().weight(1.0), TextStyle::default());
            Text("Online", Modifier::empty(), TextStyle::default());
        },
    );
}
```

The name takes the remaining width; the status keeps the width it needs.
Dimensions such as `16.0` are logical points. The platform host applies display
density. Text sizes use `TextUnit::Sp` so text scaling can be applied separately.

### Put modifiers in the order you mean

Each modifier wraps the modifiers after it:

```rust
use cranpose::prelude::*;

fn card_modifiers() -> (Modifier, Modifier) {
    let blue = Color::from_rgb_u8(40, 90, 180);
    let outer_space = Modifier::empty().padding(16.0).background(blue);
    let inner_space = Modifier::empty().background(blue).padding(16.0);
    (outer_space, inner_space)
}
```

`outer_space` leaves space outside the blue area. `inner_space` paints the padded
area blue too. The same ordering rule affects clipping and clickable bounds.
Use `width_in` or `height_in` for limits, `fill_max_width` to fill a parent, and
`align` to position a child in a `Box`. See the [modifier reference](MODIFIERS.md)
for the complete set.

### Show a long collection

Use a lazy list when the collection can be larger than the screen:

```rust
use cranpose::prelude::*;

#[composable]
fn Messages() {
    let scroll = rememberLazyListState();
    LazyColumn(
        Modifier::empty().fill_max_size(),
        scroll,
        LazyColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        |scope| {
            scope.items(1_000, |index| {
                Text(
                    format!("Message {index}"),
                    Modifier::empty().fill_max_width().padding(12.0),
                    TextStyle::default(),
                );
            });
        },
    );
}
```

This fixed collection uses positions as identities. For rows that can be inserted,
removed or reordered, supply keys from your data:

```rust
use cranpose::prelude::*;

#[composable]
fn SavedArticles() {
    let scroll = rememberLazyListState();
    let articles = [(42_u64, "State"), (87_u64, "Layout")];
    LazyColumn(
        Modifier::empty().fill_max_size(),
        scroll,
        LazyColumnSpec::default(),
        move |scope| {
            for (id, title) in articles {
                scope.item_keyed(Some(id), None, move || {
                    Text(title, Modifier::empty(), TextStyle::default());
                });
            }
        },
    );
}
```

For large collections, `LazyItems::new(count).key(...)` defines keys without
registering every row individually. Its `content_type(...)` groups rows with
the same structure for reuse. Keep large data in its owner and pass a shared
handle into retained row closures. Give the list a bounded height; do not put a
vertical lazy list in a vertically unbounded scroll container.

### Adapt to the available space

Read a container's size with `Modifier::report_size_state` and choose a one-pane
or two-pane layout from that size. Keep screen state above this choice so a
resize does not reset it. Apply `local_safe_area_insets().current()` as padding
where controls meet system bars or cutouts.

The showcase's [RootShell and SplitShell](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/app.rs)
implement this pattern: one route on a phone, a list beside a detail pane on a
wide window. The [lazy-list guide](lazy_list_doc.md) covers scroll control and
item reuse in more detail.

## Text and input

### Style a label

`TextStyle` separates character styling (`SpanStyle`) from paragraph layout
(`ParagraphStyle`):

```rust
use cranpose::prelude::*;
use cranpose::text::{FontWeight, TextUnit};

#[composable]
fn PageTitle() {
    Text(
        "Your library",
        Modifier::empty().heading(),
        TextStyle {
            span_style: SpanStyle {
                font_size: TextUnit::Sp(24.0),
                font_weight: Some(FontWeight::BOLD),
                color: Some(Color::from_rgb_u8(30, 40, 55)),
                ..Default::default()
            },
            ..Default::default()
        },
    );
}
```

Use `AnnotatedString` for multiple styles within a paragraph. `LinkedText`
handles link annotations and passes activated links to your callback. Use
`AppFonts` when your app bundles a font; include fonts covering the scripts your
users will enter. The **Text** demo has examples of spans, links and paragraphs.

### Edit text

Remember a `TextFieldState` and give it to `BasicTextField`. It owns the editable
text, selection and IME composition:

```rust
use cranpose::prelude::*;

#[composable]
fn NameForm() {
    let name = remember(|| TextFieldState::new("Ada")).with(|state| *state);
    Column(
        Modifier::empty().padding(16.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            BasicTextField(
                name,
                Modifier::empty()
                    .fill_max_width()
                    .content_description("Name"),
                TextStyle::default(),
            );
            Text(
                format!("Hello, {}", name.text()),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}
```

Use `BasicTextFieldWithOptions` to configure keyboard and editing behavior, or
`BasicTextFieldDecorated` to supply a surrounding label and border. Test your
form with the target platform's keyboard, including composing text through an
IME and moving the selection.

### Make actions available beyond touch

Use `Button` for an ordinary action. It supplies button behavior and semantics.
`Modifier::clickable` is useful for a custom clickable surface; its callback
receives a pointer position, so an unused position is written `move |_| ...`.
Use `pointer_input` for gestures that need the pointer stream.

Give custom controls a useful `content_description`, mark headings with
`heading()`, and name screens with `pane_title(...)`. Expose selected, disabled
and adjustable state rather than encoding it only in color. Mark decorative
images so readers do not announce them.

Try keyboard focus and activation as well as clicking. The
[accessibility guide](accessibility.md) shows focus, semantic actions, canvas
children and range controls. The **Text Input** demo provides controls to exercise on each platform.

## Animation and drawing

### Animate a value when state changes

Add `cranpose-animation` to use tweens and springs. Read an animation in the
phase that needs it. An opacity change belongs in the graphics layer:

```rust
use cranpose::prelude::*;
use cranpose_animation::{Easing, animateFloatAsState, tween};

#[composable]
fn FadingLabel(visible: bool) {
    let alpha = animateFloatAsState(
        if visible { 1.0 } else { 0.0 },
        tween(180, Easing::EaseOut),
        "label-opacity",
    );
    Text(
        "Saved",
        Modifier::empty().graphics_layer(move || GraphicsLayer {
            alpha: alpha.get(),
            ..Default::default()
        }),
        TextStyle::default(),
    );
}
```

The graphics-layer closure reads the changing value without rebuilding the
surrounding layout on each frame. The label still occupies space when transparent;
use `AnimatedVisibility` when entering or leaving content should be animated.
Use `Crossfade` to change between two pieces of content. `spring(...)` gives a
value spring motion instead of a fixed-duration tween.

### Draw custom content

`Canvas` gives you a draw scope in local coordinates. This composable draws a
circle in a fixed-size area:

```rust
use cranpose::prelude::*;

#[composable]
fn StatusDot() {
    Canvas(Modifier::empty().size_points(24.0, 24.0), |draw| {
        draw.draw_circle(
            Brush::solid(Color::from_rgb_u8(40, 160, 100)),
            Point { x: 12.0, y: 12.0 },
            8.0,
        );
    });
}
```

Use `draw_behind` to draw behind a widget, `Image` with an `ImageBitmap` for
pixels, and `graphics_layer` for transforms and opacity. Keep decoded images and
static geometry outside work repeated every frame. Custom drawing needs its own
semantics when it conveys information or exposes controls.

### Use a component theme

The `cranpose::liquid` module provides a theme, typography, colors and glass
controls. Put the theme around the screen:

```rust
use cranpose::liquid::prelude::*;
use cranpose::prelude::*;

#[composable]
fn ThemedScreen() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        Text(
            "Your library",
            Modifier::empty(),
            liquid_typography().title3,
        );
    });
}
```

Use `GlassSurface` for a material-backed container, `GlassButton` and
`GlassIconButton` for actions, and `LiquidTabBar` for tabs. Their specs control
appearance; the content remains normal composables. The showcase's
[widgets](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/src/widgets)
show these controls over a drawn star field. Try the **Liquid UI** demo to see
the available components together.

## Navigation and data

### Describe destinations as Rust data

Add `cranpose-navigation`. A route enum keeps navigation arguments typed:

```rust
use cranpose::prelude::*;
use cranpose_navigation::{NavHost, rememberNavController};

#[derive(Clone, Copy, PartialEq)]
enum Route {
    Home,
    Article(u64),
}

#[composable]
fn App() {
    let nav = rememberNavController(Route::Home);
    NavHost(nav, move |route| match route {
        Route::Home => {
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || nav.navigate(Route::Article(42)),
                || {
                    Text("Read article", Modifier::empty(), TextStyle::default());
                },
            );
        }
        Route::Article(id) => {
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || {
                    nav.navigate_up();
                },
                move || {
                    Text(
                        format!("Article {id}: go back"),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                },
            );
        }
    });
}
```

`NavHost` supplies a view-model store per entry and handles back navigation.
Use `navigate_with` and `NavOptions` when opening a destination should replace
part of the stack. Use `NavHostWith` to choose the transition. Keep business
operations in your data layer; a route should describe where to go.

### Collect a flow in a screen

Add `coroflow` and `cranpose-coroflow` when your data layer exposes flows.
`StateFlow` holds a current value; collecting it makes that value observable by
composition:

```rust
use coroflow::StateFlow;
use cranpose::prelude::*;
use cranpose_coroflow::{Handle, StateFlowCollect};

#[composable]
fn DownloadStatus(progress: Handle<StateFlow<u32>>) {
    let percent = progress.get().collectAsStateWithLifecycle();
    Text(
        format!("Downloaded {}%", percent.get()),
        Modifier::empty(),
        TextStyle::default(),
    );
}
```

The parent can create the handle with `rememberHandle(|| repository.progress())`,
where `repository.progress()` returns its flow. `Handle` provides a stable,
copyable composable input for objects such as `StateFlow` that do not implement
value equality.

Lifecycle-aware collection pauses while the host is inactive. Use
`collectAsState` when collection should continue for the composition's whole
lifetime. Keep each collector near the UI that displays its result.

### Keep business logic across screen recomposition

`cranpose_coroflow::viewModel(key, |scope| ...)` returns a `Handle<VM>` from the
current view-model store. The supplied `MainScope` owns the model's coroutines.
A screen under `NavHost` already has a store. Wrap a standalone screen in
`ViewModelStoreOwner` when it needs one.

The showcase's [BodyCard](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/screens/list_screen.rs)
asks for a view model keyed by the body's ID. The
[view model](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/presentation/body_card_view_model.rs)
exposes saved state and a fetched fact as flows, and handles the save action.
Scrolling the card offscreen does not lose its fetched result because the
screen's store still owns the model.

Use `rememberHandle` for a shared object that only needs a composition lifetime.
Use `SavedStateHandle` for model values that need the store's save/restore
support, and durable storage for user documents.

## Platform services

### Open a link

Cranpose services hide the platform-specific call. Read a service from its
composition local, then invoke it in an event handler:

```rust
use cranpose::prelude::*;

#[composable]
fn HelpButton() {
    let uri = local_uri_handler().current();
    let error = rememberMutableStateOf(|| None::<String>);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        let uri = uri.clone();
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || {
                error.set(
                    uri.open_uri("https://example.com/help")
                        .err()
                        .map(|failure| failure.to_string()),
                );
            },
            || {
                Text("Help", Modifier::empty(), TextStyle::default());
            },
        );
        if let Some(message) = error.get() {
            Text(message, Modifier::empty(), TextStyle::default());
        }
    });
}
```

The service handle is shared with a callback retained by the button. The
platform launcher installs the backend; the screen does not need Android
intents, UIKit calls or browser APIs. The showcase uses this in its
[source link](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/widgets/source_link.rs).

### Let the user choose a file

Use a remembered launcher with a unique, stable request key. Cranpose uses the
key to deliver a recovered Android picker result after activity recreation:

```rust
use cranpose::prelude::*;

#[composable]
fn ImportFile() {
    let status = rememberMutableStateOf(|| String::from("Choose a document"));
    let picker = rememberOpenFileLauncher("library.import", move |result| {
        status.set(match result {
            Ok(Some(content)) => format!("Selected {}", content.metadata().name),
            Ok(None) => String::from("No file selected"),
            Err(error) => format!("Could not open: {error}"),
        });
    });
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        let picker = picker.clone();
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || picker.launch(FilePickerOptions::default().with_title("Import")),
            || {
                Text("Choose file", Modifier::empty(), TextStyle::default());
            },
        );
        Text(status.get(), Modifier::empty(), TextStyle::default());
    });
}
```

The result is a `ContentHandle`, not necessarily a filesystem path. Read through
its content API so the same code can handle a desktop file, an Android document
provider or a browser selection. Use `rememberSaveDocumentLauncher` to choose an
output document, or `rememberOpenFilesLauncher` for multiple selections.

[Cranamp's import code](https://github.com/samoylenkodmitry/cranamp/blob/main/src/audio.rs)
walks selected content, prepares audio tracks and supplies them to the media
service. It is an example of keeping provider-specific access outside widgets.

### Play media

Enable `cranpose/media` for the in-process desktop/Android media backend.
iOS and web use their platform players through the same service API:

```rust
use cranpose::prelude::*;

fn play_track(uri: String) -> Result<(), MediaError> {
    let item = MediaItem::new(uri).with_metadata(MediaMetadata::titled("Evening walk"));
    open_media(item)?;
    play_media()
}
```

Call this from an action with a URI your source can supply. Use `pause_media`,
`seek_media_fraction`, and `set_media_volume` for controls. For low-latency game
sounds, use the separate audio API and enable `audio` (`audio-desktop` for the
desktop output backend). [Cranamp](https://github.com/samoylenkodmitry/cranamp)
provides a complete player built around these APIs.

### Capture a photo and react to device events

Android and iOS include their camera integration with the platform host. On
macOS, enable `cranpose/camera-desktop` and run a signed app bundle. Declare the
platform's camera permission or usage description in your app package. Check
`camera_supported()` before offering capture. A supported camera can still fail
to start, for example when permission is denied.

Pair `start_camera` and `stop_camera` with the screen's lifetime as shown in
**State and effects**. Use `rememberCameraState()` for starting, active and error
state. Subscribe to `rememberCameraFrames()` through `cranpose_core::CollectEvents`
for preview frames, and call `capture_camera_still().await` for a full photo.
Keep image processing off the UI thread.

[CranScan's capture screen](https://github.com/samoylenkodmitry/cranscan/blob/main/app/src/ui/capture.rs)
combines a disposable camera session, frame events and asynchronous processing.
Its [service layer](https://github.com/samoylenkodmitry/cranscan/blob/main/app/src/services.rs)
keeps image decoding and document processing separate from composables.

For tactile feedback, call a haptics service from the action that needs it:

```rust
use cranpose::prelude::*;

fn confirm_selection() {
    default_haptics().vibrate(18, 120);
}
```

The arguments are duration in milliseconds and amplitude. Use `HapticPattern`
for a sequence. [Cranorbit's haptic director](https://github.com/samoylenkodmitry/cranorbit/blob/main/app/src/game/haptics.rs)
selects patterns for game events and applies the user's intensity setting.

### Find the service for your task

The facade re-exports `cranpose-services`; most app code can keep using
`cranpose::prelude::*`.

- **HTTP requests:** `local_http_client`, `HttpClient`, `HttpRequest`.
- **Open URLs:** `local_uri_handler`.
- **Choose or save documents:** `rememberOpenFileLauncher`, `rememberSaveDocumentLauncher`, `ContentHandle`.
- **Share content:** `local_share_sheet`, `ShareContent`.
- **Receive shared content:** `rememberIncomingContent`.
- **Preferences and app directories:** `preferences`, `application_directories`.
- **Notifications:** `local_notifier`.
- **Camera:** `rememberCameraState`, `rememberCameraFrames`, `capture_camera_still`.
- **Media and game audio:** `open_media`, `play_media`, `audio`.
- **Haptics:** `default_haptics`, `HapticPattern`.
- **App lifecycle:** `rememberLifecycleState`, `cranpose::LifecycleEffect`.
- **Keep the display awake:** `cranpose::KeepScreenOn`.
- **Purchases:** `purchases`, `rememberStoreState`, `rememberPurchaseEvents`.

Use the [services API](https://docs.rs/cranpose-services/latest/cranpose_services/)
for signatures and the [capability guide](capability_parity.md) for target support.
Check the relevant support result and handle errors at the action boundary. A
shared API does not mean that every device has a camera, store or share sheet.

## Compose integration

### Put a Rust screen inside an Android Compose screen

You can migrate one part of an app at a time. Keep your existing Activity,
Compose navigation and Kotlin data layer. Host a Cranpose component using
`AndroidView`, then exchange application events with Rust.

There are three pieces: a Rust factory returning `NativeSession`, the generated
UniFFI bindings, and the Android `CranposeView`. Cranpose owns the component's
rendering and input scheduling.

Start from the [native demo](../apps/native-demo/README.md) for the library and
binding setup. In your Rust library, add `cranpose`, `cranpose-native` and
`uniffi`, enable `renderer-wgpu` on Cranpose, and build a `cdylib`. Define a
factory like this:

```rust
use cranpose::prelude::*;
use cranpose_native::{NativeContent, NativeSession, SendToHost};
use std::{cell::Cell, rc::Rc, sync::Arc};

uniffi::setup_scaffolding!();

#[uniffi::export]
pub fn create_counter() -> Arc<NativeSession> {
    NativeSession::new(|| {
        let commands = Rc::new(Cell::new(None::<MutableState<i32>>));
        let content_commands = Rc::clone(&commands);
        NativeContent::new(move || {
            let count = rememberMutableStateOf(|| 0_i32);
            content_commands.set(Some(count));
            Column(
                Modifier::empty().padding(16.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        format!("Rust count: {}", count.get()),
                        Modifier::empty(),
                        TextStyle::default(),
                    );
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        move || count.update(|value| *value += 1),
                        || {
                            Text("Increment in Rust", Modifier::empty(), TextStyle::default());
                        },
                    );
                    SendToHost("count", count.get().to_string());
                },
            );
        })
        .on_event(move |event| {
            if event.name == "increment" {
                if let Some(count) = commands.get() {
                    count.update(|value| *value += 1);
                }
            }
        })
    })
}
```

Create remembered state inside the content closure, where a composition is
active. The shared cell lets the session's command handler reach that state on
the same worker thread. `SendToHost` publishes after composition commits. The event
names and string payloads are your application's protocol; use structured
serialization for richer data.

Generate bindings using the native demo's
[bindgen entry point](../apps/native-demo/src/bin/bindgen.rs) and
[Android build script](../scripts/native_demo.sh).
Set the generated bindings' `cdylib_name` to your Rust library's name and package
that library for each Android ABI you support. The build script shows both the
runtime and application bindings; your app needs both.

Include the framework's `platforms/android` library as a Gradle module, as in
the demo's [settings](../apps/native-demo/android/settings.gradle.kts), set
`cranposeBindingsDir` to the generated runtime Kotlin sources, and add the
module to your app's dependencies:

```kotlin
dependencies {
    implementation(project(":cranpose"))
}
```

Assuming the Rust library is named `counter_ui`, UniFFI exposes its factory as
`uniffi.counter_ui.createCounter`. This Compose screen sends button actions to
Rust and displays the resulting count in Kotlin:

```kotlin
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import dev.cranpose.CranposeView
import uniffi.counter_ui.createCounter

@Composable
fun CounterScreen() {
    var count by remember { mutableIntStateOf(0) }
    var error by remember { mutableStateOf<String?>(null) }
    var component by remember { mutableStateOf<CranposeView?>(null) }
    Column {
        Text("Kotlin count: $count")
        Button(onClick = { component?.sendEvent("increment") }) {
            Text("Increment in Kotlin")
        }
        AndroidView(
            modifier = Modifier.fillMaxWidth().height(200.dp),
            factory = { context ->
                CranposeView(context, createCounter()).also { component = it }
            },
            update = { view ->
                view.onEvent = { event ->
                    if (event.name == "count") {
                        event.value.toIntOrNull()?.let { count = it }
                    }
                }
                view.onError = { error = it }
            },
            onRelease = { view ->
                if (component === view) component = null
                view.close()
            },
        )
        error?.let { Text(it) }
    }
}
```

Both buttons update the Rust-owned count. Kotlin observes the result through
`onEvent`. Keeping one owner avoids a loop where each side writes the other's
state back. For Kotlin-owned data, use the reverse arrangement: send values to
Rust and have Rust emit actions for Kotlin to handle.

Create the view in `factory`; `update` refreshes its inputs and callbacks.
`onRelease` closes the session when Compose discards the view. Temporary
attachment changes preserve its state. This follows Android's
[View-in-Compose lifecycle](https://developer.android.com/develop/ui/compose/migrate/interoperability-apis/views-in-compose).

### Choose the boundary deliberately

Kotlin and Rust have separate compositions and state stores. Exchange values
and actions; do not pass composable functions across the boundary. A Kotlin
navigation destination can own the Rust component, or a Rust screen can send an
`open-details` event for Kotlin navigation to handle.

The native component adapters currently forward one touch pointer and do not
bridge Rust text input or accessibility. Keep editable or reader-accessible
controls in the native host when using this embedding path. Standalone Cranpose
applications use the regular platform input and accessibility bridges.

### Host the same component in UIKit

Generate Swift bindings for the same Rust library and import the runtime host
module. The iOS [native demo](../apps/native-demo/ios/App.swift) shows module
linking and view-controller ownership. Inside a view controller:

```swift
import UIKit
import Cranpose
import CranposeBindings

let counter = CranposeView(session: createCounter())
counter.onEvent = { event in
    if event.name == "count" {
        print("Rust count: \(event.value)")
    }
}
counter.onError = { message in print(message) }
view.addSubview(counter)
counter.translatesAutoresizingMaskIntoConstraints = false
NSLayoutConstraint.activate([
    counter.leadingAnchor.constraint(equalTo: view.leadingAnchor),
    counter.trailingAnchor.constraint(equalTo: view.trailingAnchor),
    counter.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
    counter.heightAnchor.constraint(equalToConstant: 200),
])
counter.sendEvent("increment")
```

Keep the component in a controller property and call `close()` when permanently
discarding it. A SwiftUI app can wrap this UIKit view in `UIViewRepresentable`
and close it from `dismantleUIView`.

## Native views

**Place a platform control in your Cranpose layout.**

Use a native child when you need a platform SDK's view, such as a browser or map.
Cranpose allocates its layout bounds; the native view keeps its own rendering
and input.

### Show a website

For a standalone app, enable the `cranpose/webview` feature and use `WebView`:

```rust
use cranpose::prelude::*;

#[composable]
fn HelpPage() {
    let error = rememberMutableStateOf(|| None::<String>);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        WebView(
            "https://example.com",
            Modifier::empty().fill_max_width().height(320.0),
            move |event| match event {
                WebViewEvent::Loaded(_) => error.set(None),
                WebViewEvent::Failed(message) => error.set(Some(message)),
            },
        );
        if let Some(message) = error.get() {
            Text(message, Modifier::empty(), TextStyle::default());
        }
    });
}
```

When the screen runs inside `NativeSession`, register `WebViewFactory` with its
native host instead. Android uses an Android WebView; iOS uses WKWebView:

```kotlin
val component = CranposeView(
    context,
    createHelpComponent(),
    mapOf("web" to WebViewFactory()),
)
```

`createHelpComponent` is your generated factory, built like `createCounter` in
the preceding chapter with `HelpPage` as its content. Import `CranposeView` and
`WebViewFactory` from `dev.cranpose`. Add Android's `INTERNET` permission. The
native demo includes both Android and iOS factory registration.

### Wrap your own Android control

In an embedded Rust screen, call `cranpose_native::NativeView` with a factory
name, configuration and callback:

```rust
use cranpose::prelude::*;
use cranpose_native::NativeView;

#[composable]
fn NativeVolume() {
    let volume = rememberMutableStateOf(|| 50_u32);
    NativeView(
        "volume",
        &volume.get().to_string(),
        Modifier::empty().fill_max_width().height(56.0),
        move |event| {
            if let Ok(value) = event.parse::<u32>() {
                volume.set(value.min(100));
            }
        },
    );
}
```

On Android, register a factory named `volume`. This adapter uses a real `SeekBar`
and emits changes made by the user:

```kotlin
import android.content.Context
import android.widget.SeekBar
import dev.cranpose.NativeViewChild
import dev.cranpose.NativeViewFactory

class VolumeFactory : NativeViewFactory {
    override fun create(context: Context, emit: (String) -> Unit): NativeViewChild {
        val slider = SeekBar(context).apply { max = 100 }
        slider.setOnSeekBarChangeListener(object : SeekBar.OnSeekBarChangeListener {
            override fun onProgressChanged(bar: SeekBar, value: Int, fromUser: Boolean) {
                if (fromUser) emit(value.toString())
            }
            override fun onStartTrackingTouch(bar: SeekBar) {}
            override fun onStopTrackingTouch(bar: SeekBar) {}
        })
        return object : NativeViewChild {
            override val view = slider
            override fun update(value: String) {
                val next = value.toIntOrNull()?.coerceIn(0, 100) ?: return
                if (slider.progress != next) slider.progress = next
            }
            override fun dispose() {
                slider.setOnSeekBarChangeListener(null)
            }
        }
    }
}
```

Pass `mapOf("volume" to VolumeFactory())` as the third argument to `CranposeView`.
Use a factory returning `NativeContent::new(NativeVolume)` for this screen.
Configuration changes call `update` on the existing child. Removing the
composable calls `dispose`; changing its factory name replaces the child.
The callback reaches only the slot that created it.

The iOS [NativeViewFactory protocol](../platforms/ios/Sources/Cranpose/NativeViewFactory.swift)
provides the same create, update, visibility and disposal contract. Implement it
with a `UISlider` and register it under the same `volume` name to reuse the Rust
screen. For a custom Rust host, `cranpose::native_view::NativeViewHost` exposes
the lower-level layout and event contract.

### Lay out native children within their limits

Give every native slot an explicit size. Native children sit above Cranpose's
drawing and are clipped to the host container. Keep them axis-aligned; arbitrary
transforms, rounded ancestor clips and Cranpose effects over a native child are
not supported. Hide the child while an overlapping Cranpose popup is open.

The [native demo](../apps/native-demo/README.md) is the runnable example for both
directions: host controls update Rust state, and Rust positions a native website
view. Use it as the starting project when an existing app owns the window.
