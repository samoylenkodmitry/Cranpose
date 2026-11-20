# cranpose-xr

Cranpose composables for headsets. An app writes a `Subspace` of `SpatialPanel`s with the same
composables as on every other platform:

```rust,ignore
use cranpose::prelude::*;
use cranpose_xr::{SpatialPanel, Subspace, SubspaceModifier};

#[composable]
fn App() {
    Subspace(|| {
        SpatialPanel(SubspaceModifier::new().size(800.0, 500.0), || {
            Text("Hello from a headset", Modifier::empty(), TextStyle::default());
        });
    });
}

cranpose_xr::android_main! {
    launcher: cranpose::AppLauncher::new(),
    content: App,
}
```

In a headset each panel is a layer of the headset's compositor, which samples the panel's
pixels once at the display's resolution, so its text stays sharp. A dp is a millimeter in the
room. The controllers point at the panels, and their triggers press. Outside a headset the
panels draw flat, side by side.

A panel or a surface with `SubspaceModifier::movable()` has a handle under it: a sphere that shows
from any side. A controller grabs it from any side and carries the panel.
`movable_with(MovePolicy::system(...))` calls back with a `SpatialMoveEvent` for the grab
(`Start`), each change of the pose (`Moving`) and the release (`End`). The event's pose is in
meters, relative to the subspace's origin. An app that saves it places the panel there at its next
start with `SubspaceModifier::with_pose`.

```rust,ignore
let pose = saved_pose.unwrap_or(Pose::IDENTITY);
SpatialPanel(
    SubspaceModifier::new()
        .size(800.0, 500.0)
        .with_pose(pose)
        .movable_with(MovePolicy::system(move |event| {
            if event.event_type == SpatialMoveEventType::End {
                save(event.pose);
            }
        })),
    || {},
);
```

`MovePolicy::custom` leaves the pose to the app: the headset reports the steps, and the app feeds
them into the panel's modifier.

A panel or a surface with `SubspaceModifier::resizable_with(limits, policy)` has a second handle at
its bottom right corner, an amber sphere. A controller grabs it, and the panel's size follows the
point where the ray meets the panel's plane while the trigger stays down. The center stays where it
is, so the pose does not change. The app owns the size, as with `MovePolicy::custom`. The policy
receives a `SpatialResizeEvent` for the grab (`Start`), each new size (`Resizing`) and the release
(`End`), with the size in dp. The app sets the state that feeds `SubspaceModifier::size`.
`ResizeLimits` sets the smallest size, the largest size and `maintain_aspect_ratio`, which keeps the
proportions of the size at the grab. A video surface usually keeps them, because its picture fills
the whole surface. The modifier composes with `movable`.

```rust,ignore
let size = rememberMutableStateOf(|| [800.0_f32, 500.0]);
let [width, height] = size.get();
SpatialPanel(
    SubspaceModifier::new()
        .size(width, height)
        .movable()
        .resizable_with(
            ResizeLimits::new()
                .minimum_size(400.0, 300.0)
                .maximum_size(1600.0, 1200.0),
            ResizePolicy::custom(move |event| size.set(event.size)),
        ),
    || {},
);
```

A panel inside a `SpatialCurvedRow` has no corner handle.

A `Subspace` stands a meter ahead of where the head was when the app started. A
`FollowingSubspace(FollowTarget::View(FollowMode::Snap), ...)` stands a meter ahead of the head as
it is when the subspace first composes, and again after the user recenters. Its panels stay
movable. Jetpack XR spells this `Subspace(follow = FollowTarget.view(FollowMode.snap()))`. Rust has
no overloads, so the composable has its own name.

`FollowMode::Tight` follows the target in every frame with no lag, as Jetpack XR's
`FollowMode.tight` does. The follow moves the panels' poses in the frame loop and composes nothing.
Jetpack XR's `TrackedDimensions` chooses the tracked axes and tracks all six by default. Here the
subspace tracks the target's place and its turn about the vertical axis, so the panels stand
upright. `FollowMode.soft` is not available.

`FollowTarget::Controller(hand, mode)` follows the controller in a hand. Jetpack XR has no such
target. The origin is where the controller's grip pose is. It turns about the vertical axis with
the heading of the controller's aim pose, which is the direction of its ray seen from above. A ball
10 cm above the controller stays straight above it when the hand tilts. A ray that points straight
up keeps the heading that it had. While the runtime does not track the controller, the subspace's
panels do not show and take no hits. A controller's own ray ignores the panels that ride on it. The
other hand's ray presses them. The user can show or hide the app's panels with a ball above the
left controller that the right ray presses:

```rust
FollowingSubspace(FollowTarget::Controller(Hand::Left, FollowMode::Tight), move || {
    SpatialPanel(SubspaceModifier::new().size(40.0, 40.0).offset(0.0, 100.0, 0.0), move || {
        Box(
            Modifier::empty()
                .fill_max_size()
                .background(Color(0.2, 0.6, 0.45, 1.0))
                .rounded_corners(20.0)
                .clickable(move |_| panels_shown.set(!panels_shown.get())),
            BoxSpec::default(),
            || {},
        );
    });
});
```

A panel that has rounded corners and no background around them shows as that shape. The headset
draws the panels with premultiplied alpha. A 40 by 40 dp panel with `rounded_corners(20.0)` is a
circle.

