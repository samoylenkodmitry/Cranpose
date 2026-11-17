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

A panel or a surface with `SubspaceModifier::movable()` has a handle under it, which a controller
grabs and carries. `movable_with(MovePolicy::system(...))` calls back with a `SpatialMoveEvent`
for the grab (`Start`), each change of the pose (`Moving`) and the release (`End`). The event's
pose is in meters, relative to the subspace's origin. An app that saves it places the panel there
at its next start with `SubspaceModifier::with_pose`.

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

A `SpatialCurvedRow` bends the panels inside it around the viewer, as a cylinder layer of the
compositor. `local_spatial_environment()` gives the app the room around its panels: where the
headset has passthrough, `set_preferred_passthrough_opacity(Some(1.0))` shows it. Check
`local_spatial_capabilities().current().is_passthrough_control_enabled()` first.

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

Outside a headset the composable shows nothing.

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
