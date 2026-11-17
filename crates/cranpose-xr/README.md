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

## The headset's state

`SpatialFrameEffect` runs a callback once in each frame of the headset, before the panels draw. It
receives an `XrFrame`: the predicted display time on the monotonic clock, the head's pose, each
eye's pose and field of view, both controllers with their poses, velocities, buttons, sticks and
touches, and the runtime's recenter event. `local_haptics()` gives the `Haptics` handle that plays
a `Vibration` on a hand's controller. A streaming app sends the state to its host with them.
Outside a headset the callback never runs.

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

`apps/xr-demo` in the repository is a complete app. The names follow Jetpack XR Compose, and
`docs/xr_architecture.md` describes the design and the steps that follow.