A `SpatialCurvedRow` bends the panels inside it around the viewer, as a cylinder layer of the
compositor. `local_spatial_environment()` gives the app the room around its panels: where the
headset has passthrough, `set_preferred_passthrough_opacity(Some(1.0))` shows it. Check
`local_spatial_capabilities().current().is_passthrough_control_enabled()` first.

## 3D content

`SpatialVolume` puts 3D content in a `Subspace`. Its content is ordinary composables whose nodes
carry `mesh_surface`. Cranpose's own 3D scene draws them, so a mesh shows live composable content on
its surface:

```rust,ignore
use cranpose::space::{Light, Mat4, Material, Mesh, MeshSurface, Vec3};
use cranpose_xr::{SpatialVolume, SubspaceModifier};

SpatialVolume(
    SubspaceModifier::new().size(300.0, 300.0).depth(300.0).movable(),
    move |scope| {
        scope.light(Light {
            direction: Vec3::new(0.45, 0.6, 0.65),
            ambient: 0.3,
            shadows: false,
            size: 0.0,
        });
        let mesh = Arc::clone(&torus);
        Box(
            Modifier::empty()
                .required_size(Size::new(512.0, 128.0))
                .mesh_surface(move || MeshSurface {
                    mesh: Arc::clone(&mesh),
                    transform: Mat4::scaling(Vec3::new(100.0, 100.0, 100.0)),
                    material: Material::Lit,
                    casts_shadow: false,
                    solid: true,
                })
                .background(Color(0.95, 0.55, 0.15, 1.0)),
            BoxSpec::default(),
            || {},
        );
    },
);
```

The modifier's `width`, `height` and `depth` give a box in dp, which are millimeters, 500 each by
default. The content's space has its origin at the box's center, x to the right, y down and z away
from the viewer, and a unit is a dp. The modifier's `offset`, `rotate`, `movable` and `resizable`
work as on a panel.

A controller's or a hand's ray meets the content's meshes on the CPU. It stops at the nearest
triangle of any mesh in the volume, from either side, and passes through the empty part of the box.
A volume with no mesh stops the ray at its box. `scope.on_input_event` receives an `InputEvent`, as
Jetpack XR's `InteractableComponent` gives its listener: the ray in the content's space, an
`InputAction` (`HoverEnter`, `HoverMove`, `HoverExit`, `Down`, `Move`, `Up`) and a `HitInfo` with
the node of the content whose mesh the ray meets (`input_entity`), which of its meshes (`mesh_index`),
the place of the hit in dp (`hit_position`) and the triangle. A press names the mesh that it began on
until the release. `SpatialGltfModelState::node_of(&hit)` names the glTF node of a hit on a model.
`scope.on_pointer_event` still receives Compose's pointer events, with the position in dp on the
box's front face. The meshes are those that the last draw left in the scene, so a mesh that moves in
a frame is hit where it stood in the frame before.

```rust,ignore
SpatialVolume(modifier, move |scope| {
    scope.on_input_event(move |event| {
        if let (InputAction::Down, Some(hit)) = (event.action, event.hit_info) {
            log::info!("pressed node {:?} at {:?}", hit.input_entity, hit.hit_position);
        }
    });
    // ...the meshes
});
```

Jetpack XR 1.0 shows 3D content with `SpatialGltfModel` for a glTF model and `SceneCoreEntity` for a
SceneCore entity. Cranpose has no SceneCore entities, so its composable has its own name. It has a
`SpatialGltfModel` too (see "glTF models").

A headset composes the content once and draws it for each eye, from that eye's pose and field of
view, into one projection layer for all volumes. Where the runtime offers a depth swapchain, the
layer carries the scene's depth (`XR_KHR_composition_layer_depth`), and where it has
`XR_FB_composition_layer_depth_test` the panels and rays over it are set to test against that depth.
A Quest 3 shows the panels behind the 3D content where they stand behind it. Otherwise the
panels cover the 3D content. A frame with no volume has no projection layer and costs
no GPU time. A movable volume takes two of the compositor's layers. A window shows the volume as a
3D view in its box.

The scene takes four samples in each pixel where the device takes four in its colour and depth
formats, so the edges of a mesh show smooth. The renderer averages the samples straight into the
page that the eye's image is converted from, and a second pass with one sample leaves the layer's
depth. A pixel that an edge covers in part carries the depth of the surface at its centre. A sheet
made with `Mesh::plane` already smooths its outline, so the gain shows on the edges of solids. The
`docs/xr_architecture.md` file has the path and the measurements.

## glTF models

The `gltf` feature adds `SpatialGltfModel`, which shows a glTF 2.0 model in a volume, after Jetpack XR's
composable of the same name. An app without the feature has none of the code and none of its
dependencies (`gltf`, `image`, `base64` and `percent-encoding`).

```rust,ignore
use cranpose_xr::{
    SpatialGltfModel, SpatialGltfModelSource, SpatialGltfModelStatus, SubspaceModifier,
    rememberSpatialGltfModelState,
};

// A source made from bytes is a new source each time, so the app keeps it
let source = remember(|| SpatialGltfModelSource::from_bytes(MODEL_GLB)).with(Clone::clone);
let state = rememberSpatialGltfModelState(source);
SpatialGltfModel(
    state.clone(),
    SubspaceModifier::new().size(300.0, 300.0).depth(300.0),
    |_| {},
);
// The animations exist once the model has loaded. An effect keyed by the status starts them
// once, where a call in the composable's body would start them again at each composition.
let status = state.status();
LaunchedEffect(status.clone(), async move |_| {
    if status == SpatialGltfModelStatus::Loaded {
        for animation in state.animations() {
            animation.set_looping(true);
            animation.start();
        }
    }
});
```

