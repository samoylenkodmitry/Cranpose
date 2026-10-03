# Build apps with Cranpose

## Welcome

**Build apps with Compose in Rust.**

Cranpose uses the Jetpack Compose model. Composable functions describe the UI.
State changes update it. Callbacks handle user actions.
Cranpose handles the platform window, input, graphics and device services.

### Map Compose APIs to Rust

- `@Composable fun Screen()` → `#[composable] fn Screen()`.
- `remember { mutableStateOf(0) }` → `rememberMutableStateOf(|| 0)`.
- `count.value` → `count.get()` and `count.set(value)`.
- `Column { ... }` → `Column(modifier, spec, move || { ... })`.
- `Modifier.padding(16.dp)` → `Modifier::empty().padding(16.0)`.
- `LazyColumn { items(...) }` → `LazyColumn(modifier, state, spec, move |scope| { ... })`.
- `LaunchedEffect(key)` → `LaunchedEffect` or `LaunchedEffectAsync`.
- `DisposableEffect(key)` → `DisposableEffect` with `DisposableEffectResult`.
- `CompositionLocalProvider` → `CompositionLocalProvider`.
- `NavHost` → `NavHost` with `rememberNavController`.
- `collectAsStateWithLifecycle()` → `StateFlowCollect::collectAsStateWithLifecycle()`.

Rust closures replace Kotlin lambdas. Specs hold container options.
State flows down. Events flow up. Effects own tasks and resources.

Start with **Get started** for a new app. **Compose integration** adds Rust UI
to an Android Compose app. **Native views** adds platform controls to Rust UI.
The guide works offline. Source links open in a browser.

## Get started

### Create an application

Install Rust. Create a desktop app:

```sh
cargo new my-app
cd my-app
cargo add cranpose --features desktop,renderer-wgpu
cargo add anyhow
```

