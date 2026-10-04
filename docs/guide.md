# Cranpose Guide

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

### Start with the project template

Install Rust with [rustup](https://rustup.rs/). Cargo creates packages, resolves
dependencies and runs the compiler. `Cargo.toml` serves the role of a Gradle build
file. A crate is a Rust library or executable.

The [Cranpose plugin for IntelliJ IDEA and RustRover](https://plugins.jetbrains.com/plugin/34594-cranpose)
includes the [cranpose-showcase project template](https://github.com/samoylenkodmitry/cranpose-showcase).
Install the plugin, then choose **File → New Project → Cranpose**. Select an empty
directory. The wizard creates the project, opens the source and starts a preview.

For a terminal workflow, choose **Use this template** on GitHub or clone the starter:

```sh
git clone https://github.com/samoylenkodmitry/cranpose-showcase.git my-app
cd my-app
cargo run --features desktop,renderer-wgpu
```

The starter contains shared Rust UI, Android Gradle files, iOS scripts and a web
build script. Replace the sample screens in `src/screens`. Keep data access in
`src/data` and screen models in `src/presentation`.

### Preview in the IDE

Open a Rust source file, select **Split**, choose a Cargo binary or example, then
press **Run**. Save an edit to reload the preview. Supported literal edits update
directly; other supported edits compile and preserve app state. Use **Restart**
after a structural or type change.

For a component preview, enable `cranpose/preview` in the desktop build and add a
parameterless fixture:

```sh
cargo add cranpose --features preview
```

```rust
use cranpose::prelude::*;

#[cranpose::preview(name = "Greeting", width = 320, height = 120)]
#[composable]
fn GreetingPreview() {
    Text(
        "Hello, Compose",
        Modifier::empty().padding(16.0),
        TextStyle::default(),
    );
}
```

The preview pane selects fixtures and display sizes. The layout inspector shows
bounds and links a selected node to source. Editor stability badges explain
parameter comparison and causes of recomposition. Hover a badge for details.
The [IDE guide](intellij.md) covers preview controls and platform builds.

### Create a package by hand

For a smaller project, create a package with a shared UI library:

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

```rust preview=counter entry=Counter
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
                Modifier::empty()
                    .height_in(48.0, f32::INFINITY)
                    .padding(12.0),
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

Build and run on Windows, Linux or macOS. The commands below use a native toolchain
on the target OS: Visual Studio C++ Build Tools on Windows, a C compiler and
X11/Wayland development packages on Linux, or Xcode Command Line Tools on macOS.
The app needs a GPU driver with a wgpu backend such as Vulkan, Metal or DirectX 12.
GitHub's Windows, Linux and macOS runners can build separate desktop artifacts.

For `cranpose-showcase`, run `cargo run --features desktop,renderer-wgpu`.
For the package created above, use this entry point.

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

Use Windows, Linux or macOS with Android Studio, its SDK and NDK, and a JDK supported
by the project's Android Gradle Plugin. Run the APK on a physical Android device
or an emulator. Match the ABI: `arm64-v8a` for ARM devices and Apple Silicon
emulators; `x86_64` for Intel/AMD emulators. An emulator needs hardware virtualization.

The showcase starter includes the host. For an ARM device, run from the starter root:

```sh
cd android
./gradlew :app:installDebug -PshowcaseAbi=arm64-v8a
```

On Windows, use `gradlew.bat`. For a package created by hand, add the host as follows.

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

Copy the [showcase Android host](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/android) into `my-app/android`.
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

An iOS build needs macOS and Xcode with the iOS SDK. A simulator runs on a Mac.
Use an iPhone or iPad for device tests. Device installs require an Apple development
identity and a provisioning profile.

The showcase starter provides `./ios/run-sim.sh`. For the manual package above,
the following steps create a simulator bundle.

The iOS launcher starts UIKit and renders into a Metal surface. The desktop
`src/main.rs` also serves as the iOS entry point; the target and Cargo feature
select the host.

On an Apple Silicon Mac with Xcode and an iOS Simulator runtime:

```sh
rustup target add aarch64-apple-ios-sim aarch64-apple-ios
cargo build --bin my-app --target aarch64-apple-ios-sim --no-default-features --features ios,renderer-wgpu
```

For an Intel Mac, replace `aarch64-apple-ios-sim` with `x86_64-apple-ios`.

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

### Build Apple targets with GitHub Actions

From a Windows or Linux workstation, push the project to GitHub and build on a
macOS runner. [Standard hosted runners are free for public repositories](https://docs.github.com/en/actions/reference/runners/github-hosted-runners).
The runner supplies Xcode. This workflow compiles the manual app for the ARM simulator:

```yaml
name: iOS simulator
on: [push, workflow_dispatch]
jobs:
  ios:
    runs-on: macos-latest
    steps:
      - uses: actions/checkout@v7
      - run: rustup toolchain install stable --profile minimal
      - run: rustup target add aarch64-apple-ios-sim --toolchain stable
      - run: cargo +stable build --bin my-app --target aarch64-apple-ios-sim --no-default-features --features ios,renderer-wgpu
```

Save the workflow as `.github/workflows/ios.yml`. Add the bundle steps from **Run on
iOS** to produce an app. Device distribution also needs certificates and profiles
from your Apple developer account. A macOS runner builds and tests remotely; local
simulator interaction still needs a Mac.

### Run in a browser

Build on Windows, Linux or macOS. Cranpose selects WebGL by default. Add
`?backend=webgpu` to request WebGPU, or `?backend=auto` to try WebGPU first
with WebGL fallback. Use `localhost` for local tests and HTTPS for a public host.
The showcase starter uses `./build-web.sh`, then serves `dist` as static files.
For the manual package above, add the web target and entry point:

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

Open `http://localhost:8000/?backend=webgpu` in a browser with WebGPU support,
or use `http://localhost:8000/?backend=auto` for WebGPU with WebGL fallback.
`wasm-pack` creates the WebAssembly module and JavaScript loader in `pkg`. The
canvas ID matches the first argument to `run_web`.

## Ownership

### Capture state in callbacks

Rust gives each owned value one owner. `move` transfers captured values into a
closure. Values with `Copy`, such as integers and Cranpose state handles, copy
into each closure.

`MutableState<T>` holds a reference to composition-owned storage. Copy the handle
into child content and callbacks. Every handle reads and updates the same value.

```rust
use cranpose::prelude::*;

#[composable]
fn DraftEditor() {
    let drafts = rememberMutableStateOf(Vec::<String>::new);
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            format!("Drafts: {}", drafts.with(Vec::len)),
            Modifier::empty(),
            TextStyle::default(),
        );
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || drafts.update(|items| items.push(String::from("New draft"))),
            || {
                Text("Add draft", Modifier::empty(), TextStyle::default());
            },
        );
    });
}
```

The composition owns the vector. Both closures own a copy of `drafts`.
`.with(...)` borrows the vector; `.update(...)` edits the vector in place.
`.get()` returns a cloned value, so prefer `.with(...)` for collections and large objects.

### Retain an object

Use `remember` for a value with composition lifetime. Borrow the value through
`.with(...)`. Use `rememberHandle` for an object shared by composables and callbacks.
Add the adapter with `cargo add cranpose-coroflow`.

```rust
use cranpose::prelude::*;
use cranpose_coroflow::{Handle, rememberHandle};

struct Catalog {
    names: Vec<String>,
}

#[composable]
fn CatalogCount(catalog: Handle<Catalog>) {
    let count = catalog.get().names.len();
    Text(
        format!("Books: {count}"),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
fn Library() {
    let catalog = rememberHandle(|| Catalog {
        names: vec![String::from("The Rust Book")],
    });
    CatalogCount(catalog);
}
```

`Handle<T>` implements `Copy`. `.get()` returns an `Rc<T>` and shares the object.
`Catalog` needs neither `Clone` nor `PartialEq`. A plain field change stays outside
Compose observation. Put observable fields in `MutableState` or `StateFlow`.

### Choose a lifetime

- A function owns local `String`, `Vec` and struct values until the scope ends.
- A `move` closure owns captured values until the closure drops.
- `remember` and `rememberMutableStateOf` retain values until their composition group leaves.
- A view-model store retains models until the owner leaves or its back-stack entry is removed.
- `Rc` shares ownership on one thread. `Arc` shares ownership across threads.

Callbacks retained by a component require `'static` captures. Capture owned values
or state handles. `'static` describes independence from stack borrows; the callback
still drops with its owner. UI state handles belong to the UI thread. Use a flow
for data shared with worker threads, as shown in **Shared app state**.

## State

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

## Effects

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

## Flows

### Expose observable data

`coroflow` supplies Kotlin-style `Flow`, `StateFlow`, `MutableStateFlow`,
`SharedFlow`, coroutine scopes and operators. `cranpose-coroflow` connects flows
to Compose state.

```sh
cargo add coroflow cranpose-coroflow
```

Keep a mutable flow inside the data owner. Expose a read-only `StateFlow` to the UI:

```rust
use coroflow::{MutableStateFlow, StateFlow};

struct CounterStore {
    count: MutableStateFlow<u32>,
}

impl CounterStore {
    fn new() -> Self {
        Self {
            count: MutableStateFlow::new(0),
        }
    }

    fn count(&self) -> StateFlow<u32> {
        self.count.as_state_flow()
    }

    fn increment(&self) {
        self.count.update(|value| value + 1);
    }
}
```

`StateFlow` holds a current value. An update emits when the value differs from the previous value.
`.update(...)` applies an atomic change. Concurrent writers may cause the update
closure to run again; keep side effects outside the closure.

### Collect in a composable

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

Create the stable handle in the parent with
`rememberHandle(|| repository.progress())`. The repository returns a `StateFlow<u32>`.
`collectAsStateWithLifecycle` collects while the host is at least `Started`.
`Stopped` pauses collection; a return to `Started` reads the latest value.
`collectAsState` collects for the composition's lifetime.

### Map Kotlin flow concepts

- `MutableStateFlow<T>` stores shared current state. Consumers use `StateFlow<T>`.
- `Flow<T>` emits a sequence. `FlowCollect::collectAsState(initial)` supplies the initial UI value.
- `SharedFlow<T>` broadcasts events. Choose replay and buffer policy for the event stream.
- `map`, `combine`, `distinct_until_changed` and `state_in` derive state from upstream flows.

Use a state flow for a persistent condition, such as download progress or a selected
account. Use an event stream for a one-time action, such as a toast request.
The [showcase view model](https://github.com/samoylenkodmitry/cranpose-showcase/blob/main/src/presentation/body_card_view_model.rs)
combines repository flows and exposes UI state with `state_in`.

## View models

### Give screen logic a scope

`viewModel(key, factory)` corresponds to Android's `viewModel(key) { ... }`.
The factory receives a `MainScope`, the equivalent of `viewModelScope`.
Retain the scope in the model to own async tasks.

```rust
use coroflow::{MainScope, MutableStateFlow, StateFlow};
use cranpose::prelude::*;
use cranpose_coroflow::{StateFlowCollect, ViewModelStoreOwner, viewModel};

struct CounterModel {
    count: MutableStateFlow<u32>,
    scope: MainScope,
}

impl CounterModel {
    fn new(scope: MainScope) -> Self {
        Self {
            count: MutableStateFlow::new(0),
            scope,
        }
    }

    fn count(&self) -> StateFlow<u32> {
        self.count.as_state_flow()
    }

    fn increment(&self) {
        self.count.update(|value| value + 1);
    }

    fn increment_later(&self) {
        let count = self.count.clone();
        self.scope.launch(async move {
            cranpose::prelude::delay(std::time::Duration::from_secs(1)).await;
            count.update(|value| value + 1);
        });
    }
}

#[composable]
fn CounterRoute() {
    let model = viewModel((), CounterModel::new);
    let count = model.get().count().collectAsStateWithLifecycle();
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            format!("Count: {}", count.get()),
            Modifier::empty(),
            TextStyle::default(),
        );
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || model.get().increment(),
            || {
                Text("Add now", Modifier::empty(), TextStyle::default());
            },
        );
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || model.get().increment_later(),
            || {
                Text(
                    "Add after one second",
                    Modifier::empty(),
                    TextStyle::default(),
                );
            },
        );
    });
}

#[composable]
fn StandaloneScreen() {
    ViewModelStoreOwner(CounterRoute);
}
```

The async task owns a cloned flow handle. Both handles share the same counter.
UI callbacks copy the model's `Handle`.

### Choose the model owner

`NavHost` provides a store for each back-stack entry. The model survives while the
entry remains in the stack, including time behind another destination.
`ViewModelStoreOwner` provides a store for a dialog, screen or other subtree.
Removal of the owner releases the model; the model's scope cancels its tasks.

Use `viewModel(item_id, factory)` for one model per list item. A row can leave the
viewport and return to the same model. `ProvideViewModelStore` accepts a store
with test models. `SavedStateHandle` stores small restorable values; a database
or file owns persistent documents.

The showcase separates [presentation models](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/src/presentation)
from [screens](https://github.com/samoylenkodmitry/cranpose-showcase/tree/main/src/screens).
Screens collect state and send user actions to model methods.

## Shared app state

### Share a custom struct with a worker

Use a `MutableStateFlow` inside a custom store. Wrap the store in `Arc` when UI
and worker threads both own the store. Use `Handle` to pass the store between
composables. Rust declares fields in a `struct` and methods in an `impl` block.
The state below contains a completed-work count and a user-editable goal.

```rust
use coroflow::{MutableStateFlow, StateFlow};
use cranpose::prelude::*;
use cranpose_coroflow::{Handle, StateFlowCollect, rememberHandle};
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq)]
struct WorkState {
    completed: u32,
    goal: u32,
}

struct WorkStore {
    state: MutableStateFlow<WorkState>,
}

impl WorkStore {
    fn new() -> Self {
        Self {
            state: MutableStateFlow::new(WorkState {
                completed: 0,
                goal: 100,
            }),
        }
    }

    fn state(&self) -> StateFlow<WorkState> {
        self.state.as_state_flow()
    }

    fn complete_one(&self) {
        self.state.update(|value| WorkState {
            completed: value.completed + 1,
            ..*value
        });
    }

    fn extend_goal(&self) {
        self.state.update(|value| WorkState {
            goal: value.goal + 10,
            ..*value
        });
    }
}

#[composable]
fn WorkSummary(store: Handle<Arc<WorkStore>>) {
    let state = store.get().state().collectAsStateWithLifecycle();
    let value = state.get();
    Text(
        format!("Completed: {} / {}", value.completed, value.goal),
        Modifier::empty(),
        TextStyle::default(),
    );
}

#[composable]
fn GoalButton(store: Handle<Arc<WorkStore>>) {
    Button(
        Modifier::empty(),
        ButtonSpec::default(),
        move || store.get().extend_goal(),
        || {
            Text("Add 10 to goal", Modifier::empty(), TextStyle::default());
        },
    );
}

#[composable]
fn WorkScreen() {
    let store = rememberHandle(|| Arc::new(WorkStore::new()));
    LaunchedEffect((), move |scope| {
        let worker = Arc::clone(store.get().as_ref());
        scope.launch_background(
            move |token| async move {
                for _ in 0..100 {
                    if token.is_cancelled() {
                        break;
                    }
                    worker.complete_one();
                }
            },
            |_| {},
        );
    });
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        WorkSummary(store);
        GoalButton(store);
    });
}
```

The UI updates `goal`; the worker updates `completed`. Each atomic update preserves
the other field across concurrent changes. The flow owns the current `WorkState`.

`Arc::clone` creates one worker owner. Composable calls copy `Handle`.
The UI releases its owner when `WorkScreen` leaves composition. The worker releases
its owner when the job ends. The cancellation token lets long jobs stop early.
On WebAssembly, `launch_background` uses the browser event loop. A Web Worker needs
a message boundary for CPU-heavy work.

## Composition locals

Add the provider types with `cargo add cranpose-core`.

### Read platform values

Composition locals correspond to Compose's `LocalDensity`, `LocalLayoutDirection`
and service providers. `.current()` reads the nearest provider and subscribes the
composable to changes. The launcher supplies platform values; a subtree can override
values for a preview or a test.

```rust
use cranpose::prelude::*;

#[composable]
fn SafeContent() {
    let safe = local_safe_area_insets().current();
    let keyboard = local_ime_insets().current();
    Column(
        Modifier::empty().padding_each(
            safe.left,
            safe.top,
            safe.right,
            safe.bottom.max(keyboard.bottom),
        ),
        ColumnSpec::default(),
        || {
            Text(
                "Content above the keyboard",
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}
```

### Override density and direction

`local_density()` supplies pixel density and font scale.
`local_layout_direction()` supplies left-to-right or right-to-left layout.

```rust
use cranpose::prelude::*;
use cranpose_core::CompositionLocalProvider;

#[composable]
fn ArabicPreview() {
    CompositionLocalProvider([local_density().provides(Density::new(1.0, 1.3))], || {
        ProvideLayoutDirection(LayoutDirection::Rtl, || {
            Text("مرحبا", Modifier::empty(), TextStyle::default());
        });
    });
}
```

### Override theme and accessibility

`local_system_theme()` exposes `SystemTheme::Light` or `SystemTheme::Dark`.
`local_accessibility_options()` exposes font scale, motion, transparency and contrast
preferences. `local_accessibility_state()` exposes active assistive services.

```rust
use cranpose::prelude::*;

#[composable]
fn LargeTextPreview() {
    ProvideSystemTheme(SystemTheme::Dark, || {
        ProvideAccessibilityOptions(
            AccessibilityOptions {
                font_scale: 1.5,
                reduce_motion: true,
                ..Default::default()
            },
            || {
                Text("Account settings", Modifier::empty(), TextStyle::default());
            },
        );
    });
}
```

### Read services and screen context

- `local_http_client()` supplies HTTP requests.
- `local_uri_handler()` and `local_share_sheet()` open links and share content.
- `local_file_picker()` and `local_image_picker()` select documents and images.
- `local_audio()`, `local_haptics()` and `local_notifier()` access device services.
- `local_launch_args()` supplies launch arguments and deep-link values.
- `local_lifecycle_state()` supplies the host lifecycle.
- `local_focus_manager()` moves or clears keyboard focus.
- `local_announcer()` announces a status change to assistive technology.
- `LocalWindowState::current()` reads the nearest `.window(...)` state.

Service providers use names such as `ProvideUriHandler`, `ProvideFilePicker`,
`ProvideAudio`, `ProvideHaptics` and `ProvideNotifier`. Supply a service implementation
to replace the platform service within a subtree. **Platform services** contains
complete call examples. Layout and text components also provide locals for lazy-item
keys, modal depth, focus reveal and text-surface contrast.

### Supply a test HTTP service

Provide a stub to exercise the same request code in a preview or a test:

```rust
use cranpose::prelude::*;
use cranpose_core::CompositionLocalProvider;
use std::sync::Arc;

#[composable]
fn RequestPreview() {
    let client = remember(|| {
        let client: HttpClientRef = Arc::new(StubHttpClient::with_body(b"Sample response"));
        client
    });
    client.with(|client| {
        CompositionLocalProvider([local_http_client().provides(Arc::clone(client))], || {
            let response = rememberMutableStateOf(String::new);
            let client = local_http_client().current();
            LaunchedEffectAsync((), move |_| {
                Box::pin(async move {
                    let result = client.get_text("https://example.test/message").await;
                    response.set(result.unwrap_or_else(|error| error.to_string()));
                })
            });
            Text(response.get(), Modifier::empty(), TextStyle::default());
        });
    });
}
```

The request returns the stub body. Other subtrees keep their own HTTP provider.

### Provide app values

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

## App lifecycle

### Match the host lifecycle

`Created`, `Started`, `Resumed`, `Paused`, `Stopped` and `Destroyed` describe the host.
`Paused` remains at least `Started`: the UI remains visible while another surface
has focus. A stopped host pauses lifecycle-aware collectors.

- **Android:** Activity start, resume, pause, stop and destroy callbacks drive the states.
- **iOS:** scene activation resumes the host; scene suspension stops the host. Shutdown destroys the host.
- **Desktop:** a visible app with a focused window is resumed. A visible app with focus elsewhere is paused. An occluded or minimized main window stops the host.
- **Web:** a visible, focused page is resumed. A visible page with focus elsewhere is paused. A hidden tab or minimized browser stops the host.

Read the lifecycle as Compose state:

```rust
use cranpose::prelude::*;

#[composable]
fn HostStatus() {
    let lifecycle = rememberLifecycleState();
    Text(
        format!("Host: {:?}", lifecycle.get()),
        Modifier::empty(),
        TextStyle::default(),
    );
}
```

### Pair resource use with visibility

Add `cranpose-coroflow` for `LifecycleStartEffect` and `LifecycleResumeEffect`.
This camera session starts while the host has focus and stops on pause or
composition exit:

```rust
use cranpose::prelude::*;
use cranpose_coroflow::LifecycleResumeEffect;

#[composable]
fn FocusedCamera() {
    let error = rememberMutableStateOf(|| None::<String>);
    LifecycleResumeEffect((), move |scope| {
        error.set(start_camera().err().map(|failure| failure.to_string()));
        scope.on_pause_or_dispose(stop_camera)
    });
    if let Some(message) = error.get() {
        Text(message, Modifier::empty(), TextStyle::default());
    }
}
```

Declare camera access in the platform manifest, as described in **Platform services**.
Use `LifecycleStartEffect` with `on_stop_or_dispose` for work allowed while the UI
remains visible.

### Separate screen lifetime from app lifetime

`DisposableEffect` ends when its keys change or its composition group leaves.
`LaunchedEffect` cancels composition-owned tasks at the same boundary.
App focus and visibility can change while the screen remains composed. Use a
lifecycle effect for focus-sensitive resources and `collectAsStateWithLifecycle`
for screen data. Persist user edits when the edits occur; process termination can
skip a final lifecycle callback.

## Navigation

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

### Graphics composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

#### Canvas

Draw in local coordinates. Give the canvas an explicit size.

```rust preview=canvas
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

`IconWith` accepts an explicit modifier and `IconSpec`.

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

## Accessibility

### Name controls and expose state

Text buttons carry their label. Give icon buttons a name. Custom controls need a
role, current state and action. This custom toggle exposes all three:

```rust
use cranpose::prelude::*;

#[composable]
fn FavoriteToggle() {
    let selected = rememberMutableStateOf(|| false);
    let checked = selected.get();
    Box(
        Modifier::empty()
            .size_points(48.0, 48.0)
            .clickable(move |_| selected.update(|value| *value = !*value))
            .semantics(move |config| {
                config.content_description = Some(String::from("Favorite"));
                config.role = Some(SemanticsWidgetRole::Checkbox);
                config.toggled = Some(checked);
            }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                if checked { "★" } else { "☆" },
                Modifier::empty(),
                TextStyle::default(),
            );
        },
    );
}
```

Use `heading()` for section titles and `pane_title("Library")` for a screen region.
Pass `None` as an image description for decoration. Give charts a text summary or
`canvas_children` with separate accessible bounds and actions.

### Respect text size and motion preferences

Use `TextUnit::Sp` for font sizes. The host applies the user's font scale:

Import `cranpose::prelude::*` and place this example inside a composable.

```rust preview=scaled_text
use cranpose::text::TextUnit;
Text(
    "Your library",
    Modifier::empty(),
    TextStyle {
        span_style: SpanStyle {
            font_size: TextUnit::Sp(24.0),
            ..Default::default()
        },
        ..Default::default()
    },
);
```

Allow the label to wrap and the parent to grow vertically. Use
`ProvideAccessibilityOptions` from **Composition locals** to preview larger text.
Cranpose animations and Liquid surfaces read motion, transparency and contrast
preferences. Custom drawing can read `local_accessibility_options().current()`
and choose a static or higher-contrast presentation.

### Support keyboard and screen readers

Buttons and fields participate in focus traversal. Tab and Shift+Tab move focus;
Enter and Space activate the focused control. Use the focus manager for an explicit
action such as dismissal of the keyboard:

```rust
use cranpose::prelude::*;

#[composable]
fn DoneButton() {
    let focus = local_focus_manager().current();
    Button(
        Modifier::empty(),
        ButtonSpec::default(),
        move || {
            focus.clear_focus();
        },
        || {
            Text("Done", Modifier::empty(), TextStyle::default());
        },
    );
}
```

Verify custom controls with TalkBack on Android, VoiceOver on Apple platforms,
and the target desktop or browser screen reader. The [accessibility guide](accessibility.md)
covers focus groups, canvas controls, announcements and validation tools.

## Testing

### Add a test dependency

Run tests on Windows, Linux or macOS with the app's Rust toolchain and desktop
build dependencies. The composition and headless input tests below use the CPU.

From the `my-app` package in **Get started**:

```sh
cargo add cranpose-testing --dev
```

Cargo runs functions marked `#[test]` in `tests/*.rs`. These files import the app
library as `my_app`. Keep reusable composables in `src/lib.rs` or library modules;
keep the platform entry point in `src/main.rs`.

| Test scope | API | Example check |
| --- | --- | --- |
| Composition and layout | `ComposeTestRule` | A button has a label and a 48-point touch target |
| Input and state | `create_headless_robot_test` | A click changes the counter text |
| Desktop window and renderer | `AppLauncher::with_test_driver` | The app accepts input and produces a screenshot |

### Check layout and semantics

Create `tests/composition.rs`. Reuse `Counter` from **Get started**:

```rust
use cranpose::prelude::*;
use cranpose_testing::ComposeTestRule;
use my_app::Counter;

#[test]
fn increment_button_has_a_label_and_touch_target() {
    let mut rule = ComposeTestRule::new();
    rule.set_content(Counter).expect("Counter composes");

    let screen = rule
        .placed_semantics(Size::new(360.0, 240.0))
        .expect("Counter lays out")
        .expect("Counter has content");
    let button = screen
        .controls()
        .into_iter()
        .find(|node| node.label.as_deref() == Some("Increment"))
        .expect("Increment has an accessible label");

    assert!(button.target_bounds().height >= 48.0);
}
```

`placed_semantics` settles pending composition work, applies the viewport size,
and returns accessible labels and bounds. `target_bounds()` includes the control's
hit area. Run the test by file name:

```sh
cargo test --test composition
```

### Check a state update

Add this test to `tests/composition.rs`. `with_runtime` binds the state handle to
the test composition. The assertion checks the text after a state change:

```rust
#[test]
fn state_change_updates_the_label() {
    let mut rule = ComposeTestRule::new();
    let count = MutableState::with_runtime(0, rule.runtime_handle());
    rule.set_content(move || {
        Text(
            format!("Count: {}", count.get()),
            Modifier::empty(),
            TextStyle::default(),
        );
    })
    .expect("Label composes");

    count.set(3);
    let screen = rule
        .placed_semantics(Size::new(360.0, 240.0))
        .expect("Label lays out")
        .expect("Label has content");

    assert!(screen
        .flatten()
        .iter()
        .any(|node| node.label.as_deref() == Some("Count: 3")));
}
```

### Audit accessibility

Add a screen title around the counter and audit the whole screen:

```rust
#[test]
fn counter_passes_the_accessibility_audit() {
    let mut rule = ComposeTestRule::new();
    rule.set_content(|| {
        Box(
            Modifier::empty().fill_max_size().pane_title("Counter"),
            BoxSpec::default(),
            Counter,
        );
    })
    .expect("Screen composes");

    rule.assert_accessible(Size::new(360.0, 240.0))
        .expect("Screen lays out");
}
```

The audit reports unnamed controls, duplicate control names, small targets and
other semantics issues. Pair the audit with the platform screen-reader checks in
**Accessibility**.

## Robot tests

### Click a control in a headless test

Create `tests/counter.rs`. The headless robot runs the app shell, layout and input
code with an in-memory renderer:

```rust
use cranpose_testing::create_headless_robot_test;
use my_app::Counter;

#[test]
fn increment_changes_the_counter() {
    let mut robot = create_headless_robot_test(360, 240, Counter);

    robot.find_by_text("Count: 0").assert_exists();
    assert!(robot.find_by_text("Increment").click());
    assert!(robot.get_all_text().iter().any(|text| text == "Count: 1"));
}

#[test]
fn increment_stays_inside_each_viewport() {
    let mut robot = create_headless_robot_test(360, 640, Counter);

    for (width, height) in [(360, 640), (800, 600)] {
        robot.set_viewport(width, height);
        let bounds = robot
            .find_by_text("Increment")
            .bounds()
            .expect("Increment is visible");
        assert!(bounds.x >= 0.0);
        assert!(bounds.x + bounds.width <= width as f32);
        assert!(bounds.y + bounds.height <= height as f32);
    }
}
```

`find_by_text` matches text fragments. `get_all_text` supports exact comparisons.
The finder settles pending frames before a query. `click` sends pointer input to
the label's center. The final text assertion checks the button's effect.

```sh
cargo test --test counter
cargo test --test composition --test counter
```

### Run a desktop robot

A desktop robot uses the app's window backend and wgpu renderer. Use the desktop
build tools and GPU driver from **Get started**. Headless desktop mode creates a
hidden OS window; Linux still needs an X11 or Wayland display server.

Add `robot` to the app's existing `[features]` table in `Cargo.toml`. Add the test
target as a separate table:

```toml
[features]
robot = ["cranpose/robot"]

[[test]]
name = "counter_robot"
path = "tests/robots/counter.rs"
harness = false
required-features = ["desktop", "renderer-wgpu", "robot"]
```

`harness = false` lets Cargo run `main` directly, so the desktop event loop runs
on the main thread. Create `tests/robots/counter.rs`:

```rust
use cranpose::{AppLauncher, Robot};
use my_app::Counter;
use std::sync::mpsc;

fn check_counter(robot: &Robot) -> Result<(), String> {
    robot.wait_for_idle()?;
    let (x, y, width, height) = robot
        .find_button_bounds_exact("Increment")?
        .ok_or("Increment button is missing")?;
    robot.click(x + width / 2.0, y + height / 2.0)?;
    robot.wait_for_idle()?;

    if robot.find_text_bounds_exact("Count: 1")?.is_none() {
        return Err("Counter displays the wrong value after a click".into());
    }
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let (sender, receiver) = mpsc::sync_channel(1);
    AppLauncher::new()
        .with_title("Counter test")
        .with_size(360, 240)
        .with_headless(true)
        .with_test_driver(move |robot| {
            let result = check_counter(&robot);
            let _ = sender.send(result.and(robot.exit()));
        })
        .try_run(Counter)?;
    receiver.recv()?.map_err(anyhow::Error::msg)?;
    Ok(())
}
```

The driver runs on a separate thread. `wait_for_idle` waits for pending app work.
The driver closes the app after either result and sends failures to `main`, so
Cargo reports a nonzero exit code for a failed check.

```sh
cargo test --test counter_robot --features desktop,renderer-wgpu,robot
```

Set `.with_headless(false)` to watch the test in a desktop window.
For an Ubuntu or Debian CI machine, install Xvfb and a software Vulkan driver
alongside the app's desktop build dependencies:

```sh
sudo apt-get update
sudo apt-get install -y xvfb mesa-vulkan-drivers libvulkan1
xvfb-run -a cargo test --test counter_robot --features desktop,renderer-wgpu,robot
```

### Save a render capture

Add a PNG encoder as a development dependency:

```sh
cargo add image --dev --no-default-features --features png
```

Add this function to `tests/robots/counter.rs`. Call `save_capture(robot)?` in
`check_counter` before `Ok(())`:

```rust
fn save_capture(robot: &Robot) -> Result<(), String> {
    let capture = robot.screenshot()?;
    image::save_buffer(
        "counter.png",
        &capture.pixels,
        capture.width,
        capture.height,
        image::ColorType::Rgba8,
    )
    .map_err(|error| error.to_string())
}
```

`screenshot` captures Cranpose's render target. Keep the viewport, font, scale and
renderer fixed for image comparisons. For a continuous animation, advance a
bounded frame count before a capture:

```rust
robot.pump_frames(3)?;
save_capture(robot)?;
```

Use platform tests for native controls and OS dialogs: Android UI Automator or
Espresso, and Apple XCTest. Keep pointer, layout and state checks in the Rust suite.

## App windows

### Put a composable subtree in a window

`.window(...)` moves a subtree to a separate OS window on desktop. The subtree
keeps its composition identity, state and effects. Android, iOS and web hosts keep
the same content inline.

```rust
use cranpose::prelude::*;

#[composable]
fn Inspector() {
    let detached = rememberMutableStateOf(|| false);
    let window = rememberWindowState(320.0, 220.0);
    let modifier = if detached.get() {
        Modifier::empty().window(
            WindowConfig::new_for_state("Inspector", window)
                .with_min_size(240.0, 160.0)
                .on_close_requested(move || detached.set(false)),
        )
    } else {
        Modifier::empty().width(320.0)
    };
    Column(modifier.padding(16.0), ColumnSpec::default(), move || {
        let count = rememberMutableStateOf(|| 0_u32);
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || count.update(|n| *n += 1),
            move || {
                Text(
                    format!("Count: {}", count.get()),
                    Modifier::empty(),
                    TextStyle::default(),
                );
            },
        );
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || detached.update(|value| *value = !*value),
            move || {
                Text(
                    if detached.get() { "Dock" } else { "Detach" },
                    Modifier::empty(),
                    TextStyle::default(),
                );
            },
        );
    });
}
```

Keep the content call at the same composition position and change the modifier.
The counter survives each dock and detach action. The close callback returns the
inspector to the main window.

### Read window geometry

`rememberWindowStateAt(x, y, width, height)` adds an initial position.
`window.size()` reads content size; `window.position()` reads the outer position.
The units are logical pixels. A nested composable can read the nearest window:

```rust
use cranpose::LocalWindowState;
use cranpose::prelude::*;

#[composable]
fn WindowSizeLabel() {
    if let Some(window) = LocalWindowState::current() {
        let size = window.size();
        Text(
            format!("{} × {}", size.width, size.height),
            Modifier::empty(),
            TextStyle::default(),
        );
    }
}
```

`WindowConfig::borderless_for_state` removes system decoration.
Use `.window_drag_area(on_start, on_end)` for a custom title bar and
`.window_resize_area(direction)` for an edge. `with_visible`, `with_resizable`,
`with_always_on_top` and `with_transparent` configure the host window.
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

Capture a JPEG from a button and retain the bytes in state:

```rust
use cranpose::prelude::*;

#[composable]
fn CapturePhoto() {
    let photo = rememberMutableStateOf(|| None::<CameraStill>);
    let error = rememberMutableStateOf(|| None::<String>);
    DisposableEffect((), move |_| {
        if let Err(failure) = start_camera() {
            error.set(Some(failure.to_string()));
        }
        DisposableEffectResult::new(stop_camera)
    });
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        let scope = rememberCoroutineScope();
        Button(
            Modifier::empty(),
            ButtonSpec::default(),
            move || {
                scope.launch(async move {
                    match capture_camera_still().await {
                        Ok(still) => {
                            photo.set(Some(still));
                            error.set(None);
                        }
                        Err(failure) => error.set(Some(failure.to_string())),
                    }
                });
            },
            || {
                Text("Take photo", Modifier::empty(), TextStyle::default());
            },
        );
        if let Some(bytes) = photo.with(|value| value.as_ref().map(|still| still.jpeg.len())) {
            Text(
                format!("JPEG: {bytes} bytes"),
                Modifier::empty(),
                TextStyle::default(),
            );
        }
        if let Some(message) = error.get() {
            Text(message, Modifier::empty(), TextStyle::default());
        }
    });
}
```

`CameraStill::jpeg` contains the encoded photo. Decode the JPEG with EXIF orientation support.
Use `launch_background` for image processing on native targets.

### Add haptic feedback

Call haptics from a user action:

```rust
use cranpose::prelude::*;

fn confirm_selection() {
    default_haptics().vibrate(18, 120);
}
```

The arguments are milliseconds and amplitude, from `0` to `255`.
Retain a `HapticPattern` for repeated actions. Each duration has a corresponding amplitude:

```rust
use cranpose::prelude::*;

#[composable]
fn ConfirmButton() {
    let pattern = remember(|| HapticPattern::new(&[20, 40, 20], &[120, 0, 180]));
    Button(
        Modifier::empty(),
        ButtonSpec::default(),
        move || {
            pattern.with(|result| match result {
                Ok(pattern) => default_haptics().play_pattern(pattern),
                Err(error) => eprintln!("Haptic pattern error: {error}"),
            });
        },
        || {
            Text("Confirm", Modifier::empty(), TextStyle::default());
        },
    );
}
```

This pattern plays two pulses with a 40-millisecond pause.

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
Sizes use logical points. The host applies display density.
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

```rust preview=column
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

```rust preview=row
use cranpose::widgets::*;

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

```rust preview=layers
use cranpose::widgets::*;

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

```rust preview=spacer
use cranpose::widgets::*;

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

```rust preview=flow_row
use cranpose::widgets::*;

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

`ForEach` wraps a keyed `for` loop. Both forms compose every item and retain child
state by the hash of each item. Use `LazyColumn` for a collection larger than the viewport.

```rust
ForEach(&["Mercury", "Venus", "Earth"], |planet| {
    Text(*planet, Modifier::empty(), TextStyle::default());
});
```

The explicit loop has the same per-item key behavior:

```rust
for planet in ["Mercury", "Venus", "Earth"] {
    cranpose_core::with_key(&planet, || {
        Text(planet, Modifier::empty(), TextStyle::default());
    });
}
```

Choose an explicit ID key when an item's label changes or labels repeat:

```rust
for (id, title) in [(1_u64, "Inbox"), (2, "Inbox")] {
    cranpose_core::with_key(&id, || {
        Text(title, Modifier::empty(), TextStyle::default());
    });
}
```

#### LazyColumn

Compose visible items in a vertical viewport. Retain the scroll state in composition.

```rust preview=lazy_column
use cranpose::widgets::*;

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

`LazyRow` uses `LazyRowSpec` for a horizontal list with the same item API.

#### BoxWithConstraints

Choose content from the available width during layout.

```rust preview=constraints
use cranpose::widgets::*;

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

```rust preview=scaffold
use cranpose::widgets::*;

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

`ScaffoldWith` adds `ScaffoldSpec` for surfaces and inset policy, plus a floating-action slot.

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

Use `HorizontalScrollbar` for a horizontal track. `Scrollbar` accepts an explicit orientation.

#### Layout

Supply a measure policy for a custom container. This example reuses the box policy.
Implement `cranpose_ui_layout::MeasurePolicy` for custom child sizes and positions.

```rust preview=layout
use cranpose::widgets::*;

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

### Text composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.

#### Text

Show plain text or an `AnnotatedString` with a `TextStyle`.

```rust preview=text
use cranpose::widgets::*;

Text("Your library", Modifier::empty(), TextStyle::default());
```

`TextWithOptions` adds line limits, ellipsis and layout callbacks through `TextOptions`. `BasicText` and `BasicTextWithOptions` expose the lower-level text primitive.

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

```rust preview=text_field
use cranpose::widgets::*;

let name = remember(|| TextFieldState::new("Ada")).with(|state| *state);
BasicTextField(
    name,
    Modifier::empty().fill_max_width().content_description("Name"),
    TextStyle::default(),
);
```

`BasicTextFieldWithOptions` adds edit and layout options. `BasicTextFieldDecorated` adds a decoration slot around the editor.

#### SelectionContainer

Allow selection across several text children.

```rust preview=selection
use cranpose::widgets::*;

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

## Controls

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

`IconButtonWith` accepts an explicit modifier and icon-button spec.

#### Slider

Use a value from `0.0` to `1.0`. Supply the track and thumb in the content slot.

```rust preview=slider
use cranpose::widgets::*;

let volume = rememberMutableStateOf(|| 0.5_f32);
Slider(
    Modifier::empty()
        .fill_max_width()
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

```rust preview=dismiss
use cranpose::widgets::*;

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

`SwipeToDismissBox` supplies an edge-swipe preset for screen navigation and preserves content size after dismissal.

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

`PopupAnchored` follows an anchor rectangle. `PopupDismissable` adds outside-click and back dismissal; `PopupDismissableWhen` also takes visibility state.

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

`DialogWithScrim` adds an explicit scrim color. Put dialog state in the parent and clear the state in the dismissal callback.

### Progress composables

Import `cranpose::prelude::*` and `cranpose::widgets::*`. Place these examples inside a composable.

#### CircularProgressIndicator

Show indeterminate progress in a bounded circle.

```rust preview=circular_progress
use cranpose::widgets::*;

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

```rust preview=linear_progress
use cranpose::widgets::*;

LinearProgressIndicator(
    Modifier::empty()
        .fill_max_width()
        .height(4.0)
        .content_description("Request in progress"),
    Color::BLUE,
);
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

## Liquid components

Liquid provides an opinionated theme and controls for Apple-style glass interfaces.
Use Liquid components together for a consistent app surface.

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

```rust preview=glass_button
use cranpose::liquid::prelude::*;

let saved = rememberMutableStateOf(|| false);
GlassButton(
    Modifier::empty(),
    GlassButtonSpec::prominent(),
    move || saved.set(true),
    move || {
        GlassButtonLabel(if saved.get() { "Saved" } else { "Save" }, GlassButtonSpec::prominent());
    },
);
```

`GlassButtonLabel` supplies a label with the current glass-button style.

#### GlassIconButton

Create a circular action from an icon path.

```rust preview=glass_icon
use cranpose::liquid::prelude::*;

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

```rust preview=glass_group
use cranpose::liquid::prelude::*;

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

```rust preview=surface
use cranpose::liquid::prelude::*;

Surface(Modifier::empty().padding(16.0), || {
    Text("Account", Modifier::empty(), liquid_typography().body);
});
```

#### Card

Group content in a rounded card.

```rust preview=card
use cranpose::liquid::prelude::*;

Card(Modifier::empty().padding(16.0), || {
    Text("Account", Modifier::empty(), liquid_typography().body);
});
```

`LiquidCard` is the implementation behind `Card`. `GlassSurface` accepts an explicit `Glass` material.

#### LiquidListSection

Group related rows under a section title.

```rust preview=list_section
use cranpose::liquid::prelude::*;

LiquidListSection(Modifier::empty(), "Account", || {
    Text("Ada", Modifier::empty(), liquid_typography().body);
});
```

#### LiquidListRow

Add an action row with a Liquid row spec.

```rust preview=list_row
use cranpose::liquid::prelude::*;

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

```rust preview=chip
use cranpose::liquid::prelude::*;

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

```rust preview=action_chip
use cranpose::liquid::prelude::*;

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

```rust preview=toggle
use cranpose::liquid::prelude::*;

let enabled = rememberMutableStateOf(|| true);
LiquidToggle(
    Modifier::empty().content_description("Notifications"),
    enabled.get(),
    move |value| enabled.set(value),
);
```

#### LiquidSlider

Show a styled slider with a value in `0.0..=1.0`.

```rust preview=liquid_slider
use cranpose::liquid::prelude::*;

let volume = rememberMutableStateOf(|| 0.5_f32);
LiquidSlider(
    Modifier::empty().fill_max_width().content_description("Volume"),
    volume.get(),
    move |value| volume.set(value),
);
```

#### LiquidSegmentedControl

Select one segment by index.

```rust preview=segments
use cranpose::liquid::prelude::*;

let selected = rememberMutableStateOf(|| 0_usize);
LiquidSegmentedControl(
    Modifier::empty().fill_max_width(),
    selected.get(),
    move |index| selected.set(index),
    |segments| {
        segments.segment("All");
        segments.segment("Favorites");
    },
);
```

#### SearchField

Edit a query in a themed search field.

```rust preview=search
use cranpose::liquid::prelude::*;

let query = remember(|| TextFieldState::new("")).with(|state| *state);
SearchField(Modifier::empty().fill_max_width(), query, "Search books");
```

`SearchBar` is another name for the same field. `LiquidSearchField` accepts `LiquidSearchFieldSpec`.

#### LiquidNavBar

Collapse the title as the content scrolls. Share the `ScrollState` with the content.

```rust preview=nav_bar
use cranpose::liquid::prelude::*;

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

```rust preview=tab_bar
use cranpose::liquid::prelude::*;

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

`LiquidTabBarWithAccessory` adds an accessory slot. `LiquidTabBarSearchAccessory` supplies a search accessory.

#### LiquidDropdownMenu

Anchor a menu to a composable. Place the menu under `PopupHost`.

```rust preview=dropdown
use cranpose::liquid::prelude::*;
use cranpose::widgets::PopupHost;

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

`LiquidMenuAbsorbedIconButton` uses the absorbed-button treatment. `cranpose::liquid::icons::Icon` draws a themed Liquid icon.

## Wear components

Wear provides opinionated controls for round watch screens and Wear OS-style layouts.
The component library handles curved lists, scale and watch control geometry.

### Wear composables

Import `cranpose::prelude::*` and `cranpose::widgets::*` at module scope.
Each example below goes inside a `#[composable]` function.
Also import `cranpose::widgets::wear::*` and `cranpose::round_scaling_list::CentreAnchor`.

#### ScreenScaffold

Provide a watch screen with a curved scroll indicator. Share the list state with the scaffold.

```rust preview=watch_screen
use cranpose::widgets::wear::*;
use cranpose::round_scaling_list::CentreAnchor;

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

```rust preview=watch_list
use cranpose::widgets::wear::*;
use cranpose::round_scaling_list::CentreAnchor;

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

`WearScalingLazyColumnNode` accepts `WearScalingListContent` and returns the layout node ID.

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

```rust preview=watch_header
use cranpose::widgets::wear::*;

ListHeader(
    Modifier::empty().fill_max_width(),
    ListHeaderSpec::default(),
    "Playback",
);
```

#### WearButton

Show a capsule button with primary and secondary labels.

```rust preview=watch_button
use cranpose::widgets::wear::*;

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

```rust preview=watch_switch
use cranpose::widgets::wear::*;

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

`SwitchButtonNode` returns the layout node ID. `SwitchGraphic` draws the switch graphic inside a custom control.

## Compose integration

### Put a Rust screen inside an Android Compose screen

Use Windows, Linux or macOS with the Android SDK, NDK and the project’s JDK.
Test on an Android device or emulator, as described in **Get started**.

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
    implementation("net.java.dev.jna:jna:5.19.1@aar")
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

Build on macOS with Xcode and the iOS SDK. Use an Apple device or iOS Simulator
for tests. A GitHub macOS runner can build from a Windows or Linux workstation;
**Get started** contains the workflow.

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
fn NativeVolume(volume: MutableState<u32>, modifier: Modifier) {
    NativeView(
        "volume",
        &volume.get().to_string(),
        modifier,
        move |event| {
            if let Ok(value) = event.parse::<u32>() {
                volume.set(value.min(100));
            }
        },
    );
}

#[composable]
fn VolumeScreen() {
    let volume = rememberMutableStateOf(|| 50_u32);
    NativeVolume(volume, Modifier::empty().fill_max_width().height(56.0));
}
```

Add a factory to the Rust library from **Compose integration**. Reuse the imports
for `Arc`, `NativeSession` and `NativeContent`:

```rust
#[uniffi::export]
pub fn create_volume() -> Arc<NativeSession> {
    NativeSession::new(|| NativeContent::new(VolumeScreen))
}
```

Generate the bindings after the Rust build. Kotlin and Swift expose the factory
as `createVolume`.

Register `volume` as a native `SeekBar` factory:

```kotlin
import android.content.Context
import android.widget.SeekBar
import dev.cranpose.NativeViewChild
import dev.cranpose.NativeViewFactory

class VolumeFactory : NativeViewFactory {
    override fun create(context: Context, emit: (String) -> Unit): NativeViewChild {
        val slider = SeekBar(context).apply {
            max = 100
            contentDescription = "Volume"
        }
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

Create the Android component with the generated factory:

```kotlin
import dev.cranpose.CranposeView
import uniffi.counter_ui.createVolume

val component = CranposeView(
    context,
    createVolume(),
    mapOf("volume" to VolumeFactory()),
)
```

- Configuration changes call `update` on the same child.
- Slot removal calls `dispose`.
- A factory-name change replaces the child.
- Events reach the child's own slot.

### Wrap an iOS control

Use the same `VolumeScreen` and `create_volume` Rust factory. Add `VolumeFactory.swift`
to the UIKit host from **Compose integration**:

```swift
import UIKit
import Cranpose

@MainActor
struct VolumeFactory: NativeViewFactory {
    func create(emit: @escaping (String) -> Void) -> any NativeViewChild {
        VolumeChild(emit: emit)
    }
}

@MainActor
final class VolumeChild: NSObject, NativeViewChild {
    private let slider = UISlider()
    private var emit: ((String) -> Void)?
    var view: UIView { slider }

    init(emit: @escaping (String) -> Void) {
        self.emit = emit
        super.init()
        slider.minimumValue = 0
        slider.maximumValue = 100
        slider.accessibilityLabel = "Volume"
        slider.addTarget(self, action: #selector(changed), for: .valueChanged)
    }

    @objc private func changed() {
        emit?(String(Int(slider.value.rounded())))
    }

    func update(_ value: String) {
        guard let number = Float(value) else { return }
        slider.setValue(min(100, max(0, number)), animated: false)
    }

    func dispose() {
        slider.removeTarget(self, action: #selector(changed), for: .valueChanged)
        emit = nil
    }
}
```

Create the component inside the view controller. Use the constraints from
**Host the component in UIKit** to size the container:

```swift
import CranposeBindings

let component = CranposeView(
    session: createVolume(),
    factories: ["volume": VolumeFactory()]
)
component.onError = { message in print(message) }
view.addSubview(component)
```

The host owns each child. `update` receives the Rust value. Slider events return
to the matching `NativeView` callback. Slot removal calls `dispose`.

### Size and position native children

Use layout modifiers for the slot bounds. The host clips native children to the
component's bounds. Apply rotation, rounded clips and graphics effects inside
the platform control.

Replace `VolumeScreen` with this layout. The slider fills the padded column and
occupies a 56-point row. The Rust state survives the dialog; the native child
leaves composition while the dialog covers the screen:

```rust
use cranpose::widgets::{Dialog, DialogSpec, PopupHost};

#[composable]
fn VolumeScreen() {
    let volume = rememberMutableStateOf(|| 50_u32);
    let show_help = rememberMutableStateOf(|| false);

    PopupHost(move || {
        Column(
            Modifier::empty().fill_max_size().padding(24.0),
            ColumnSpec::default()
                .vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
            move || {
                Text(
                    format!("Volume: {}", volume.get()),
                    Modifier::empty(),
                    TextStyle::default(),
                );
                if show_help.get() {
                    Spacer(Modifier::empty().height(56.0));
                } else {
                    NativeVolume(
                        volume,
                        Modifier::empty().fill_max_width().height(56.0),
                    );
                }
                Button(
                    Modifier::empty(),
                    ButtonSpec::default(),
                    move || show_help.set(true),
                    || {
                        Text("Volume help", Modifier::empty(), TextStyle::default());
                    },
                );
            },
        );
        if show_help.get() {
            Dialog(
                DialogSpec::default(),
                move |_| show_help.set(false),
                || {
                    Text(
                        "Move the slider to choose a volume from 0 to 100.",
                        Modifier::empty().padding(24.0),
                        TextStyle::default(),
                    );
                },
            );
        }
    });
}
```

Native children draw above Cranpose content. The conditional slot keeps the native
slider clear of the Cranpose dialog. Dismiss the dialog through the scrim or back
action; the factory creates a slider with the saved Rust value.