A source is a file (`.glb`, or `.gltf` with embedded buffers or files beside it) or bytes. The model
loads on a worker thread. `state.status()` is `Loading`, `Loaded` or `Failed(GltfError)`, and the
composable shows nothing until it is `Loaded`. The model keeps its proportions and scales to fit the
box that the modifier sets, as in Jetpack XR. A side that the modifier leaves out never makes the
model larger than its own size. With no size at all the model has its own size, a meter for each
glTF unit.

The scene shows the node tree of the file's default scene with each node's transform. Each mesh
carries its material's base color, which is the base color texture times the base color factor. The
`OPAQUE` and `MASK` alpha modes work, and `BLEND` keeps the alpha of the base color. The scene has
one directional light for all volumes. The model sets a default light, and the `content` callback
may set another with `scope.light`.

`GltfAnimation` (Jetpack XR's `GltfAnimation`) plays the translation, rotation and scale channels
of a node, with the `LINEAR`, `STEP` and `CUBICSPLINE` interpolations: `start`, `stop`, `pause` and
`resume`, a loop and a speed. Skins and morph targets are not animated: a skinned mesh stands in its
bind pose. The loader leaves out the rest of a glTF material (metallic, roughness, normal,
occlusion and emissive values, texture transforms, texture samplers), the file's cameras and lights,
vertex colors, and primitives that are not triangles. A texture clamps at its edges and never
repeats. A texture wider or higher than 2048 pixels is halved until it fits.

Jetpack XR's `SpatialGltfModelState` also lists the model's `nodes`. Cranpose has no counterpart,
and its `SpatialGltfModelSource` has `from_path` and `from_bytes` where Jetpack XR's has `fromPath`,
`fromUri` and `fromResource`. The doc comments say where each call differs.

## Controller models

A headset draws a model at the grip pose of each controller in use. The models stand in the 3D layer
of the volumes, so they share its depth. Where the runtime tests depth (see "3D content"), a panel
and a model hide each other by their distance from the head. Without the test a panel covers a
model wherever it stands. A tracked hand with no controller in it shows no model. A controller that
the runtime does not track shows none either. The models are on by default.

The runtime's own model shows where the runtime has one. A Meta Quest hands out glTF models through
`XR_FB_render_model`. The app declares `Use::controller_models()` in its build script. That writes
the manifest entries which make the Horizon OS runtime list the extension. The app also turns on the
`gltf` feature, which loads the file with the loader of `SpatialGltfModel`. The backend asks the
runtime for the model of each hand before the first frame that it waits for. It asks again when a
controller comes back after the runtime had no model for it. It copies the bytes out, and a worker
thread decodes them, so no frame waits for the file. A hand shows nothing while its model decodes.

```rust,ignore
// build.rs
cranpose_capabilities::declare(&[Use::controller_models()]).emit();
```

The hand shows a plain model that Cranpose builds in code in three cases: the runtime has no model
(Android XR and most desktop runtimes), the app declared nothing, or the model fails to load. The
plain model has a handle, a head, a thumbstick, two buttons, a trigger and a tip that marks the
front. It has no texture and no asset file. It uses the axes of the runtime's models around the
grip pose: x to the right, y up and z toward the user.

`local_controller_models()` gives the app a `ControllerModels` handle. `set_enabled(false)` turns the
models off, so no frame draws a scene for them. `set_model(hand, model)` replaces the model of one
hand with a `ControllerModel`. `Runtime` is the default. `Plain` is the built-in model. `Gltf(source)`
is a model of the app's own, which needs the `gltf` feature. Its origin lies at the grip pose, and
its unit is the meter.

```rust,ignore
use cranpose_xr::{ControllerModel, Hand, SpatialGltfModelSource, local_controller_models};

if let Some(models) = local_controller_models().current() {
    models.set_model(Hand::Left, ControllerModel::Plain);
    models.set_model(
        Hand::Right,
        ControllerModel::Gltf(SpatialGltfModelSource::from_bytes(MY_CONTROLLER)),
    );
    // models.set_enabled(false) shows none
}
```

Jetpack XR has no controller models. Its `Entity.setEnabled` and `isEnabled` name the switch. The
names of the model choices are Cranpose's own.

A model costs the projection layer of the volumes while a controller is in use. An app with no
volume pays for that layer, its swapchains and its pass only then. With the models off, or with no
controller in use, no frame draws the scene. The frame loop moves a model by writing one state that
the draw reads. A controller that moves composes nothing and allocates nothing.

## Spatial audio

`SpatialPointSource(modifier, source)` plays a sound from the pose that `modifier` gives in its
`Subspace`. `SubspaceModifier::point_source(source)` makes a panel or a volume the source of a sound.
That sound comes from the pose of the content and follows it when the user carries it. In each frame
the sound reaches each ear with the level and the delay that the place of the source relative to the
head gives. Cranpose's own panner in the audio mixer computes them, so the runtime has no part in it.

```rust,ignore
use cranpose::{PlaybackParams, default_audio};
use cranpose_xr::{
    DistanceAttenuation, PointSource, PointSourceParams, SpatialPointSource, SpatialVolume,
    Subspace, SubspaceModifier, local_spatial_capabilities,
};

// The app installs the audio engine before it composes: `cranpose::install_audio()`, or the
// `audio` feature of `cranpose` in an Android app. Jetpack XR's `SpatialSoundPool.play` has no
// counterpart for a sound id: the engine's `load` gives it.
let hum = default_audio().load(include_bytes!("hum.wav")).ok();

if local_spatial_capabilities().current().is_spatial_audio_enabled()
    && let Some(sound) = hum
{
    Subspace(move || {
        // A sound 80 cm to the right of where the subspace starts, which fades from 1 m to 8 m
        SpatialPointSource(
            SubspaceModifier::new().offset(800.0, 0.0, 0.0),
            PointSource::new(sound).looping().params(
                PointSourceParams::new().distance_attenuation(DistanceAttenuation::Linear {
                    min_distance: 1.0,
                    max_distance: 8.0,
                    gain_at_max_distance: 0.0,
                }),
            ),
        );
        // A sound at a volume: a controller's ray carries both
        SpatialVolume(
            SubspaceModifier::new()
                .movable()
                .point_source(PointSource::new(sound).looping()
                    .playback(PlaybackParams::new().volume(0.6))),
            |_| { /* the 3D content */ },
        );
    });
}
```

`PointSourceParams` and `DistanceAttenuation` carry the names of Jetpack XR. `PointSource` bundles the
arguments of `SpatialSoundPool.play`: the sound, the params and the `PlaybackParams` with the volume,
the rate and the bus. A flag says whether the sound loops. A sound plays while its source is in the
composition.
Outside a headset the composables play nothing.

The sources sound while the app has input focus. Every other lifecycle state silences them. Those states
occur under a system panel, while the runtime shows none of the app and after the app ends. A paused
sound goes on from where it stopped. The app pauses the sound that does not play from a source.

The model has four parts: a distance gain, a level difference between the ears, a delay between the ears
and a head shadow filter. It has no elevation cue. A source ahead sounds like one behind.
`docs/xr_architecture.md` gives the formulas. It tells why this crate uses its own panner and neither
`android.media.Spatializer` nor `SpatialSoundPool`. It shows how the pose of the head reaches the audio
thread without a lock.

The `audio-tap` feature logs the level of each ear of the mixed output once a second. The line also
holds the place of the first source relative to the head. `scripts/dev/xr_audio_check.sh` reads it on a
Quest.

## Video and camera surfaces

`SpatialExternalSurface` gives the app a surface that a hardware video decoder or a camera
writes into. The headset's compositor shows the pictures directly as a layer, with no copy through
Cranpose. `StereoMode` tells how a picture divides between the eyes.

```rust,ignore
use cranpose_xr::{SpatialExternalSurface, StereoMode, SubspaceModifier};

SpatialExternalSurface(
    StereoMode::SideBySide,
    SubspaceModifier::new().size(960.0, 540.0),
    |scope| {
        scope.on_surface_created(|surface| start_decoder(surface));
        scope.on_surface_destroyed(|_| stop_decoder());
    },
);
```

The callbacks receive an `ExternalSurface` from `cranpose-services`. Outside a headset the
composable shows an empty box, and the callbacks never run.

A controller's ray stops on a surface. `scope.on_pointer_event` gives the app Compose's
`PointerEvent`s for the ray: `Enter`, `Move`, `Down`, `Up` and `Exit`. The `position` is in dp from
the surface's top left corner, in the units of the surface's `size`, so an app that shows a picture
of `w` by `h` pixels finds the picture's pixel at `(x / width * w, y / height * h)`. A streamed
desktop moves its mouse cursor with it.

```rust,ignore
scope.on_pointer_event(|event| match event.kind {
    PointerEventKind::Move | PointerEventKind::Enter => send_mouse_move(event.position),
    PointerEventKind::Down => send_mouse_button(true),
    PointerEventKind::Up => send_mouse_button(false),
    _ => {}
});
```

## Closing the app

`cranpose_services::request_exit()` closes a headset app as it closes a flat one. The backend ends
the OpenXR session, then finishes the activity on Android or returns from `run` on a desktop.

## The headset's state

`SpatialFrameEffect` runs a callback once in each frame of the headset, before the panels draw. It
receives an `XrFrame`: the predicted display time on the monotonic clock, the head's pose, each
eye's pose, pose relative to the head and field of view, the head's linear and angular velocity,
the floor (the pose of the runtime's stage space, `floor_height()`), both controllers with their
poses, velocities, buttons, sticks and touches, and the runtime's recenter event. `local_haptics()` gives
the `Haptics` handle that plays a `Vibration` on a hand's controller. `local_headset_properties()`
gives the headset's name, its eyes' image sizes and its refresh rates. A streaming app sends the
state to its host with them. Outside a headset the callback never runs.

```rust,ignore
use cranpose_xr::{Hand, SpatialFrameEffect, Vibration, local_haptics};

let haptics = local_haptics().current();
SpatialFrameEffect(move |frame| {
    if let Some(pose) = frame.device_pose {
        send_to_host(frame.predicted_display_time, pose);
    }
    if frame.right_controller.primary.pressed
        && let Some(haptics) = &haptics
    {
        haptics.vibrate(Hand::Right, Vibration::new(0.5, Duration::from_millis(30)));
    }
});
```

## Focus and pause

Meta's Virtual Reality Checks for Quest apps need an immersive app to behave in two ways. An app
that has lost input focus, as under the Universal Menu, keeps rendering. It shows no controllers
and takes no input. An app that the system pauses stops its interactive elements. The backend
keeps the first rule itself. While the session is `VISIBLE` and not `FOCUSED`, the panels, the
volumes and the surfaces stay on display. The pointer gets no aim from any source. A press ends,
a carried panel drops and no ray shows. The backend does not stop the app's own work, because the
app knows which of its elements are interactive. It tells the app through the lifecycle that
Compose uses:

| OpenXR session state | `local_lifecycle_state()` | The backend |
|---|---|---|
| `FOCUSED` | `Resumed` | composes, draws and takes input |
| `VISIBLE` | `Paused` | composes and draws, takes no input, shows no ray |
| `READY`, `SYNCHRONIZED` | `Created` until the runtime first shows the app, then `Stopped` | draws nothing and ends frames with no layers |
| `STOPPING`, `IDLE` | `Stopped` | runs no frame until the session begins again |
| `EXITING`, `LOSS_PENDING`, the activity's end | `Destroyed` | leaves the loop |

```rust,ignore
use cranpose::{LifecycleState, local_lifecycle_state};

#[composable]
fn Game() {
    // Composes again on each change of the lifecycle
    let running = local_lifecycle_state().current() == LifecycleState::Resumed;
    if !running {
        PauseMenu();
    }
}
```

`LifecycleEffect` and `observe_lifecycle` of Cranpose serve code that runs outside a composition.
Jetpack XR Compose has no composition local for the session's state, so an XR app reads the
lifecycle that an Android app reads. `XrFrame::focused` gives the same answer in a
`SpatialFrameEffect`. The app keeps its composition and the panels' places through `Stopped`.
`Paused` runs the durable saves that the app registered. The frame loop waits for them for up to
`durable_save_deadline()`.

## Store entitlement

The Meta Horizon Store needs a store app to check at its start that the viewer owns it. The app
must quit when the check fails. The `meta-entitlement` feature of this crate runs Meta's check for an
Android app. It is off by default. With the feature off, nothing of Meta's Platform SDK is linked
or downloaded.

The app turns it on in three places. The Rust package enables the feature. The Gradle build lists
the service. The build's configuration gives the app's id from the Meta Horizon developer
dashboard (My Apps, then Development, then API). The id is never in the code.

```toml
# Cargo.toml of the Android package
cranpose-xr = { version = "0.26", features = ["openxr", "meta-entitlement"] }
```

```kotlin
// build.gradle.kts
cranpose {
    services.addAll("xr", "meta-entitlement")
    metaAppId.set(providers.gradleProperty("cranposeMetaAppId")) // or -PcranposeMetaAppId=<id>
}
```

**Whoever turns the feature on accepts Meta's license.** The `meta-entitlement` service downloads
`com.meta.horizon.platform.ovr:android-platform-sdk` from Maven Central. The SDK is under the
[Meta Platform Technologies SDK License Agreement](https://developers.meta.com/vr/licenses/oculussdk/).
The agreement says that downloading or using the SDK accepts it. It binds the person or the
entity that does so. The repository holds none of the SDK.

The backend starts the check before it connects to the runtime, as Meta asks for the check within
10 seconds of the launch. The Platform SDK initializes with the id of the merged manifest and asks
whether the viewer is entitled. Every failure counts as a failed check: an SDK that does not
initialize, an id that the store does not know, an exception, no answer for 30 seconds and a build
that lacks the Gradle service. A failed check quits the app by default. The backend calls
`request_exit`, so the session ends and the activity finishes. An app that prefers to limit itself,
as a demo does, picks `OnNotEntitled::Continue`. `EntitlementCheck::entitlement` then tells it
`NotEntitled`.

```rust,ignore
use cranpose_xr::{Entitlement, OnNotEntitled, local_entitlement_check};

#[composable]
fn Root() {
    if let Some(check) = local_entitlement_check().current() {
        // Before the answer comes: the first composition is early enough
        remember(move || check.set_on_not_entitled(OnNotEntitled::Continue));
        if check.entitlement() == Entitlement::NotEntitled {
            Text("Buy the app to unlock this", Modifier::empty(), TextStyle::default());
        }
    }
}
```

`local_entitlement_check()` is `None` where no check runs: in a build without the feature, on a
desktop headset and in a window. Jetpack XR has no entitlement API. A build that installs without the
store fails the check, so a test on a headset needs the app's store channel or a test user. The log
names the result: `cranpose xr: entitlement check: NotEntitled (...); the app quits`.

## Hands, body, face and eyes

An app that wants the user's hands, body, face or eyes asks for them in its build script, with the
services of `cranpose::capabilities`, and hands the declaration to its launcher:

```rust,ignore
// build.rs
use cranpose_capabilities::{Use, declare};

fn main() {
    declare(&[Use::hand_tracking(), Use::face_tracking()]).emit();
}

// lib.rs
cranpose::app_capabilities!();

cranpose_xr::android_main! {
    launcher: cranpose::AppLauncher::new().with_capabilities(&CAPABILITIES),
    content: App,
}
```

`Use::hand_tracking()`, `Use::body_tracking()`, `Use::face_tracking()` and `Use::eye_tracking()`
each enable the OpenXR extensions of their kind that the runtime offers, create the trackers once
the user grants the permissions, and add those permissions to the Android manifest. An app that
asks for none enables no extension, creates no tracker, makes no call in a frame and adds no
permission. OpenXR fixes the extensions when the instance is made, before the content composes, so
a modifier or a composable cannot ask. Jetpack XR asks at run time, with `Session.configure`.

Each frame's `XrFrame` then holds fixed-size arrays that the backend fills in place:

| Field | Holds | OpenXR |
|---|---|---|
| `left_hand`, `right_hand`, `hand(Hand)` | a `HandState`: a `TrackingState`, 26 `HandJoint`s at the index of each `HandJointType`, and a `HandAim` with the pinch of each finger | `XR_EXT_hand_tracking`, `XR_FB_hand_tracking_aim` |
| `body` | a `BodyState`: 70 or 84 `BodyJoint`s at the index of each `BodyJointType`, and a confidence | `XR_FB_body_tracking`, `XR_META_body_tracking_full_body` |
| `face` | a `FaceState` with Jetpack XR's names: a weight for each `FaceBlendShapeType`, and a confidence for each `FaceConfidenceRegion` | `XR_ANDROID_face_tracking`, or the shared shapes of `XR_FB_face_tracking2` |
| `meta_face` | a `MetaFaceState` with Meta's names: a weight for each of its 70 `FaceExpression`s, its confidences and its `FaceDataSource`, which is `Audio` on a Quest 3 | `XR_FB_face_tracking2` |
| `eyes` | `Eyes`: an `EyeState` for each eye, and a combined gaze pose | `XR_FB_eye_tracking_social`, `XR_EXT_eye_gaze_interaction` |
| `tracking` | the live `TrackingCapabilities` | |

`local_tracking_capabilities()` gives the `TrackingCapabilities` of the session's start, in the
pattern of `local_spatial_capabilities()`: for each `TrackingFeature` an `availability()` of
`Unavailable`, `Offered`, `AwaitingPermission`, `PermissionRefused` or `Active`, and which extension
supplies what. A feature that the app did not ask for is `Offered` when the runtime lists an
extension for it. Nothing is enabled then, so the list cannot tell whether the headset has the
sensor. A feature that the app asked for gets the system's answer, so a Quest 3 reports its eyes as
`Unavailable`. `XrFrame::tracking` follows the permissions that the user grants.

On a Meta Quest the face and eyes need the user's permission, and a Quest 3 estimates the face from
the microphone, so `Use::face_tracking()` adds `RECORD_AUDIO`. The system's prompt shows before the
session starts, because the system takes an immersive app off the headset to show it and the runtime
ends a session that runs. The app waits for the answer. The backend makes a tracker only when every
permission is granted, and it does not ask again after a refusal.

## Hands as pointers

A tracked hand points at the panels, the curved panels and the volumes, and a pinch presses. The app
asks for it with `Use::hand_tracking()`, as above. Its buttons, sliders and handles take the hand
as they take a controller, because one pointer serves both.

- A controller that the runtime tracks points first. A hand points only on a side with no
  controller in use, so a user can hold a controller in one hand and point with the other.
- The hand's aim pose casts the ray, and the pinch strength of its index finger is the trigger. The
  controller's rules apply to it: a press starts above 0.6 and ends below 0.4, a press holds its
  point while the hand shakes, and it drags once the ray turns. The beam shows from the aim pose.
- On a Meta Quest the aim comes from `XR_FB_hand_tracking_aim` (`HandAim`). A runtime that tracks
  the joints and lacks that extension gives the hand's pointer through the hand interaction profile
  of `XR_EXT_hand_interaction`, whose aim pose and pinch value come as a controller's actions do.
  That profile points only while the cameras track the hand, because a Quest keeps its aim valid
  with no hand in view.
- A hand that makes the system's menu gesture, the palm turned up with a pinch, points at nothing.
  A pinch that is closed when a hand appears does not press either. Both press after the fingers
  open and close again.
- When a controller takes over from a hand, or a hand from a controller, in the middle of a press, the
  press ends with a release. The new input presses after its trigger went up.
- A hand holds no motor, so a vibration that the app asks for on that side is dropped.

`XrFrame::pointer(Hand)` gives the app the `InputSource` that points for a side, `Controller` or
`Hands`, or `None`. `InputSource` is Jetpack XR's `InputEvent.Source`. Jetpack XR tags each input event
with its source. Cranpose tags the frame instead, and the pointer's events reach the composition as
plain pointer events. An app that did not declare hand tracking has no hand pointer.

A build that turns on the `scripted-hand` feature can play a hand in place of the cameras', for a
check where no hand can be tracked, such as an emulator. `script_hand(ScriptedHand::pointing(hand,
from, at, PinchCycle::default()))` installs a hand whose aim pose points at a place in the room and
whose index finger pinches in a cycle. The frame holds it as a tracked hand, so the pointer treats
it as the cameras' hand. The feature is off by default.

## Anchors and planes

An anchor is a place in the room that the runtime holds still. A plane is a wall, a floor, a ceiling
or a table that the runtime found. The names follow Jetpack XR's `androidx.xr.arcore` and its Compose
package. An app asks for them in its build script:

```rust,ignore
// build.rs
use cranpose_capabilities::{Use, declare};

fn main() {
    declare(&[Use::anchors(), Use::plane_tracking()]).emit();
}
```

`Use::anchors()` is Jetpack XR's `Config.anchorPersistence` and enables the runtime's anchors and their
storage. `Use::plane_tracking()` is `Config.planeTracking` and enables the planes. The planes need
the user's permission: the system shows its prompt before the session starts. An app that asks for
neither enables no extension, makes no call in a frame and adds no permission. A Meta Quest holds
anchors through `XR_FB_spatial_entity`, keeps them with `XR_FB_spatial_entity_storage` and reads the room
that the user set up through `XR_FB_scene`. Android XR holds both through `XR_ANDROID_trackables` and
keeps anchors with `XR_ANDROID_device_anchor_persistence`.

```rust,ignore
#[composable]
fn Notes() {
    let (Some(anchors), Some(planes)) = (local_anchors().current(), local_planes().current()) else {
        return;
    };
    let placed = rememberMutableStateOf(|| None::<Anchor>);
    // A panel that the user carries snaps to a table or a wall that it is dropped near, and the
    // app keeps the place where it came to rest
    let keep = anchors.clone();
    let policy = MovePolicy::anchor(&[], &[], move |event| {
        if event.event_type == SpatialMoveEventType::End {
            if let Some(anchor) = keep.create(event.room_pose).into_anchor() {
                anchor.persist();
                placed.set(Some(anchor));
            }
        }
    });
    // The next start loads what the runtime kept, once the list has come
    let kept = anchors.persisted_uuids().last().copied();
    let loader = anchors.clone();
    DisposableEffect(kept, move |_| {
        if let (None, Some(uuid)) = (placed.get_non_reactive(), kept) {
            placed.set(loader.load(uuid).into_anchor());
        }
        DisposableEffectResult::new(|| {})
    });
    match placed.get() {
        Some(anchor) => FollowingSubspace(FollowTarget::Anchor(anchor, FollowMode::Tight), move || {
            SpatialPanel(SubspaceModifier::new().movable_with(policy.clone()), || {});
        }),
        None => Subspace(move || {
            SpatialPanel(SubspaceModifier::new().movable_with(policy.clone()), || {});
        }),
    }
    for plane in planes.all() {
        Text(format!("{:?}", plane.state().label), Modifier::empty(), TextStyle::default());
    }
}
```

`Anchors::create(pose)` and `Plane::create_anchor(pose)` take a pose in the app's room, the OpenXR
LOCAL space that `XrFrame` describes. `SpatialMoveEvent::room_pose` gives the pose of a carried panel
in that space. An anchor starts `Paused` at the pose that the app gave. The runtime holds it from the next
frame on, and the state becomes `Tracking`. `Anchor::persist()` asks the runtime to keep it, and
`Anchor::persistence()` goes from `Persisting` to `Persisted(uuid)`. `Anchors::persisted_uuids()` lists what
the runtime kept for the app, and `Anchors::load(uuid)` gives an anchor that stands where the runtime
finds the place again. A runtime needs some frames after the start to read its storage and to
recognize the room, so an app reads the list in a composition, which composes again when the list
changes. The room's origin is where the head was at each start, so the pose of a loaded anchor in the
room differs from one start to the next while the anchor stays at one place in the world.

`FollowTarget::Anchor(anchor, mode)` holds the panels of a `FollowingSubspace` to an anchor with all six
degrees of freedom. The panels do not show until the runtime places the anchor. A detached or lost anchor
leaves them where they were. `MovePolicy::anchor(orientations, semantics, on_move)` is Jetpack Compose
XR's `MovePolicy.anchor`: a panel or a volume that the user carries comes to rest on the plane that it
is near, when the two lists admit the plane. Its +Z axis then points out of the plane, so a panel lies
flat along it, as in Jetpack XR. `Anchors::capabilities()` and `Planes::availability()` say what
the runtime offers and how far each part has come. A pose that the runtime refines in a frame writes no
state and allocates nothing. A composition that reads `Anchor::state()` composes again when the
tracking state changes, not when the pose moves.

## Text entry

A text field asks for the platform's software keyboard when it takes focus, as it does on a phone.
The options of `BasicTextField` hold the request (`show_keyboard_on_focus`), and a tap on a focused
field repeats it. A headset answers with one of two keyboards. An app needs no code for it.

The keyboard that Cranpose draws is a `SpatialPanel` of keys. It opens below the panel that holds
the focused field, in the plane of that panel, and it closes when focus leaves the field. It follows
the panel: a panel that the user carries, or that follows a controller, takes the keyboard along.
A controller's ray and a hand's pinch press the keys, because the keys are ordinary buttons.
A key does not take the focus from the field.

- The keys are the digits, the 26 letters in the QWERTY order, shift, backspace, enter, space, the
  comma and the period, a key that shows a page of symbols (`?123`) and back (`abc`), and a key that
  hides the keyboard. The symbols page holds `@ # $ % & * ( ) - + ! " ' : ; / ? = _ [ ] { } < > \ |`.
- Each key sends the key event that a hardware keyboard sends for it (`KeyEvent`, with the `Key`
  and the text that the US layout gives), and the backend hands it to the shell after the pointer's
  events of the frame. The text reaches the field through the path of a hardware keyboard, so the
  `on_key_event` modifiers of the app see it too. A letter in a capital carries the shift modifier.
- One press of shift makes the next letter a capital. A second press locks shift, and a third
  unlocks it.
- Enter runs the keyboard action of a single-line field (`on_keyboard_action`). Without one the
  action is Done, which hides the keyboard. In a multi-line field enter starts a new line.
- The hide key closes the keyboard and leaves the focus in the field. A tap on the field opens the
  keyboard again.
- The keyboard has the Latin alphabet only. It has no other layout, no suggestions, no composing
  text and no repeat of a held key.
- The open keyboard is one more layer of the frame. A runtime takes 16 layers at least, so an app
  that stands near the limit loses the panel that stands farthest from the head, as with any other
  panel.

On Android XR the system shows its own input method over an immersive app. A text field then opens
that keyboard in place of the drawn one, through the same request. The keyboard is a panel of the
system. The system gives it the input focus, so the app's lifecycle is `Paused` while it shows, and
the app's pointer gets no aim. The back action hides the keyboard, and the app resumes. What the
keyboard types reaches the field as the text of an input connection, so the field takes
suggestions, other languages and voice input from the keyboard. Android XR shows the keyboard only
for an app whose activity has an editor view, which the backend adds.

`use_keyboard(Keyboard::Drawn)` draws the keyboard of Cranpose on every runtime, also where the
system shows its own. `Keyboard::System`, the default, uses the system's keyboard where the runtime
shows it and draws the keyboard elsewhere. The choice counts if it comes before the app starts,
in the launcher's setup. Jetpack XR Compose has no such choice. There the system's input method
types, and Compose opens it as it does on a phone.

```rust,ignore
cranpose_xr::use_keyboard(cranpose_xr::Keyboard::Drawn);
```

A build that turns on the `scripted-hand` feature can type on the keyboard of Cranpose with
`ScriptedHand::typing`. The hand presses a field, and then the keys that type a text, one press in
each cycle. It shows the keyboard in an emulator, where no hand can be tracked.

Meta's own keyboard for Quest apps, `XR_META_virtual_keyboard`, is not used. It needs the app to
draw the keyboard's model, which the runtime hands over as glTF through `XR_FB_render_model`.
`docs/xr_architecture.md` lists what it needs.

## Streamed scenes

`StreamedScene` shows frames that another machine rendered for both eyes. The app decodes the
stream with a `VideoDecoder` in frames mode, and says for each frame where each eye was and which
part of the frame is its picture. The headset's compositor shows each picture from the pose it was
rendered for, and corrects it for the head's newer pose.

The host stamps each frame with the time at which it wants the frame on the display
(`StreamedViews::target_display_time`, on the clock of `XrFrame::predicted_display_time`). The
headset keeps up to three decoded frames and shows the one whose time lies closest to the display
time of the frame that it submits. Without times it shows the newest frame. A `StreamStats` handle
counts the frames shown, skipped and repeated. The decoder's `depth` must be at least 5. A host
that encodes each eye with full density in the center and compressed edges describes the encoding in
`StreamedView::foveation`, and the headset unwarps it, so the compositor shows the full eye.

```rust,ignore
use cranpose_xr::{StreamStats, StreamedScene, StreamedViews};

let stats = StreamStats::new();
StreamedScene(frames, |metadata: u64| -> StreamedViews { views_of_frame(metadata) }, stats.clone());
// Any thread: stats.counts() gives the shown, skipped and repeated frames.
```

Outside a headset the scene fills the box that the layout gives it with the left eye's picture of
the newest frame, fitted to the box, and shows that picture again while no newer frame comes. A
desktop window of `run` on macOS does this, and so does the screen of a phone or a tablet. The
scene takes the left eye's rectangle from `StreamedView::rect` and ignores the rest of the views.
It counts the frames shown and skipped in `StreamStats`, and `repeated` stays 0, because the system
repeats the last picture of a window by itself. A flat scene cannot unwarp a foveated picture: it
shows the picture as the host encoded it and logs that once. A host that streams to a flat viewer
must not foveate. The decoder's `depth` must be at least 6 there. macOS, iOS and Android decode
frames, so a flat scene shows nothing on other platforms. On a phone or a tablet the app starts
with `cranpose::android_main!` and builds without the `openxr` feature, because an app that
starts through this crate's `android_main!` is an immersive app that runs on a headset only.

## Android headsets

The `openxr` feature adds the backend for Android headsets such as the Meta Quest. The app's
Android build turns it on and lists the `xr` service in its Gradle build:

```kotlin
cranpose {
    services.add("xr")
}
```

The service makes the APK an immersive app and packs the Khronos OpenXR loader. The app needs
no other OpenXR code.

## PC headsets

On Linux and Windows the `openxr` feature adds `cranpose_xr::run`, which takes the arguments of
`android_main!`. It starts the app in the headset of the machine's OpenXR runtime (SteamVR,
Monado, Meta Link, Pimax, Varjo) and draws through Vulkan. It opens the flat window of
`AppLauncher::try_run` when the machine has no OpenXR loader, no runtime or no headset.

```rust,ignore
fn main() -> Result<(), cranpose_xr::RunError> {
    cranpose_xr::run(cranpose::AppLauncher::new(), App)
}
```

The app finds the OpenXR loader at run time, so a build needs no loader and a machine without one
gets the flat window. On Linux the loader is `libopenxr_loader.so.1`, which the distribution's
OpenXR package installs. On Windows no runtime puts `openxr_loader.dll` on the DLL search path:
the release ships the DLL from the Khronos OpenXR SDK beside the executable. With a logger such as
`env_logger`, `RUST_LOG=cranpose_xr=debug` prints each second's frame count.

Passthrough and curved panels work where the runtime has their extensions. `SpatialExternalSurface`
and `StreamedScene` are Android only: a desktop headset shows a surface panel or a streamed scene
as nothing, until the stream layer has a Vulkan import of the platform's decoded frames. `just xr-smoke` runs the demo
against Monado's simulated headset and its null compositor and prints the frames it ran. It finds
Monado through `XR_MONADO_PREFIX`, the directory that holds `bin/monado-service`, or through
`monado-service` on PATH.

`apps/xr-demo` in the repository is a complete app. The names follow Jetpack XR Compose, and
`docs/xr_architecture.md` describes the design and the steps that follow.