For Android, iOS and web hosts, use the
[showcase template](https://github.com/samoylenkodmitry/cranpose-showcase).

Replace `src/main.rs`:

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

Run `cargo run`.

`rememberMutableStateOf` keeps state between calls. A state handle is `Copy`.
Each `move` closure receives a handle to the same value.

Use `cargo run --release` to measure performance.
Set the release profile in your app's `Cargo.toml`:

```toml
[profile.release]
opt-level = 3
lto = true
codegen-units = 1
```

### Share the UI across platforms

Put public composables in your library. Each platform entry point starts the
same UI. Enable `renderer-wgpu` plus the target feature:

- **`desktop`:** call `AppLauncher::try_run` from `main`.
- **`android`:** package the Rust library with the Android Gradle host.
- **`ios`:** start the UIKit host. Package and sign the app with Xcode tools.
- **`web`:** build for `wasm32-unknown-unknown`. Load the module from HTML.

The template supplies [Cargo features](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/Cargo.toml),
[Android setup](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/android),
[iOS scripts](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/ios)
and a [web build](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/build-web.sh).

## State and effects

### Give state an owner

Put state in the parent that needs it. Pass values and callbacks to children:

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

Change state in callbacks and effects.

### Borrow a state value

Use `.get()` for small values. Use `.with(...)` to borrow a collection:

```rust
use cranpose::prelude::*;

#[composable]
fn DraftCount(drafts: MutableState<Vec<String>>) {
    let count = drafts.with(Vec::len);
    Text(
        format!("Drafts: {count}"),
        Modifier::empty(),
        TextStyle::default(),
    );
}
```

- `remember(|| value)` retains an ordinary value. `.with(...)` borrows it.
- `rememberMutableStateOf` retains observable state. Writes update its readers.
- `rememberKeyed` recreates a value when its key changes.
- `key` gives a group a stable identity across moves.

Remembered state lasts while its composition position exists.
Use a data store or preferences for state that survives app exit.

### Load data when an input changes

`LaunchedEffectAsync` owns an async task. Its key controls when it restarts:

```rust
use cranpose::prelude::*;

#[composable]
fn RemoteText(url: String) {
    let result = rememberMutableStateOf(|| String::from("Please wait"));
    let client = local_http_client().current();
    LaunchedEffectAsync(url.clone(), move |_| {
        Box::pin(async move {
            result.set(String::from("Please wait"));
            result.set(match client.get_text(&url).await {
                Ok(text) => text,
                Err(error) => format!("Request error: {error}"),
            });
        })
    });
    Text(result.get(), Modifier::empty(), TextStyle::default());
}
```

Use `cranpose-services/http-native` on desktop, Android and iOS.
Use `cranpose-services/web-http` in browsers.
Android needs `INTERNET` in the manifest. Browser requests follow the server's CORS policy.

The URL supplies the key and request input. Each role owns a copy.
A key change or composition exit cancels the task.
The async body runs on the UI runtime. Use it for I/O.

### Move computation to a worker

`launch_background` computes on a native worker. Its result callback runs on the UI thread:

```rust
use cranpose::prelude::*;

#[composable]
fn WordCount(text: std::sync::Arc<str>) {
    let words = rememberMutableStateOf(|| 0_usize);
    LaunchedEffect(std::sync::Arc::clone(&text), move |scope| {
        scope.launch_background(
            move |_| async move { text.split_whitespace().count() },
            move |count| words.set(count),
        );
    });
    Text(
        format!("Words: {}", words.get()),
        Modifier::empty(),
        TextStyle::default(),
    );
}
```

`Arc<str>` shares the input with the key and worker.
On web, this API runs on the browser event loop. Use a Web Worker for large computations.

### Start a task from a button

`rememberCoroutineScope` ties button tasks to the screen's lifetime:

```rust
use cranpose::prelude::*;
use std::time::Duration;

#[composable]
fn DelayedMessage() {
    let message = rememberMutableStateOf(|| "Ready");
    let scope = rememberCoroutineScope();
    Button(
        Modifier::empty(),
        ButtonSpec::default(),
        move || {
            message.set("Please wait");
            scope.launch(async move {
                delay(Duration::from_secs(1)).await;
                message.set("Done");
            });
        },
        move || {
            Text(message.get(), Modifier::empty(), TextStyle::default());
        },
    );
}
```

### Release a resource when its screen closes

`DisposableEffect` acquires a resource. Its result supplies cleanup:

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

Mount this composable while the screen needs the camera. Its removal stops the camera.
Use `rememberUpdatedState` to pass fresh values into an effect with a stable key.
Use `SideEffect` to publish values after composition commits.

### Provide a value to child composables

A composition local supplies a value to descendants:

```rust
use cranpose::prelude::*;
use cranpose_core::{CompositionLocal, CompositionLocalProvider, compositionLocalOf};

thread_local! {
    static LOCAL_USER: CompositionLocal<&'static str> = compositionLocalOf(|| "Guest");
}

#[composable]
fn UserLabel() {
    let user = LOCAL_USER.with(CompositionLocal::current);
    Text(user, Modifier::empty(), TextStyle::default());
}

#[composable]
fn AccountScreen() {
    LOCAL_USER.with(|user| {
        CompositionLocalProvider([user.provides("Ada")], UserLabel);
    });
}
```

Outside this provider, `UserLabel` reads the default: `Guest`.

## Layout and lists

### Arrange controls

`Column` stacks children vertically. `Row` places them side by side. `Box` places children in layers:

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

`weight(1.0)` gives the name the spare width. The status keeps its content width.
Sizes use logical points. The host applies display density. Text uses `TextUnit::Sp` for font scale.

### Order modifiers

Each modifier wraps the next:

```rust
use cranpose::prelude::*;

fn card_modifiers() -> (Modifier, Modifier) {
    let blue = Color::from_rgb_u8(40, 90, 180);
    let outer_space = Modifier::empty().padding(16.0).background(blue);
    let inner_space = Modifier::empty().background(blue).padding(16.0);
    (outer_space, inner_space)
}
```

`outer_space` leaves space outside the background. `inner_space` includes the space in the background.

Use `width_in` and `height_in` for size limits. Use `align` for a child in a `Box`.
See the [modifier reference](MODIFIERS.md).

### Show a long collection

`LazyColumn` composes rows for its viewport:

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

This fixed list uses positions as identities. Use data IDs when rows move:

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

For large lists, use `LazyItems::new(count).key(...)`. Use `content_type(...)` to group rows with the same structure.
Give lists a bounded height. Share large datasets with retained row callbacks.
See the [lazy-list guide](lazy_list_doc.md).

### Adapt to screen size

`report_size_state` reports the container size. Choose the layout from its width.
Keep shared state in the parent of both layouts.

Use safe-area insets to clear system bars:

```rust
use cranpose::prelude::*;

#[composable]
fn ScreenContent() {
    let insets = local_safe_area_insets().current();
    Column(
        Modifier::empty().fill_max_size().padding_each(
            insets.left,
            insets.top,
            insets.right,
            insets.bottom,
        ),
        ColumnSpec::default(),
        || {
            Text("Library", Modifier::empty(), TextStyle::default());
        },
    );
}
```

The showcase's [RootShell and SplitShell](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/app.rs)
show one pane on phones and two panes in wide windows.

## Text and input

### Style a label

`SpanStyle` sets character style. `ParagraphStyle` sets paragraph layout:

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

Use `AnnotatedString` for mixed styles. `LinkedText` sends link actions to a callback.
Use `AppFonts` for fonts in your app. Include fonts for each script your app displays.
See the **Text** demo for examples.

### Edit text

`TextFieldState` owns text, selection and IME composition:

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

`BasicTextFieldWithOptions` sets keyboard options.
`BasicTextFieldDecorated` adds your label and border.
Test the form with the platform keyboard, IME, selection and clipboard.

### Support touch, keyboard and screen readers

Use `Button` for actions. Use `clickable` for custom surfaces.
Its callback receives a pointer position. Use `pointer_input` for custom gestures.

Add semantics to custom controls:

- `content_description(...)` names a control.
- `heading()` marks a section title.
- `pane_title(...)` names a screen.

Expose selected, disabled and adjustable state. Mark decorative images.
Test focus and activation through touch, keyboard and screen readers.
See the [accessibility guide](accessibility.md) and **Text Input** demo.

## Animation and graphics

### Animate a value

Add `cranpose-animation`. This tween changes opacity over 180 milliseconds:

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

The layer reads `alpha` each frame. The label keeps its layout space.
Use `AnimatedVisibility` for content entry and exit. Use `Crossfade` to switch content.
Use `spring(...)` for spring motion.

### Draw custom content

`Canvas` uses local coordinates:

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

Use `draw_behind` for a widget background. Use `Image` and `ImageBitmap` for pixels.
Retain decoded images and static geometry between frames. Add semantics to custom canvas controls.

### Apply a theme

`cranpose::liquid` supplies colors, typography and glass controls:

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

Use `GlassSurface` for containers, `GlassButton` for actions, and `LiquidTabBar` for tabs.
Specs set their appearance. See the [showcase widgets](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/src/widgets)
and **Liquid UI** demo.

## Navigation and data

### Define destinations

Add `cranpose-navigation`. Use an enum for routes and their arguments:

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

`NavHost` handles back navigation and owns a view-model store per entry.
Use `navigate_with` and `NavOptions` to replace part of the stack.
Use `NavHostWith` to set the transition.

### Collect a flow

Add `coroflow` and `cranpose-coroflow`. A collector exposes the flow's value as UI state:

```rust
use coroflow::StateFlow;
use cranpose::prelude::*;
use cranpose_coroflow::{Handle, StateFlowCollect};

#[composable]
fn DownloadStatus(progress: Handle<StateFlow<u32>>) {
    let percent = progress.get().collectAsStateWithLifecycle();
    Text(
        format!("Download: {}%", percent.get()),
        Modifier::empty(),
        TextStyle::default(),
    );
}
```

Create the handle in the parent with `rememberHandle(|| repository.progress())`.
`repository.progress()` returns a `StateFlow<u32>`. `Handle` gives it stable identity as a composable input.

`collectAsStateWithLifecycle` pauses while the host is inactive.
`collectAsState` collects for the composition's lifetime.

### Keep state in a view model

Call this screen from `NavHost`. Its entry owns the model:

```rust
use coroflow::MutableStateFlow;
use cranpose::prelude::*;
use cranpose_coroflow::{StateFlowCollect, viewModel};

#[composable]
fn CounterRoute() {
    let model = viewModel((), |_| MutableStateFlow::new(0_u32));
    let count = model.get().as_state_flow().collectAsStateWithLifecycle();
    Button(
        Modifier::empty(),
        ButtonSpec::default(),
        move || {
            let model = model.get();
            model.set(model.value() + 1);
        },
        move || {
            Text(
                format!("Count: {}", count.get()),
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}
```

Use an item ID as the key for multiple models. The factory receives a `MainScope` for model tasks.
Wrap a standalone screen in `ViewModelStoreOwner` to supply a store.
Use `SavedStateHandle` for state restore and a data store for documents.

The showcase's [BodyCard](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/screens/list_screen.rs)
uses one model per body ID. Its [model](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/presentation/body_card_view_model.rs)
retains a fact and saved status as flows.

## Platform services

### Open a link

The launcher installs platform services. Read a service, then call it from an action:

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

See the showcase's [source link](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/widgets/source_link.rs).

### Choose a file

Give each launcher a unique request key. Android uses it to restore results after activity recreation:

```rust
use cranpose::prelude::*;

#[composable]
fn ImportFile() {
    let status = rememberMutableStateOf(|| String::from("Choose a document"));
    let picker = rememberOpenFileLauncher("library.import", move |result| {
        status.set(match result {
            Ok(Some(content)) => format!("Selected {}", content.metadata().name),
            Ok(None) => String::from("Choose a document"),
            Err(error) => format!("File error: {error}"),
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

`ContentHandle` supports files, Android document providers and browser selections.
For a small UTF-8 document:

```rust
use cranpose::prelude::*;

async fn read_document(content: ContentHandle) -> anyhow::Result<String> {
    let bytes = content.read_all().await?;
    Ok(String::from_utf8(bytes)?)
}
```

For large files, call `open()` and read chunks with `read_chunk()`.
Use `rememberSaveDocumentLauncher` for output documents and `rememberOpenFilesLauncher` for multiple files.
[Cranamp's import code](https://github.com/samoylenkodmitry/cranamp/blob/main/src/audio.rs) loads audio through these content APIs.

### Play media

Enable `cranpose/media` for desktop and Android. iOS and web use platform players:

```rust
use cranpose::prelude::*;

fn play_track(uri: String) -> Result<(), MediaError> {
    let item = MediaItem::new(uri).with_metadata(MediaMetadata::titled("Evening walk"));
    open_media(item)?;
    play_media()
}
```

Call this from a user action. Use `pause_media`, `seek_media_fraction` and `set_media_volume` for controls.
For game sounds, enable `audio`; use `audio-desktop` on desktop.
See [Cranamp](https://github.com/samoylenkodmitry/cranamp) for a player app.

### Use the camera

Android and iOS supply camera backends. macOS needs `cranpose/camera-desktop` and a signed app bundle.
Declare the camera permission or usage description in the app package.
Check `camera_supported()`. Handle errors from `start_camera()`.

- `DisposableEffect` starts and stops the camera with the screen.
- `rememberCameraState()` exposes camera status.
- `CollectEvents(rememberCameraFrames(), key, callback)` receives preview frames.
- `capture_camera_still().await` returns a photo.

[CranScan's capture screen](https://github.com/samoylenkodmitry/cranscan/blob/main/app/src/ui/capture.rs)
uses these APIs. Its [service layer](https://github.com/samoylenkodmitry/cranscan/blob/main/app/src/services.rs)
processes photos off the UI thread.

### Add haptic feedback

Call haptics from a user action:

```rust
use cranpose::prelude::*;

fn confirm_selection() {
    default_haptics().vibrate(18, 120);
}
```

The arguments are milliseconds and amplitude. `HapticPattern` defines a sequence.
[Cranorbit](https://github.com/samoylenkodmitry/cranorbit/blob/main/app/src/game/haptics.rs) maps game events to patterns.

### Find a service

`cranpose::prelude::*` includes the service APIs:

- **HTTP:** `local_http_client`, `HttpClient`, `HttpRequest`.
- **URLs:** `local_uri_handler`.
- **Documents:** `rememberOpenFileLauncher`, `rememberSaveDocumentLauncher`, `ContentHandle`.
- **Share:** `local_share_sheet`, `ShareContent`.
- **Receive shared content:** `rememberIncomingContent`.
- **Preferences and directories:** `preferences`, `application_directories`.
- **Notifications:** `local_notifier`.
- **Camera:** `rememberCameraState`, `rememberCameraFrames`, `capture_camera_still`.
- **Media and audio:** `open_media`, `play_media`, `audio`.
- **Haptics:** `default_haptics`, `HapticPattern`.
- **Lifecycle:** `rememberLifecycleState`, `cranpose::LifecycleEffect`.
- **Display power:** `cranpose::KeepScreenOn`.
- **Purchases:** `purchases`, `rememberStoreState`, `rememberPurchaseEvents`.

Check [target support](capability_parity.md). Handle support results and operation errors.
See the [services API](https://docs.rs/cranpose-services/latest/cranpose_services/) for signatures.

## Compose integration

### Put a Rust screen inside an Android Compose screen

`AndroidView` hosts `CranposeView` inside Compose. Keep the Activity, navigation and data layer in Kotlin.
Exchange values and actions with Rust through events.

Use the [native demo](../apps/native-demo/README.md) as the project template.
Add `cranpose`, `cranpose-native` and `uniffi` to your Rust library.
Enable `cranpose/renderer-wgpu`. Build a `cdylib`. Export a session factory:

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

The content closure creates state during composition.
The shared cell gives the command handler access on the same worker.
`SendToHost` emits an event after composition commits.

Generate the UniFFI code with the demo's
[bindgen entry point](../apps/native-demo/src/bin/bindgen.rs) and
[build script](../scripts/native_demo.sh).
Set `cdylib_name` to your library name. Package the library for each target ABI.
Include both runtime and application bindings.

Add `platforms/android` as a Gradle module. Follow the demo's
[settings](../apps/native-demo/android/settings.gradle.kts).
Point `cranposeBindingsDir` at the generated runtime Kotlin sources. Add the module dependency:

```kotlin
dependencies {
    implementation(project(":cranpose"))
}
```

For a library named `counter_ui`, the factory is `uniffi.counter_ui.createCounter`:

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

Rust owns the count. Both buttons send actions to Rust. Kotlin displays the result.
For Kotlin-owned state, send values to Rust and actions back to Kotlin.

- `factory` creates the view.
- `update` sets its callbacks and inputs.
- `onRelease` closes the session. Temporary detachment preserves state.

See Android's [View-in-Compose lifecycle](https://developer.android.com/develop/ui/compose/migrate/interoperability-apis/views-in-compose).

### Choose the host boundary

Each language owns its composition. Exchange data and events across the boundary.
For example, Rust sends `open-details`; Kotlin changes the navigation destination.

Embedded touch support covers one pointer.
Use native host controls for text input and accessibility in embedded components.
Standalone Cranpose apps use the platform input and accessibility bridges.

### Host the component in UIKit

Generate Swift bindings for the same library. Follow the [iOS native demo](../apps/native-demo/ios/App.swift).
Add these imports at file scope. Create the view inside your view controller:

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

Keep the component in a controller property. Call `close()` when the controller discards it.
For SwiftUI, use `UIViewRepresentable` and call `close()` from `dismantleUIView`.

## Native views

**Place a platform control in your Cranpose layout.**

Cranpose sets the child's bounds. The native view draws itself and handles input.

### Show a website

Enable `cranpose/webview` in a standalone app:

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

For a `NativeSession`, register `WebViewFactory` with the host.
Android uses WebView. iOS uses WKWebView:

```kotlin
val component = CranposeView(
    context,
    createHelpComponent(),
    mapOf("web" to WebViewFactory()),
)
```

Use the `createCounter` factory pattern with `HelpPage` as its content.
Import `CranposeView` and `WebViewFactory` from `dev.cranpose`.
Add Android's `INTERNET` permission.

### Wrap an Android control

Call `NativeView` inside a `NativeSession`. Pass a factory name, configuration and callback:

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

Register `volume` as a native `SeekBar` factory:

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

Pass `mapOf("volume" to VolumeFactory())` to `CranposeView`.
Use `NativeContent::new(NativeVolume)` in the session factory.

- Configuration changes call `update` on the same child.
- Slot removal calls `dispose`.
- A factory-name change replaces the child.
- Events reach the child's own slot.

On iOS, implement [NativeViewFactory](../platforms/ios/Sources/Cranpose/NativeViewFactory.swift)
with a `UISlider`. Register it as `volume` to reuse this Rust screen.
Custom Rust hosts use `cranpose::native_view::NativeViewHost` for layout and events.

### Size and position native children

Give each slot an explicit size. Keep its bounds axis-aligned.
Native children sit above Cranpose content. The host clips them to its bounds.
Apply transforms and effects within the native child.
Hide the child while a Cranpose popup overlaps it.

The [native demo](../apps/native-demo/README.md) shows both directions:
host controls update Rust state; Rust positions a native website view.
