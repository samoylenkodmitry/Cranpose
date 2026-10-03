# Build apps with Cranpose

## Welcome

**Build apps with Compose in Rust.**

Cranpose uses the Jetpack Compose model. Composable functions describe the UI.
Cranpose recomposes the UI when observed state changes. Callbacks handle user actions.
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
Parents pass state to children. Children send actions through callbacks.
Effects manage tasks and resources for the lifetime of a composable.

Start with **Get started** for a new app. **Compose integration** adds Rust UI
to an Android Compose app. **Native views** adds platform controls to Rust UI.

## Get started

### Create an application

Install the Rust toolchain with [rustup](https://rustup.rs/). Cargo creates packages,
resolves dependencies and runs the compiler. `Cargo.toml` serves the role of a
Gradle build file. A crate is a Rust library or executable.

Create a package with a shared UI library:

```sh
cargo new my-app --lib
cd my-app
cargo add cranpose --features renderer-wgpu
cargo add anyhow
```

- `src/lib.rs` contains the UI and app logic for every platform.
- `src/main.rs` starts the desktop or iOS executable.
- `Cargo.toml` lists dependencies, features and build targets.
- `Cargo.lock` records the resolved dependencies. Commit this file with the app.

Keep the generated package and dependency entries in `Cargo.toml`. Add these sections:

```toml
[lib]
crate-type = ["rlib", "cdylib"]

[features]
default = ["desktop", "renderer-wgpu"]
renderer-wgpu = ["cranpose/renderer-wgpu"]
desktop = ["cranpose/desktop"]
android = ["cranpose/android"]
ios = ["cranpose/ios"]
```

`rlib` lets another Rust crate use the library. `cdylib` produces the shared library
Android loads and the WebAssembly module browsers load. Cargo converts the package
name `my-app` to the Rust import name `my_app` and the Android library name `my_app`.

Cargo features select optional code and dependencies. For example,
`android = ["cranpose/android"]` forwards your app's `android` feature to Cranpose.
`--no-default-features` replaces the default desktop feature set for a target build.

Replace `src/lib.rs`:

```rust
use cranpose::prelude::*;

#[composable]
pub fn Counter() {
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
```

`rememberMutableStateOf` retains the count between compositions. The state handle
implements `Copy`, so each `move` closure receives access to the same count.
The closure syntax `move || { ... }` corresponds to a Kotlin lambda; `move`
transfers captured values into the closure.

### Run on desktop

Create `src/main.rs`:

```rust
use cranpose::AppLauncher;
use my_app::Counter;

fn main() -> anyhow::Result<()> {
    AppLauncher::new()
        .with_title("My app")
        .with_size(640, 480)
        .try_run(Counter)?;
    Ok(())
}
```

Run the executable from the package directory:

```sh
cargo run
```

Cargo builds the library and executable together. `?` returns a launch error to
`main`; `Ok(())` reports success. Use `cargo run --release` for an optimized build.

### Run on Android

The Gradle host builds `src/lib.rs` as `libmy_app.so` and packages the library in
the APK. Add the Android entry point at the end of `src/lib.rs`:

```rust
cranpose::android_main! {
    launcher: cranpose::AppLauncher::new().with_title("My app"),
    content: Counter,
}
```

The macro connects the Android Activity to `Counter`. The macro expands on Android
targets; the desktop build continues to use `src/main.rs`.

Install the Android SDK and NDK through Android Studio. Install the Rust targets
and Cargo's NDK build tool:

```sh
rustup target add aarch64-linux-android x86_64-linux-android
cargo install cargo-ndk
```

Copy the [starter's android directory](../apps/isolated-demo/android) into `my-app/android`.
The directory contains the Gradle wrapper, settings and manifest. The settings
resolve the Cranpose Gradle plugin from your Cargo dependency.

In `android/app/build.gradle.kts`, set the package, label and ABIs:

```kotlin
plugins {
    id("com.android.application")
    id("dev.cranpose.android")
}

cranpose {
    workspaceRoot.set("../..")
    cargoPackage.set("my-app")
    label.set("My app")
    debugAbis.set(listOf("arm64-v8a", "x86_64"))
}
```

Keep the template's `android` block. Set `namespace` and `applicationId` to your
app ID, such as `com.example.myapp`. Set `sdk.dir` in `android/local.properties`
to your Android SDK directory, as in an Android Studio project.

Start an emulator or connect a device with USB debug access. From `my-app`:

```sh
cd android
./gradlew :app:installDebug
```

The plugin invokes Cargo with `android,renderer-wgpu`, supplies the Cranpose Activity,
and packages the `.so` for each ABI. Open **My app** from the device launcher.

### Run on iOS

The iOS launcher starts UIKit and renders into a Metal surface. The desktop
`src/main.rs` also serves as the iOS entry point; the target and Cargo feature
select the host.

On a Mac with Xcode and an iOS Simulator runtime:

```sh
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
cargo build --bin my-app --target aarch64-apple-ios-sim --no-default-features --features ios,renderer-wgpu
```

Package the executable in an app bundle. Create `ios/Info.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>com.example.myapp</string>
    <key>CFBundleName</key><string>My app</string>
    <key>CFBundleExecutable</key><string>my-app</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleVersion</key><string>1</string>
    <key>CFBundleShortVersionString</key><string>1.0</string>
    <key>LSRequiresIPhoneOS</key><true/>
    <key>UILaunchScreen</key><dict/>
    <key>UIApplicationSceneManifest</key>
    <dict>
        <key>UIApplicationSupportsMultipleScenes</key><false/>
        <key>UISceneConfigurations</key>
        <dict>
            <key>UIWindowSceneSessionRoleApplication</key>
            <array><dict>
                <key>UISceneConfigurationName</key><string>Default Configuration</string>
                <key>UISceneDelegateClassName</key><string>CranposeSceneDelegate</string>
            </dict></array>
        </dict>
    </dict>
</dict>
</plist>
```

`CranposeSceneDelegate` connects the Cranpose window to the UIKit scene.
With an Apple Silicon simulator open, run from `my-app`:

```sh
mkdir -p target/MyApp.app
cp target/aarch64-apple-ios-sim/debug/my-app target/MyApp.app/my-app
cp ios/Info.plist target/MyApp.app/Info.plist
codesign --force --sign - target/MyApp.app
xcrun simctl install booted target/MyApp.app
xcrun simctl launch booted com.example.myapp
```

For a physical device, use the `aarch64-apple-ios` target and sign the bundle with
your Apple development identity and provisioning profile. The
[iOS build scripts](../apps/ios-demo/ios/build-app.sh) show the bundle and signature steps.

### Run in a browser

Add the WebAssembly export helpers:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
cargo add wasm-bindgen --optional
cargo add wasm-bindgen-futures --optional
```

Add `web` under `[features]` in `Cargo.toml`:

```toml
web = ["cranpose/web", "dep:wasm-bindgen", "dep:wasm-bindgen-futures"]
```

Add the browser entry point at the end of `src/lib.rs`:

```rust
#[cfg(all(feature = "web", target_arch = "wasm32"))]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn run_app() -> Result<(), wasm_bindgen::JsValue> {
    cranpose::AppLauncher::new().run_web("app", Counter).await
}
```

Create `index.html` in `my-app`:

```html
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>My app</title>
<style>
    html, body { margin: 0; width: 100%; height: 100%; }
    canvas { display: block; width: 100%; height: 100%; }
</style>
<canvas id="app"></canvas>
<script type="module">
    import init, { run_app } from "./pkg/my_app.js";
    await init();
    await run_app();
</script>
</html>
```

Build the library and serve the package directory:

```sh
wasm-pack build --target web --out-dir pkg --no-default-features --features web,renderer-wgpu
python3 -m http.server 8000
```

Open `http://localhost:8000` in a browser with WebGPU support. `wasm-pack` creates
the WebAssembly module and JavaScript loader in `pkg`. The canvas ID must match
the first argument to `run_web`.

## State and effects

### Give state an owner

Keep shared state in the closest common parent. Pass the current value and an action callback to each child:

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

- `remember(|| value)` retains an ordinary value. `.with(...)` borrows the stored value.
- `rememberMutableStateOf` retains observable state. A write schedules recomposition for readers of the state.
- `rememberKeyed` recreates a value when its key changes.
- `key` gives a group a stable identity across moves.

Cranpose discards remembered state when the composable leaves composition.
Save persistent state in a data store or preferences.

### Load data when an input changes

`LaunchedEffectAsync` starts an async task when the composable enters composition.
A key change cancels the previous task and starts a new request:

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

Add an HTTP backend from your package directory. Cargo's target dependency sections
select the native backend for desktop and mobile, and the browser backend for WebAssembly:

```sh
cargo add cranpose-services --no-default-features --features http-native --target 'cfg(not(target_arch = "wasm32"))'
cargo add cranpose-services --no-default-features --features web-http --target 'cfg(target_arch = "wasm32")'
```

These commands write `[target.'cfg(...)'.dependencies]` sections in `Cargo.toml`.
`http-native` supplies the native HTTP client. `web-http` uses browser requests.
The target condition follows the compiled app, so a web build on macOS selects `web-http`.
The launcher provides the selected backend through `local_http_client()`.

For Android, add this permission directly inside `<manifest>` in
`android/app/src/main/AndroidManifest.xml`:

```xml
<uses-permission android:name="android.permission.INTERNET" />
```

Use HTTPS URLs. For browser requests to another origin, configure the server's
`Access-Control-Allow-Origin` response header for your app's origin.

The URL supplies the key and request input. Each role owns a copy.
A key change or composition exit cancels the task.
The async body runs on the UI runtime and yields while I/O completes.

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

Add `cranpose-core` for composition-local types:

```sh
cargo add cranpose-core
```

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

### Layout composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

#### Column

Place children from top to bottom. Set the gap through `ColumnSpec`.

```rust
Column(
    Modifier::empty().padding(16.0),
    ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
    || {
        Text("Name", Modifier::empty(), TextStyle::default());
        Text("Ada", Modifier::empty(), TextStyle::default());
    },
);
```

#### Row

Place children from start to end. `weight` divides the available width.

```rust
Row(
    Modifier::empty().fill_max_width(),
    RowSpec::default(),
    || {
        Text(
            "Subtotal",
            Modifier::empty().weight(1.0),
            TextStyle::default(),
        );
        Text("24.00", Modifier::empty(), TextStyle::default());
    },
);
```

#### Box

Place children in layers. Later children appear above earlier children.

```rust
Box(
    Modifier::empty().size_points(120.0, 80.0),
    BoxSpec::default(),
    || {
        Spacer(Modifier::empty().fill_max_size().background(Color::BLUE));
        Text(
            "Photo",
            Modifier::empty().align(Alignment::CENTER),
            TextStyle::default(),
        );
    },
);
```

#### Spacer

Reserve space between controls. A weighted spacer absorbs spare space in a row or column.

```rust
Row(
    Modifier::empty().fill_max_width(),
    RowSpec::default(),
    || {
        Text("Title", Modifier::empty(), TextStyle::default());
        Spacer(Modifier::empty().weight(1.0));
        Text("Details", Modifier::empty(), TextStyle::default());
    },
);
```

#### FlowRow

Wrap children to the next row when the available width fills.

```rust
FlowRow(
    Modifier::empty().fill_max_width(),
    FlowRowSpec::default(),
    || {
        for label in ["Rust", "Android", "Desktop", "Web"] {
            Text(label, Modifier::empty().padding(8.0), TextStyle::default());
        }
    },
);
```

#### ForEach

Compose a small collection with each value as the identity. Use unique values.

```rust
Column(Modifier::empty(), ColumnSpec::default(), || {
    ForEach(&["Home", "Library", "Account"], |label| {
        Text(*label, Modifier::empty(), TextStyle::default());
    });
});
```

#### LazyColumn

Compose visible items in a vertical viewport. Retain the scroll state in composition.

```rust
let state = rememberLazyListState();
LazyColumn(
    Modifier::empty().height(240.0),
    state,
    LazyColumnSpec::default(),
    |list| {
        list.items(LazyItems::new(100).key(|index| index as u64), |index| {
            Text(
                format!("Item {index}"),
                Modifier::empty().padding(12.0),
                TextStyle::default(),
            );
        });
    },
);
```

#### LazyRow

Compose visible items in a horizontal viewport. Retain the scroll state in composition.

```rust
let state = rememberLazyListState();
LazyRow(
    Modifier::empty().fill_max_width().height(80.0),
    state,
    LazyRowSpec::default(),
    |list| {
        list.items(LazyItems::new(100).key(|index| index as u64), |index| {
            Text(
                format!("Item {index}"),
                Modifier::empty().padding(12.0),
                TextStyle::default(),
            );
        });
    },
);
```

#### BoxWithConstraints

Choose content from the available width during layout.

```rust
BoxWithConstraints(Modifier::empty().fill_max_width(), |scope| {
    let title = if scope.max_width() >= Dp(600.0) {
        "Library and details"
    } else {
        "Library"
    };
    Text(title, Modifier::empty(), TextStyle::default());
});
```

#### Scaffold

Reserve space for top and bottom bars. Apply the supplied insets to the content.

```rust
Scaffold(
    Modifier::empty().fill_max_size(),
    || {
        Text(
            "Library",
            Modifier::empty().padding(16.0),
            TextStyle::default(),
        );
    },
    || {
        Text(
            "3 books",
            Modifier::empty().padding(16.0),
            TextStyle::default(),
        );
    },
    |insets| {
        Text(
            "Your books",
            insets.apply_to(Modifier::empty()),
            TextStyle::default(),
        );
    },
);
```

#### ScaffoldWith

Add a floating action and configure scaffold colors and insets through `ScaffoldSpec`.

```rust
let count = rememberMutableStateOf(|| 0);
ScaffoldWith(
    Modifier::empty().fill_max_size(),
    ScaffoldSpec::default(),
    || {},
    || {},
    move || {
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || count.update(|n| *n += 1),
            || {
                Text("Add", Modifier::empty(), TextStyle::default());
            },
        );
    },
    move |insets| {
        Text(
            format!("Items: {}", count.get()),
            insets.apply_to(Modifier::empty()),
            TextStyle::default(),
        );
    },
);
```

#### VerticalScrollbar

Share one `ScrollState` between the content and scrollbar.

```rust
let state = remember(|| ScrollState::new(0.0)).with(|state| *state);
Box(
    Modifier::empty().size_points(240.0, 200.0),
    BoxSpec::default(),
    move || {
        Text(
            "A long document",
            Modifier::empty()
                .vertical_scroll(state, false)
                .size_points(600.0, 600.0),
            TextStyle::default(),
        );
        VerticalScrollbar(
            Modifier::empty().align(Alignment::TOP_END).height(200.0),
            state,
        );
    },
);
```

#### HorizontalScrollbar

Share one `ScrollState` between the content and scrollbar.

```rust
let state = remember(|| ScrollState::new(0.0)).with(|state| *state);
Box(
    Modifier::empty().size_points(240.0, 200.0),
    BoxSpec::default(),
    move || {
        Text(
            "A long document",
            Modifier::empty()
                .horizontal_scroll(state, false)
                .size_points(600.0, 600.0),
            TextStyle::default(),
        );
        HorizontalScrollbar(
            Modifier::empty()
                .align(Alignment::BOTTOM_START)
                .width(240.0),
            state,
        );
    },
);
```

#### Scrollbar

Set the axis and appearance explicitly for a custom scroll surface.

```rust
use cranpose_ui_layout::Axis;
let state = remember(|| ScrollState::new(0.0)).with(|state| *state);
Box(
    Modifier::empty().size_points(240.0, 200.0),
    BoxSpec::default(),
    move || {
        Text(
            "Document",
            Modifier::empty()
                .vertical_scroll(state, false)
                .height(600.0),
            TextStyle::default(),
        );
        Scrollbar(
            Modifier::empty().align(Alignment::TOP_END).height(200.0),
            state,
            Axis::Vertical,
            ScrollbarSpec::default(),
        );
    },
);
```

#### Layout

Supply a measure policy for a custom container. This example reuses the box policy.
Implement `cranpose_ui_layout::MeasurePolicy` for custom child sizes and positions.

```rust
use cranpose::layout::policies::BoxMeasurePolicy;

Layout(
    Modifier::empty().size_points(200.0, 100.0),
    BoxMeasurePolicy::new(Alignment::CENTER, false),
    || {
        Text("Custom container", Modifier::empty(), TextStyle::default());
    },
);
```

#### SubcomposeLayout

Compose children during measurement when content depends on constraints.
Each slot needs a stable `SlotId`. Measure each child, then return the container size and placements.

```rust
use cranpose::{SubcomposeLayoutScope, SubcomposeMeasureScope};
use cranpose_core::SlotId;

SubcomposeLayout(Modifier::empty(), |scope, constraints| {
    let children = scope.subcompose(SlotId::new(0), constraints, || {
        Text("Measured content", Modifier::empty(), TextStyle::default());
    });
    let mut size = Size::new(constraints.min_width, constraints.min_height);
    let mut placements = Vec::with_capacity(children.len());
    for child in children {
        let measured = scope.measure(child, constraints);
        size.width = size.width.max(measured.width());
        size.height = size.height.max(measured.height());
        measured.place(0.0, 0.0);
        placements.push(Placement::new(measured.node_id(), 0.0, 0.0, 0));
    }
    scope.layout(size.width, size.height, placements)
});
```

For a custom measure policy, add the layout types with `cargo add cranpose-ui-layout`.

### Wear composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.
Also import `cranpose::widgets::wear::*` and `cranpose::round_scaling_list::CentreAnchor`.

#### ScreenScaffold

Provide a watch screen with a curved scroll indicator. Share the list state with the scaffold.

```rust
let state = rememberWearScalingListState(CentreAnchor::default());
ScreenScaffold(
    Modifier::empty().fill_max_size(),
    state,
    ScreenScaffoldSpec::default(),
    move || {
        WearScalingLazyColumn(
            Modifier::empty().fill_max_size(),
            state,
            WearScalingLazyColumnSpec::default(),
            |list| {
                list.items(20, |index| {
                    ListHeader(
                        Modifier::empty(),
                        ListHeaderSpec::default(),
                        format!("Item {index}"),
                    );
                });
            },
        );
    },
);
```

#### WearScalingLazyColumn

Scale and fade rows near a round screen’s edge. Use stable keys for rows with state.

```rust
let state = rememberWearScalingListState(CentreAnchor::default());
WearScalingLazyColumn(
    Modifier::empty().fill_max_size(),
    state,
    WearScalingLazyColumnSpec::default(),
    |list| {
        list.items(LazyItems::new(20).key(|index| index as u64), |index| {
            Text(
                format!("Track {index}"),
                Modifier::empty().height(52.0),
                TextStyle::default(),
            );
        });
    },
);
```

#### WearScalingLazyColumnNode

Use the lower-level entry point when a wrapper receives prepared `WearScalingListContent`.

```rust
#[composable]
fn WatchList(content: WearScalingListContent) {
    let state = rememberWearScalingListState(CentreAnchor::default());
    WearScalingLazyColumnNode(
        Modifier::empty().fill_max_size(),
        state,
        WearScalingLazyColumnSpec::default(),
        content,
    );
}
```

#### WearScalingItem

Apply an explicit scale and alpha to a custom watch row. `WearScalingLazyColumn` applies this transform to rows automatically.

```rust
let transform = remember(WearItemTransform::new).with(Clone::clone);
WearScalingItem(
    Modifier::empty(),
    transform,
    CompositingStrategy::Auto,
    || {
        Text("Track title", Modifier::empty(), TextStyle::default());
    },
);
```

#### ScrollIndicator

Place the curved indicator over a custom watch layout. Pass the same state as the visible list.

```rust
#[composable]
fn WatchIndicator(state: WearScalingListState) {
    ScrollIndicator(
        Modifier::empty().fill_max_size(),
        state,
        ScrollIndicatorSpec::default(),
    );
}
```

#### ListHeader

Add a section label with watch typography and spacing.

```rust
ListHeader(
    Modifier::empty().fill_max_width(),
    ListHeaderSpec::default(),
    "Playback",
);
```

#### WearButton

Show a capsule button with primary and secondary labels.

```rust
let plays = rememberMutableStateOf(|| 0);
WearButton(
    Modifier::empty().fill_max_width(),
    WearButtonSpec::default(),
    "Play".into(),
    Some("Selected album".into()),
    move || plays.update(|n| *n += 1),
);
```

#### SwitchButton

Show a labeled toggle. Store the new checked value in the callback.

```rust
let enabled = rememberMutableStateOf(|| true);
let spec = SwitchButtonSpec::default().progress(if enabled.get() { 1.0 } else { 0.0 });
SwitchButton(
    Modifier::empty().fill_max_width(),
    spec,
    enabled.get(),
    "Shuffle".into(),
    None,
    move |checked| enabled.set(checked),
);
```

#### SwitchButtonNode

Use an action callback when the caller computes the next state. Set the thumb progress from the checked state.

```rust
let enabled = rememberMutableStateOf(|| true);
let spec = SwitchButtonSpec::default().progress(if enabled.get() { 1.0 } else { 0.0 });
SwitchButtonNode(
    Modifier::empty().fill_max_width(),
    spec,
    enabled.get(),
    "Shuffle".into(),
    None,
    move || enabled.update(|value| *value = !*value),
);
```

#### SwitchGraphic

Draw the switch inside a custom interactive row. The parent supplies input and semantics.

```rust
let spec = SwitchButtonSpec::default().progress(1.0);
SwitchGraphic(Modifier::empty(), spec, SwitchColors::of(spec.colors, true));
```

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

`BasicTextFieldWithOptions` sets line limits, cursor color and keyboard visibility on focus.
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
See the [accessibility guide](accessibility.md).

### Text composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

#### Text

Show plain text or an `AnnotatedString` with a `TextStyle`.

```rust
Text("Your library", Modifier::empty(), TextStyle::default());
```

#### TextWithOptions

Limit a label to one line and show an ellipsis at the end.

```rust
use cranpose::text::{TextOptions, TextOverflow};
TextWithOptions(
    "A long article title",
    Modifier::empty().width(160.0),
    TextStyle::default(),
    TextOptions {
        max_lines: Some(1),
        overflow: TextOverflow::Ellipsis,
        ..Default::default()
    },
);
```

#### BasicText

Set overflow, line wrap, maximum lines and minimum lines directly.

```rust
use cranpose::text::TextOverflow;
BasicText(
    "Article summary",
    Modifier::empty().width(240.0),
    TextStyle::default(),
    TextOverflow::Ellipsis,
    true,
    3,
    1,
);
```

#### BasicTextWithOptions

Pass reusable layout options to a text component.

```rust
use cranpose::text::{TextLayoutOptions, TextOverflow};
let options = TextLayoutOptions {
    max_lines: 2,
    overflow: TextOverflow::Ellipsis,
    ..Default::default()
};
BasicTextWithOptions(
    "Article summary",
    Modifier::empty().width(240.0),
    TextStyle::default(),
    options,
);
```

#### ClickableText

Receive the character offset at a text tap.

```rust
use cranpose::text::AnnotatedString;
let offset = rememberMutableStateOf(|| 0_usize);
ClickableText(
    AnnotatedString::from("Select a character"),
    Modifier::empty(),
    TextStyle::default(),
    move |position| offset.set(position),
);
Text(
    format!("Offset: {}", offset.get()),
    Modifier::empty(),
    TextStyle::default(),
);
```

#### LinkedText

Attach a URL to a text range and open the URL through the platform handler.

```rust
use cranpose::text::{AnnotatedString, LinkAnnotation};
let uri = local_uri_handler().current();
let error = rememberMutableStateOf(|| None::<String>);
let text = AnnotatedString::builder()
    .with_link(
        LinkAnnotation::Url("https://example.com/help".into()),
        |builder| builder.append("Help"),
    )
    .to_annotated_string();
LinkedText(text, Modifier::empty(), TextStyle::default(), move |url| {
    error.set(uri.open_uri(url).err().map(|failure| failure.to_string()));
});
if let Some(message) = error.get() {
    Text(message, Modifier::empty(), TextStyle::default());
}
```

#### BasicTextField

Keep editable text in a remembered `TextFieldState`.

```rust
let name = remember(|| TextFieldState::new("Ada")).with(|state| *state);
BasicTextField(
    name,
    Modifier::empty().width(240.0).content_description("Name"),
    TextStyle::default(),
);
```

#### BasicTextFieldWithOptions

Set single-line input and a custom cursor color.

```rust
let query = remember(|| TextFieldState::new("")).with(|state| *state);
BasicTextFieldWithOptions(
    query,
    Modifier::empty().width(240.0).content_description("Search"),
    BasicTextFieldOptions {
        line_limits: TextFieldLineLimits::SingleLine,
        cursor_color: Color::BLUE,
        ..Default::default()
    },
);
```

#### BasicTextFieldDecorated

Add a label, background or border around the input. Call `inner_text_field()` once in the decoration.

```rust
let name = remember(|| TextFieldState::new("")).with(|state| *state);
BasicTextFieldDecorated(
    name,
    Modifier::empty().width(240.0),
    BasicTextFieldOptions::default(),
    |field| {
        Column(
            Modifier::empty().padding(12.0),
            ColumnSpec::default(),
            move || {
                Text("Name", Modifier::empty(), TextStyle::default());
                field.inner_text_field();
            },
        );
    },
);
```

#### SelectionContainer

Allow selection across several text children.

```rust
SelectionContainer(Modifier::empty(), || {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text("First paragraph.", Modifier::empty(), TextStyle::default());
        Text("Second paragraph.", Modifier::empty(), TextStyle::default());
    });
});
```

#### DisableSelection

Exclude a child from selection in the parent container.

```rust
SelectionContainer(Modifier::empty(), || {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "Select this paragraph.",
            Modifier::empty(),
            TextStyle::default(),
        );
        DisableSelection(|| {
            Text("Page 1", Modifier::empty(), TextStyle::default());
        });
    });
});
```

### Action composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

#### Button

Connect an action to a text or custom content slot.

```rust
let saved = rememberMutableStateOf(|| false);
Button(
    Modifier::empty(),
    ButtonSpec::default(),
    move || saved.set(true),
    || {
        Text("Save", Modifier::empty(), TextStyle::default());
    },
);
Text(
    if saved.get() { "Saved" } else { "Draft" },
    Modifier::empty(),
    TextStyle::default(),
);
```

#### IconButton

Give an icon action an accessible name. The child icon can use `None` for its description.

```rust
let saved = rememberMutableStateOf(|| false);
IconButton(
    Modifier::empty(),
    "Save",
    move || saved.set(true),
    || {
        Icon(cranpose::liquid::icons::CHECK, None, 24.0, Color::BLACK);
    },
);
```

#### IconButtonWith

Supply an interaction source to observe the button’s press state.

```rust
let source = rememberMutableInteractionSource();
let saved = rememberMutableStateOf(|| false);
IconButtonWith(
    Modifier::empty(),
    "Save",
    IconButtonSpec::default(),
    Some(source),
    move || saved.set(true),
    || {
        Icon(cranpose::liquid::icons::CHECK, None, 24.0, Color::BLACK);
    },
);
```

#### Slider

Use a value from `0.0` to `1.0`. Supply the track and thumb in the content slot.

```rust
let volume = rememberMutableStateOf(|| 0.5_f32);
Slider(
    Modifier::empty()
        .width(240.0)
        .height(32.0)
        .content_description("Volume"),
    volume.get(),
    move |value| volume.set(value),
    || {},
    SliderSpec::default().thumb_extent(20.0),
    |scope| {
        Spacer(
            Modifier::empty()
                .fill_max_width()
                .height(4.0)
                .align(Alignment::CENTER)
                .background(Color::from_rgb_u8(160, 160, 160)),
        );
        Spacer(
            Modifier::empty()
                .offset(scope.thumb_offset(), 6.0)
                .size_points(20.0, 20.0)
                .background(Color::BLUE),
        );
    },
);
```

#### SwipeToDismiss

Remove a row after the dismiss gesture completes. `SwipeToDismissSpec` controls the gesture.

```rust
let visible = rememberMutableStateOf(|| true);
if visible.get() {
    SwipeToDismiss(
        Modifier::empty().fill_max_width(),
        SwipeToDismissSpec::default(),
        move || visible.set(false),
        || {
            Text(
                "Swipe to remove",
                Modifier::empty().padding(16.0),
                TextStyle::default(),
            );
        },
    );
}
```

#### SwipeToDismissBox

Remove a row after the dismiss gesture completes. This variant uses the default gesture spec.

```rust
let visible = rememberMutableStateOf(|| true);
if visible.get() {
    SwipeToDismissBox(
        Modifier::empty().fill_max_width(),
        move || visible.set(false),
        || {
            Text(
                "Swipe to remove",
                Modifier::empty().padding(16.0),
                TextStyle::default(),
            );
        },
    );
}
```

### Popup and dialog composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

#### PopupHost

Place one host above the screen content. Popups and dialogs use this host for their overlay layer.

```rust
PopupHost(|| {
    Text("Screen content", Modifier::empty(), TextStyle::default());
});
```

#### Popup

Position content from an anchor rectangle in window coordinates.

```rust
PopupHost(|| {
    Popup(
        Rect {
            x: 20.0,
            y: 20.0,
            width: 80.0,
            height: 32.0,
        },
        Point::new(0.0, 4.0),
        || {
            Text(
                "Help text",
                Modifier::empty().background(Color::WHITE).padding(8.0),
                TextStyle::default(),
            );
        },
    );
});
```

#### PopupAnchored

Measure the anchor from a content slot. Set `expanded` from app state.

```rust
let expanded = rememberMutableStateOf(|| false);
PopupHost(move || {
    PopupAnchored(
        Modifier::empty(),
        expanded.get(),
        Point::new(0.0, 4.0),
        move || {
            Button(
                Modifier::empty(),
                ButtonSpec::default(),
                move || expanded.update(|v| *v = !*v),
                || {
                    Text("Help", Modifier::empty(), TextStyle::default());
                },
            );
        },
        || {
            Text(
                "Choose a book to open.",
                Modifier::empty().background(Color::WHITE).padding(8.0),
                TextStyle::default(),
            );
        },
    );
});
```

#### PopupDismissable

Close an open popup after an outside tap.

```rust
let open = rememberMutableStateOf(|| true);
PopupHost(move || {
    if open.get() {
        PopupDismissable(
            Rect {
                x: 20.0,
                y: 20.0,
                width: 80.0,
                height: 32.0,
            },
            Point::new(0.0, 4.0),
            move || open.set(false),
            || {
                Text(
                    "Tap outside to close",
                    Modifier::empty().background(Color::WHITE).padding(8.0),
                    TextStyle::default(),
                );
            },
        );
    }
});
```

#### PopupDismissableWhen

Close an open popup after an outside tap. The first argument controls outside-tap dismissal.

```rust
let open = rememberMutableStateOf(|| true);
PopupHost(move || {
    if open.get() {
        PopupDismissableWhen(
            true,
            Rect {
                x: 20.0,
                y: 20.0,
                width: 80.0,
                height: 32.0,
            },
            Point::new(0.0, 4.0),
            move || open.set(false),
            || {
                Text(
                    "Tap outside to close",
                    Modifier::empty().background(Color::WHITE).padding(8.0),
                    TextStyle::default(),
                );
            },
        );
    }
});
```

#### Dialog

Show centered content above a scrim and handle dismissal.

```rust
let open = rememberMutableStateOf(|| true);
PopupHost(move || {
    if open.get() {
        Dialog(
            DialogSpec::default(),
            move |_| open.set(false),
            move || {
                Button(
                    Modifier::empty(),
                    ButtonSpec::default(),
                    move || open.set(false),
                    || {
                        Text("Close dialog", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    }
});
```

#### DialogWithScrim

Show centered content above a scrim and handle dismissal. Pass the scrim color explicitly.

```rust
let open = rememberMutableStateOf(|| true);
PopupHost(move || {
    if open.get() {
        DialogWithScrim(
            DialogSpec::default(),
            Color::BLACK.with_alpha(0.6),
            move |_| open.set(false),
            move || {
                Button(
                    Modifier::empty(),
                    ButtonSpec::default(),
                    move || open.set(false),
                    || {
                        Text("Close dialog", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
    }
});
```

### Custom text selection composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

`BasicTextField` and `SelectionContainer` supply their own selection controls.
Use these components for a custom editor. Positions use window coordinates;
the editor supplies hit tests, selection ranges and clipboard actions.

#### SelectionLoupe

Show a magnifier above the active text line. Pass `None` after the drag ends.

```rust
SelectionLoupe(Some(LoupeTarget {
    focus_x: 120.0,
    line_mid_y: 80.0,
}));
```

#### SelectionHandle

Receive drag positions for a cursor or selection endpoint.

```rust
use cranpose::text_selection::HandleKind;
let tip = rememberMutableStateOf(|| Point::new(120.0, 80.0));
SelectionHandle(
    HandleKind::Cursor,
    tip.get(),
    20.0,
    6.0,
    Color::BLUE,
    move |point| tip.set(point),
    || {},
    || {},
    || {},
);
```

#### LiquidTextMenu

Define editor-specific menu actions above the text line.

```rust
let marked = rememberMutableStateOf(|| false);
LiquidTextMenu(
    MenuAnchor {
        center_x: 120.0,
        line_top: 80.0,
        line_bottom: 100.0,
    },
    true,
    None,
    vec![TextMenuItem::new("Mark", move || marked.set(true))],
);
```

#### TextSelectionMenu

Connect the standard selection actions to the editor. The editor owns clipboard access and the selected range.

```rust
#[composable]
fn EditorSelectionMenu(
    anchor: MenuAnchor,
    copy: impl Fn() + 'static,
    cut: impl Fn() + 'static,
    paste: impl Fn() + 'static,
    select_all: impl Fn() + 'static,
) {
    TextSelectionMenu(anchor, true, None, true, copy, cut, paste, select_all);
}
```

#### CaretActionMenu

Expose paste, select-all, undo and redo for a collapsed selection.

```rust
#[composable]
fn EditorCaretMenu(
    anchor: MenuAnchor,
    paste: impl Fn() + 'static,
    select_all: impl Fn() + 'static,
    undo: impl Fn() + 'static,
    redo: impl Fn() + 'static,
) {
    CaretActionMenu(
        anchor, true, true, true, true, paste, select_all, undo, redo,
    );
}
```

## Animation and graphics

### Animate a value

Add the animation crate:

```sh
cargo add cranpose-animation
```

This tween changes opacity over 180 milliseconds:

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
Specs set their appearance. The [showcase widgets](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/src/widgets) combine these controls into app components.

### Graphics composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

#### Canvas

Draw in local coordinates. Give the canvas an explicit size.

```rust
Canvas(Modifier::empty().size_points(48.0, 48.0), |draw| {
    draw.draw_circle(Brush::solid(Color::BLUE), Point::new(24.0, 24.0), 16.0);
});
```

#### Image

Draw a decoded bitmap. Keep the bitmap in app state or a cache and pass the handle to the composable.

```rust
#[composable]
fn Photo(bitmap: ImageBitmap) {
    Image(
        BitmapPainter(bitmap),
        Some("Mountain lake".into()),
        Modifier::empty().size_points(240.0, 160.0),
        Alignment::CENTER,
        ContentScale::Crop,
        1.0,
        None,
    );
}
```

Choose `BitmapRegionPainter` for a sprite, `TiledPainter` for a repeated texture,
or `NinePatchPainter` for a resizable frame. Each painter fits the first `Image` argument.

#### Icon

Draw an SVG path from a 24-point view box. Supply a description for a meaningful image.

```rust
Icon(
    cranpose::liquid::icons::CHECK,
    Some("Saved".into()),
    24.0,
    Color::BLUE,
);
```

#### IconWith

Set the icon modifier and `IconSpec` explicitly.

```rust
IconWith(
    Modifier::empty().size_points(32.0, 32.0),
    cranpose::liquid::icons::CHECK,
    IconSpec::default(),
    Some("Saved".into()),
);
```

#### AnimatedVisibility

Animate content entry and exit. The content leaves composition after the exit transition.

```rust
#[composable]
fn SavedMessage(visible: bool) {
    AnimatedVisibility(visible, fade_in(), fade_out(), || {
        Text("Saved", Modifier::empty(), TextStyle::default());
    });
}
```

#### Crossfade

Blend between two states. Add `cranpose-animation` for the animation spec.

```rust
use cranpose_animation::{Easing, tween};
#[composable]
fn SaveStatus(saved: bool) {
    Crossfade(saved, tween(180, Easing::EaseOut), |value| {
        Text(
            if value { "Saved" } else { "Draft" },
            Modifier::empty(),
            TextStyle::default(),
        );
    });
}
```

#### CircularProgressIndicator

Show indeterminate progress in a bounded circle.

```rust
CircularProgressIndicator(
    Modifier::empty()
        .size_points(32.0, 32.0)
        .content_description("Request in progress"),
    Color::BLUE,
    3.0,
);
```

#### LinearProgressIndicator

Show indeterminate progress across a horizontal track.

```rust
LinearProgressIndicator(
    Modifier::empty()
        .fill_max_width()
        .height(4.0)
        .content_description("Request in progress"),
    Color::BLUE,
);
```

### Liquid composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.
Also import `cranpose::liquid::prelude::*` and place the controls under `LiquidTheme`.

#### LiquidTheme

Provide colors and typography to descendant controls.

```rust
LiquidTheme(LiquidThemeSpec::default(), || {
    Text("Library", Modifier::empty(), liquid_typography().title3);
});
```

#### GlassButton

Use a glass or prominent button. Use the same button style for the label.

```rust
let saved = rememberMutableStateOf(|| false);
GlassButton(
    Modifier::empty(),
    GlassButtonSpec::prominent(),
    move || saved.set(true),
    || {
        GlassButtonLabel("Save", GlassButtonSpec::prominent());
    },
);
```

#### GlassButtonLabel

Use the button’s text size and foreground color in custom button content.

```rust
let spec = GlassButtonSpec::glass();
GlassButtonLabel("Save", spec);
```

#### GlassIconButton

Create a circular action from an icon path.

```rust
let saved = rememberMutableStateOf(|| false);
GlassIconButton(
    Modifier::empty().content_description("Save"),
    GlassButtonSpec::glass(),
    44.0,
    move || saved.set(true),
    icons::CHECK,
);
```

#### GlassIconButtonGroup

Group adjacent circular actions in a shared glass surface.

```rust
let action = rememberMutableStateOf(|| "Ready");
GlassIconButtonGroup(
    Modifier::empty(),
    GlassIconButtonGroupSpec::new(44.0),
    move |group| {
        group.action(icons::CHECK, "Save", move || action.set("Saved"));
        group.action(icons::SEARCH, "Search", move || action.set("Search"));
    },
);
```

#### Surface

Use the theme’s background surface.

```rust
Surface(Modifier::empty().padding(16.0), || {
    Text("Account", Modifier::empty(), liquid_typography().body);
});
```

#### Card

Group content in a rounded card.

```rust
Card(Modifier::empty().padding(16.0), || {
    Text("Account", Modifier::empty(), liquid_typography().body);
});
```

#### LiquidCard

Group content in a rounded card through the explicit Liquid API.

```rust
LiquidCard(Modifier::empty().padding(16.0), || {
    Text("Account", Modifier::empty(), liquid_typography().body);
});
```

#### GlassSurface

Apply a glass material to a container. Content behind the surface supplies the backdrop.

```rust
GlassSurface(
    Modifier::empty().size_points(240.0, 80.0),
    Glass::default(),
    || {
        Text("Now playing", Modifier::empty(), liquid_typography().body);
    },
);
```

#### LiquidListSection

Group related rows under a section title.

```rust
LiquidListSection(Modifier::empty(), "Account", || {
    Text("Ada", Modifier::empty(), liquid_typography().body);
});
```

#### LiquidListRow

Add an action row with a Liquid row spec.

```rust
let selected = rememberMutableStateOf(|| false);
LiquidListRow(
    Modifier::empty(),
    LiquidListRowSpec::default(),
    move || selected.set(true),
    || {
        Text("Profile", Modifier::empty(), liquid_typography().body);
    },
);
```

#### LiquidChip

Show a selectable filter.

```rust
let selected = rememberMutableStateOf(|| false);
LiquidChip(
    Modifier::empty(),
    selected.get(),
    move || selected.update(|value| *value = !*value),
    "Favorites",
);
```

#### LiquidActionChip

Show a compact action. The second argument selects the prominent style.

```rust
let count = rememberMutableStateOf(|| 0);
LiquidActionChip(
    Modifier::empty(),
    true,
    move || count.update(|value| *value += 1),
    "Add",
);
```

#### LiquidToggle

Show a switch with caller-owned checked state.

```rust
let enabled = rememberMutableStateOf(|| true);
LiquidToggle(
    Modifier::empty().content_description("Notifications"),
    enabled.get(),
    move |value| enabled.set(value),
);
```

#### LiquidSlider

Show a styled slider with a value in `0.0..=1.0`.

```rust
let volume = rememberMutableStateOf(|| 0.5_f32);
LiquidSlider(
    Modifier::empty().width(240.0).content_description("Volume"),
    volume.get(),
    move |value| volume.set(value),
);
```

#### LiquidSegmentedControl

Select one segment by index.

```rust
let selected = rememberMutableStateOf(|| 0_usize);
LiquidSegmentedControl(
    Modifier::empty().width(280.0),
    selected.get(),
    move |index| selected.set(index),
    |segments| {
        segments.segment("All");
        segments.segment("Favorites");
    },
);
```

#### SearchField

Use the same search component as `SearchBar` under the `SearchField` name.

```rust
let query = remember(|| TextFieldState::new("")).with(|state| *state);
SearchField(Modifier::empty().fill_max_width(), query, "Search books");
```

#### SearchBar

Use a themed search input. `SearchField` calls the same implementation.

```rust
let query = remember(|| TextFieldState::new("")).with(|state| *state);
SearchBar(Modifier::empty().fill_max_width(), query, "Search books");
```

#### LiquidSearchField

Set the placeholder, foreground and glass options explicitly.

```rust
let query = remember(|| TextFieldState::new("")).with(|state| *state);
LiquidSearchField(
    Modifier::empty().fill_max_width(),
    query,
    LiquidSearchFieldSpec {
        placeholder: "Search books".into(),
        on_glass: true,
        ..Default::default()
    },
);
```

#### LiquidNavBar

Collapse the title as the content scrolls. Share the `ScrollState` with the content.

```rust
let scroll = remember(|| ScrollState::new(0.0)).with(|state| *state);
Column(
    Modifier::empty().fill_max_size(),
    ColumnSpec::default(),
    move || {
        LiquidNavBar(
            Modifier::empty(),
            LiquidNavBarSpec::new("Library"),
            scroll,
            || {},
            || {},
        );
        Column(
            Modifier::empty().weight(1.0).vertical_scroll(scroll, false),
            ColumnSpec::default(),
            || {
                for index in 0..30 {
                    Text(
                        format!("Book {index}"),
                        Modifier::empty().padding(12.0),
                        liquid_typography().body,
                    );
                }
            },
        );
    },
);
```

#### LiquidTabBar

Select a destination by index. Render the selected screen from the same state.

```rust
let selected = rememberMutableStateOf(|| 0_usize);
LiquidTabBar(
    Modifier::empty(),
    LiquidTabBarSpec::default(),
    selected.get(),
    move |index| selected.set(index),
    |tabs| {
        tabs.tab(icons::SEARCH, "Explore");
        tabs.tab(icons::CHECK, "Saved");
    },
);
```

#### LiquidTabBarWithAccessory

Place a separate action beside the tabs.

```rust
let selected = rememberMutableStateOf(|| 0_usize);
let search = rememberMutableStateOf(|| false);
LiquidTabBarWithAccessory(
    Modifier::empty(),
    LiquidTabBarSpec::default(),
    selected.get(),
    move |index| selected.set(index),
    |tabs| {
        tabs.tab(icons::CHECK, "Saved");
        tabs.tab(icons::SEARCH, "Explore");
    },
    move || {
        LiquidTabBarSearchAccessory(move || search.set(true));
    },
);
```

#### LiquidTabBarSearchAccessory

Use the tab bar’s circular search action in an accessory slot.

```rust
let search = rememberMutableStateOf(|| false);
LiquidTabBarSearchAccessory(move || search.set(true));
```

#### LiquidDropdownMenu

Anchor a menu to a composable. Place the menu under `PopupHost`.

```rust
let expanded = rememberMutableStateOf(|| false);
let saved = rememberMutableStateOf(|| false);
PopupHost(move || {
    LiquidDropdownMenu(
        Modifier::empty(),
        expanded.get(),
        LiquidDropdownMenuSpec::default(),
        move || expanded.set(false),
        move || {
            GlassButton(
                Modifier::empty(),
                GlassButtonSpec::glass(),
                move || expanded.set(true),
                || {
                    GlassButtonLabel("Actions", GlassButtonSpec::glass());
                },
            );
        },
        move |menu| {
            menu.item(LiquidMenuItem::new("Save"), move || saved.set(true));
        },
    );
});
```

#### LiquidMenu

Position a menu from a measured window rectangle. The gesture handle connects press-and-drag input from the trigger.

```rust
let expanded = rememberMutableStateOf(|| true);
let saved = rememberMutableStateOf(|| false);
PopupHost(move || {
    let gesture = rememberLiquidMenuGesture();
    LiquidMenu(
        expanded.get(),
        Rect {
            x: 20.0,
            y: 20.0,
            width: 44.0,
            height: 44.0,
        },
        LiquidMenuSpec::new(220.0),
        Vec::new(),
        gesture,
        move || expanded.set(false),
        move |menu| {
            menu.item(LiquidMenuItem::new("Save").icon(icons::CHECK), move || {
                saved.set(true)
            });
        },
    );
});
```

#### LiquidMenuIconButton

Use a trigger with menu drag input. Pass the same gesture handle to `LiquidMenu`.

```rust
let expanded = rememberMutableStateOf(|| false);
let gesture = rememberLiquidMenuGesture();
LiquidMenuIconButton(
    Modifier::empty().content_description("Actions"),
    GlassButtonSpec::glass(),
    44.0,
    expanded.get(),
    gesture,
    move || expanded.set(true),
    icons::CHECK,
);
```

#### LiquidMenuAbsorbedIconButton

Hide the source icon while a menu displays the corresponding `LiquidMenuAbsorbedSource`.
Use the same rectangle, diameter, spec and icon in the menu’s absorbed-source list.

```rust
let expanded = rememberMutableStateOf(|| false);
LiquidMenuAbsorbedIconButton(
    Modifier::empty().content_description("Actions"),
    GlassButtonSpec::glass(),
    44.0,
    expanded.get(),
    move || expanded.set(true),
    icons::CHECK,
);
```

#### icons::Icon

Draw a Liquid icon with a theme color.

```rust
icons::Icon(
    icons::CHECK,
    Some("Saved".into()),
    24.0,
    liquid_colors().label,
);
```

## Navigation and data

### Define destinations

Add the navigation crate:

```sh
cargo add cranpose-navigation
```

Use an enum for routes and their arguments:

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

Add the flow and Compose adapter crates:

```sh
cargo add coroflow cranpose-coroflow
```

A collector exposes the flow's value as UI state:

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
`repository.progress()` returns a `StateFlow<u32>`. `Handle` gives the flow stable identity as a composable input.

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

The launcher installs platform services. Read the URI handler during composition and open a URL from a button callback:

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

Give each launcher a unique request key. Android uses the key to restore results after activity recreation:

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

Add the media backend for desktop and Android:

```sh
cargo add cranpose --features media
```

iOS and web use platform players. Open a media item and start playback:

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

Keep the Rust library beside the Android app module. The native host source lives
in `platforms/android` in the Cranpose repository. From your Android project root:

```sh
git clone https://github.com/samoylenkodmitry/Cranpose.git vendor/Cranpose
cargo new counter-ui --lib
cd counter-ui
cargo add cranpose --path ../vendor/Cranpose/crates/cranpose --features renderer-wgpu
cargo add cranpose-native --path ../vendor/Cranpose/crates/cranpose-native
cargo add uniffi --no-default-features
```

The path dependencies keep the Rust runtime and Kotlin host in the same checkout.
UniFFI generates Kotlin and Swift APIs from the exported Rust types and functions.
Keep Cargo's generated dependency entries. Add these sections to `counter-ui/Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib", "staticlib", "rlib"]

[features]
bindings = ["uniffi/cli"]

[[bin]]
name = "counter-bindgen"
path = "src/bin/bindgen.rs"
required-features = ["bindings"]
```

`cdylib` supplies the Android `.so`; `staticlib` supplies the iOS `.a`.
The `bindings` feature enables the code generator for host builds.
Replace `counter-ui/src/lib.rs` with a session factory:

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

Create `counter-ui/src/bin/bindgen.rs`:

```rust
fn main() {
    uniffi::uniffi_bindgen_main();
}
```

Create `counter-ui/uniffi.toml`. The shared-library name applies to the application
and runtime bindings:

```toml
[defaults.bindings.kotlin]
cdylib_name = "counter_ui"
```

From `counter-ui`, build the host library and generate Kotlin code. On macOS:

```sh
cargo build --features bindings --lib --bin counter-bindgen
target/debug/counter-bindgen generate target/debug/libcounter_ui.dylib --language kotlin --config uniffi.toml --out-dir generated/kotlin
```

On Linux, pass `target/debug/libcounter_ui.so` to the same generator.
The output contains `uniffi/counter_ui` for your factory and `uniffi/cranpose_native`
for `NativeSession` and events.

Build the Android library with the NDK and `cargo-ndk` from **Get started**:

```sh
cargo ndk --platform 24 -t arm64-v8a -t x86_64 -o generated/jniLibs build --lib
```

Add the host module to your Android `settings.gradle.kts`:

```kotlin
include(":cranpose")
project(":cranpose").projectDir = file("vendor/Cranpose/platforms/android")
```

Declare `com.android.library` in your root plugin configuration with the same AGP
version as `com.android.application`. Add the runtime source path to `gradle.properties`:

```properties
cranposeBindingsDir=counter-ui/generated/kotlin/uniffi/cranpose_native
```

Add the application bindings, native libraries and dependencies to `app/build.gradle.kts`:

```kotlin
android {
    sourceSets["main"].kotlin.srcDir("../counter-ui/generated/kotlin/uniffi/counter_ui")
    sourceSets["main"].jniLibs.srcDir("../counter-ui/generated/jniLibs")
}

dependencies {
    implementation(project(":cranpose"))
    implementation("net.java.dev.jna:jna:5.17.0@aar")
}
```

Build the Android app through Gradle after each Rust library build.
The [native demo build script](../scripts/native_demo.sh) combines these steps and
also generates Swift code.

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
                CranposeView(context, createCounter()).also { view -> component = view }
            },
            update = { view ->
                view.onEvent = { event ->
                    if (event.name == "count") {
                        event.value.toIntOrNull()?.let { value -> count = value }
                    }
                }
                view.onError = { message -> error = message }
            },
            onRelease = { view ->
                if (component === view) component = null
                view.close()
            },
        )
        error?.let { message -> Text(message) }
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

Keep the component in a controller property. Call `close()` when the controller releases the component.
For SwiftUI, use `UIViewRepresentable` and call `close()` from `dismantleUIView`.

## Native views

**Place a platform control in your Cranpose layout.**

Cranpose sets the child's bounds. The native view draws itself and handles input.

### Show a website

Add the native web view backend to a standalone app:

```sh
cargo add cranpose --features webview
```

Pass the URL, layout size and load-event callback to `WebView`:

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
with a `UISlider`. Register the factory as `volume` to reuse this Rust screen.
Custom Rust hosts use `cranpose::native_view::NativeViewHost` for layout and events.

### Size and position native children

Give each slot an explicit size. Keep its bounds axis-aligned.
Native children sit above Cranpose content. The host clips them to its bounds.
Apply transforms and effects within the native child.
Hide the native child while a Cranpose popup overlaps the child’s bounds.

The [native demo](../apps/native-demo/README.md) shows both directions:
host controls update Rust state; Rust positions a native website view.
