# Cranpose XR

Cranpose apps should run in headsets with the same composable API as on every other platform
([tracking issue](https://github.com/samoylenkodmitry/Cranpose/issues/2242)).
A developer writes a `Subspace` of panels and 3D content. Cranpose turns it into what the
headset's runtime takes. The first target is the Meta Quest through OpenXR. The app goes to the
Meta Horizon Store as an ordinary APK. Other OpenXR runtimes follow: SteamVR, Pico and Android
XR. The developer never touches OpenXR, a runtime's extensions or a streaming library.

## Why the picture stays sharp

A headset's compositor draws the displays. It corrects the lenses and moves each layer to the
newest head pose. Everything an app shows reaches the display through one of its layers.

A picture drawn inside a 3D scene is sampled twice: into the eye picture, then again by the
compositor. The eye picture that runtimes recommend has fewer pixels than the display: 1680 by
1760 per eye on a Quest 3, whose display has 2064 by 2208. Text in a 3D scene is therefore soft
in every engine.

A layer of its own is sampled once, by the compositor, at the display's resolution, with its
supersampling and sharpening. So Cranpose gives each kind of content the layer that keeps it
sharp. Each composable maps to one compositor primitive:

| Composable | Compositor primitive | OpenXR |
|---|---|---|
| `SpatialPanel`: a flat panel of 2D Cranpose content | quad layer | core |
| `SpatialCurvedRow`: panels on a curve | cylinder layer for each panel | `XR_KHR_composition_layer_cylinder` |
| `SpatialExternalSurface`: a surface for a video decoder or another engine | swapchain backed by an Android `Surface` | `XR_KHR_android_surface_swapchain` |
| 3D content: `scene3d` and `mesh_surface` | projection layer with depth | core, `XR_KHR_composition_layer_depth` |

A panel draws only when its content changes. The compositor shows the last image of a layer
until a new one comes, so a still panel costs no GPU time at all.

The cranvr Quest client proved each primitive on a Quest 3 by hand. Its desktop is a surface
swapchain fed straight by the hardware decoder. Its console is a quad layer of Cranpose content.
Its controllers, rays and handles are a Cranpose 3D scene in a projection layer.

## The API

Names follow Jetpack XR Compose (`androidx.xr.compose`), so Google's documentation serves
Cranpose developers too:

- `Subspace { ... }`: the 3D area of an app. Outside a headset it shows as a 3D view in the
  window.
- `SpatialPanel(modifier, shape) { ... }`: 2D Cranpose content on a flat panel.
- `SpatialRow`, `SpatialColumn`, `SpatialBox`, `SpatialCurvedRow`, `SpatialSpacer`: layouts in
  meters.
- `SpatialExternalSurface(stereo_mode, modifier) { scope }`: hands the app a surface that a
  decoder writes into. The scope's `on_surface_created` and `on_surface_destroyed` receive an
  `ExternalSurface`.
- `StreamedScene(frames, views)`: frames that another machine rendered, shown as a projection
  layer. Jetpack XR has no such composable. It has no `Spatial` prefix, because it is not placed
  in a `Subspace`: it fills the viewer's view.
- `SpatialCurvedRow(modifier, curve_radius) { ... }`: panels bent around the viewer at
  `curve_radius` dp. Each panel is a cylinder layer, and a controller's ray follows its curve.
  A runtime without `XR_KHR_composition_layer_cylinder` shows the panels flat.
- `local_spatial_capabilities()` (Jetpack XR's `LocalSpatialCapabilities`):
  `is_spatial_ui_enabled()` and `is_passthrough_control_enabled()`.
- `local_spatial_environment()`: the `SpatialEnvironment` with `preferred_passthrough_opacity`
  and `current_passthrough_opacity`, which control the room around the panels.
- `SpatialFrameEffect(on_frame)`: runs a callback once in each frame of the headset with an
  `XrFrame`, which holds the head, the eyes and the controllers. See "The headset's state".
- `local_haptics()`: the `Haptics` handle that vibrates a hand's controller.
- `Orbiter`: content attached to the edge of a panel, such as a handle or a toolbar.
- `SubspaceModifier`: `width`, `height`, `depth`, `size`, `offset`, `rotate`, `movable`,
  `resizable`, `fill_max_size`, `padding`.
- `SpatialRoundedCornerShape`: the shape of a panel.

Jetpack XR Compose shows 3D models through SceneCore entities. Cranpose has its own 3D: a
`scene3d` box and `mesh_surface` content. In a `Subspace` such content draws into the
projection layer, with a camera made from each eye's view.

## The room around the panels

An app asks for the room through `SpatialEnvironment` when `is_passthrough_control_enabled()` is
on. The backend picks how the runtime shows it:

- `XR_FB_passthrough` (Meta Quest): a passthrough layer under all other layers. The backend
  makes and starts it once. It joins a frame while the opacity is above 0, and the opacity is
  the layer's style.
- The blend mode `ALPHA_BLEND` (Android XR, Pico, Magic Leap): the frame's blend mode is
  `ALPHA_BLEND` while the opacity is above 0 and `OPAQUE` otherwise. This path has no dimming.
- Neither: the capability is off and the app's own background shows.

## External surfaces

`SpatialExternalSurface` makes a surface swapchain with `XR_KHR_android_surface_swapchain`. The
compositor shows what a producer posts into the surface as a quad layer at the composable's place
and size. Cranpose never copies the pictures, and the frame loop does not acquire or release the
swapchain's images. `StereoMode::SideBySide` and `StereoMode::TopBottom` make one layer for each
eye, each with its half of the picture.

The app receives the surface as an `ExternalSurface` of `cranpose-services`. The type sits
there so that a service that produces pictures, such as a hardware video decoder, takes it
without a dependency on `cranpose-xr`. On Android the type holds a reference to the surface's
`ANativeWindow`, which `as_ptr` returns. The app needs no `ndk`, JNI or GL code of its own.
Outside a headset the composable shows an empty box, and the callbacks never run.

## Video decoding

`VideoDecoder` of `cranpose-services` decodes H.264, HEVC and AV1 on the headset's hardware. On
Android it drives `MediaCodec` with the keys for the least delay: no reordering wait, priority
0 and the top operating rate. The app feeds access units from one thread. A thread of the
decoder drains the pictures, and no thread sleeps. Each picture returns with the timestamp and
the `u64` that the app gave its access unit, and with the decode time.

The app chooses where the pictures go:

- `DecoderOutput::Surface(ExternalSurface)`: the pictures go straight into the surface of a
  `SpatialExternalSurface`. The compositor reads them with no copy. The desktop stream and
  video players use this output.
- `DecoderOutput::Frames { depth }`: each picture returns as a `VideoFrame`, which is an
  `AHardwareBuffer` that an `AImageReader` of the private format and the GPU-sampled usage
  holds. The headset's backend imports the buffer as an EGL image through
  `VideoFrame::as_hardware_buffer_ptr`, and applies `VideoFrame::crop`. A frame that drops gives
  its image back to the decoder.

On macOS and iOS the decoder drives a `VTDecompressionSession` of VideoToolbox. It decodes H.264
and HEVC, takes `DecoderOutput::Frames` only, and reports AV1 as `DecoderError::NoDecoder`. The
backend reads the parameter sets from the Annex-B stream into a `CMVideoFormatDescription`, and
builds the session at the first keyframe. It builds the session again when a keyframe brings
other parameter sets, or when the system ends the session, as iOS does when an app goes to the
background. Until a keyframe arrives, `submit` answers `DecoderError::NeedsKeyframe`, and the
app asks the sender for one. The session decodes in real time, with no reordering wait. Each
picture is an IOSurface-backed `CVPixelBuffer` in NV12, which is the hardware decoder's own
format: a request for BGRA made VideoToolbox convert each picture, and a picture of 2560 by 1440
pixels took 2.0 to 2.7 ms instead of 1.0 to 1.2 ms. `VideoFrame::as_pixel_buffer_ptr` gives the
buffer. The frame holds one reference count of the buffer until it drops, and at most `depth`
pictures are in the queue and in the app's hands together.

An app that waits for pictures awaits `DecodedFrames::wait_for_frame`. The decoder's thread wakes
the task when it queues a picture, so no frame loop polls.

Platforms without a hardware decoder that Cranpose drives report `DecoderError::Unavailable`.

## The headset's state

A streaming app sends the head, the eyes and the controllers to a host that renders the game.
Jetpack XR reaches the head through ARCore (`ArDevice`, `RenderViewpoint`) and has no controller
type, so Cranpose takes the names `ArDevice`'s `devicePose`, `RenderViewpoint` and `FieldOfView`
from it and names the controller types itself.

`SpatialFrameEffect(on_frame)` runs `on_frame` once in each frame of a running session, on the
thread of the frame loop, before the panels draw. A state that it sets shows in the same frame.
Outside a headset it never runs. The callback receives `&XrFrame`, which the backend fills in
place: the frame loop allocates nothing for it. An app that sets a state in each frame draws
the panels that read it in each frame, so the callback sets a state only when a rounded value
changes. An `XrFrame` holds:

- `predicted_display_time`, `now` and `period`. The times are `Duration`s on the platform's
  monotonic clock. `now` is the clock's reading when the backend sampled the frame, so an app sets
  the runtime's times against its own readings. `refresh_rate()` gives the rate that follows from
  `period`. On Android and Linux that is `CLOCK_MONOTONIC`, which `XR_KHR_convert_timespec_time`
  maps the runtime's time to. On Windows it is the performance counter, which
  `XR_KHR_win32_convert_performance_counter_time` maps the runtime's time to. A runtime without
  the extension passes its own time, which Android runtimes count in `CLOCK_MONOTONIC`
  nanoseconds.
- `device_pose`: the head, the OpenXR VIEW space located in the LOCAL space.
- `left_eye` and `right_eye`: a `RenderViewpoint` with the eye's `pose` in LOCAL, its `local_pose`
  relative to the head (Jetpack's `localPose`) and its `FieldOfView`: four angles in radians with
  OpenXR's signs (left and down are negative). They come from `xrLocateViews`, which fills a fixed
  array on the stack. `interpupillary_distance()` gives the
  distance between the eyes.
- `left_controller` and `right_controller`: a `Controller` with the aim pose and the grip pose,
  each with linear and angular velocity when the runtime gives them, the trigger, squeeze,
  thumbstick, face buttons `primary` (A or X) and `secondary` (B or Y), the menu button, the touch
  states and the active `InteractionProfile`. An input that a profile lacks stays at rest. While
  the session has no input focus every controller is at rest, and `focused` is `false`.
- `recenter`: the runtime's `ReferenceSpaceChangePending` event for the LOCAL space, for one
  frame. Its `change_time` tells from which frame on the poses use the new origin. A streaming app
  tells its host.

`local_headset_properties()` gives the `HeadsetProperties` that the runtime reports once for the
session: the system name, each eye's recommended and maximum image size
(`xrEnumerateViewConfigurationViews`) and the refresh rates that `XR_FB_display_refresh_rate`
offers.

The action set binds the trigger, squeeze, stick, buttons, touches, grip pose and haptic output
of each interaction profile from the OpenXR specification's profile sections. A runtime rejects a
whole profile when one path is wrong, so a profile whose full list is rejected binds the aim,
the trigger and the stick's forward axis, which the pointer uses. The log says at the start which
profiles bound which set.

`Haptics` takes a vibration for a hand: amplitude from 0 to 1, an optional frequency and a
duration. It is `Send` and `Sync`, so a network thread plays what the host sends as it arrives.
The frame loop applies the newest request of each hand after the frame effects, with
`xrApplyHapticFeedback`.

## Flat video

`PlayerSurface(frames, modifier, content_scale)` of `cranpose` draws the pictures of a decoder in
a normal layout, such as a window on a Mac or a phone's screen. Media3 has `PlayerSurface` and
`ContentFrame`. Both take a `Player`, which Cranpose does not have, so the composable takes the
`DecodedFrames` of the decoder that the app feeds. The `content_scale` is the parameter of
`ContentFrame` and of `Image`.

On macOS and iOS the surface draws through the renderer's Metal device. For each picture it
makes two Metal textures of the picture's IOSurface with `newTextureWithDescriptor:iosurface:plane:`:
the luma plane as `R8Unorm`, and the blue and red differences as `RG8Unorm` at half the size.
`wgpu::hal::metal::Device::texture_from_raw` and `create_texture_from_hal` turn them into wgpu
textures. One render pass converts them to RGB in a texture of an `ExternalFrames` ring, which
`Image` draws as it draws any other bitmap. The pass reads the picture's range, which the pixel
format tells, and its color matrix, which the picture's attachment names: BT.601, BT.709 or
BT.2020. The shader samples the differences with a linear filter, and writes the picture's
encoded values. The decoder's buffer is the only source. No copy goes through the CPU.

The wgpu textures hold the `VideoFrame` in their drop callback, so the decoder's pool does not
write the IOSurface before the GPU has read it. The surface waits on `wait_for_frame`, takes the
newest picture in the frame that follows the output, and drops the older ones. A frame with no
new picture does no work.

Android draws nothing yet. The pictures there are `AHardwareBuffer`s of a private format, which a
renderer on Vulkan reads through a sampler with a YCbCr conversion. The import needs these steps:

1. The device is created with `VK_ANDROID_external_memory_android_hardware_buffer`,
   `VK_KHR_sampler_ycbcr_conversion`, `VK_KHR_external_memory`, `VK_KHR_dedicated_allocation`,
   `VK_KHR_bind_memory2`, `VK_KHR_get_memory_requirements2` and `VK_EXT_queue_family_foreign`.
   `GpuRequirements` asks for wgpu features only, so the Android device setup needs a list of
   extensions of its own.
2. `vkGetAndroidHardwareBufferPropertiesANDROID` returns the buffer's memory requirements and its
   `VkAndroidHardwareBufferFormatPropertiesANDROID`. The format of a private buffer is
   `VK_FORMAT_UNDEFINED` with an `externalFormat`, and the properties suggest the model, the range
   and the chroma offsets.
3. A `VkSamplerYcbcrConversion` takes the external format and the suggested values. An immutable
   `VkSampler` takes the conversion, and the descriptor set layout holds it as an immutable
   sampler. The sampler's filters are nearest unless the format supports a linear chroma filter.
4. A `VkImage` is made with `VkExternalMemoryImageCreateInfo` and `VkExternalFormatANDROID`.
   Its memory is imported with `VkImportAndroidHardwareBufferInfoANDROID` and a dedicated
   allocation. The image view carries `VkSamplerYcbcrConversionInfo`.
5. The first use acquires the image from `VK_QUEUE_FAMILY_FOREIGN_EXT` with a barrier to the
   shader read layout.
6. The frame stays alive until the GPU has read it, as on Apple, and the `AImageReader` of the
   decoder has `depth` plus the images in flight.

wgpu has no binding for an immutable sampler with a conversion, and no texture format for an
external one. A pass of the Vulkan backend itself would sample the image and write RGB into a
texture of the `ExternalFrames` ring: the same shape as the Apple pass. The work is about 700
lines of Vulkan through `wgpu::hal::vulkan` and `ash`, and it needs a phone to test. A phone
without the extension needs the GLES path of `cranpose-xr`, which binds the buffer as an EGL
image.

## Streamed scenes

A game that a computer renders for both eyes arrives as a stream of frames, and each frame
belongs to the head pose that it was rendered for. `StreamedScene(frames, views)` takes the
`DecodedFrames` of a `VideoDecoder` in frames mode. `views` maps the `u64` that the app submitted
with an access unit to a `StreamedViews`: for each eye, the render pose in the app's room, the
`FieldOfView` and the rectangle of the eye's picture in the frame. A `RenderViewpoint` of
`XrFrame` carries the same `FieldOfView`, so an app passes the field of view that the headset
reports straight into a streamed view.

At each of the headset's frames the backend takes the newest decoded frame. It imports the
frame's `AHardwareBuffer` as an EGL image, binds it to an external texture, and draws each eye's
rectangle into a swapchain of that eye's size. One pass of a small shader per eye does it, and
the external sampler turns the decoder's YUV into RGB. The frame's two eyes become the views of
an `XrCompositionLayerProjection`, with the poses and fields of view of the frame. The compositor
reprojects each picture from the pose it was rendered for to the head's newer pose. While no
newer frame comes, the layer shows its last pictures again. The scene lies over the room and under
the curved panels, the panels, the surfaces, the handles and the rays.

The backend keeps the drawn frame until the next one is drawn, because the GPU may still read its
buffer. The swapchains have the `SRGB8_ALPHA8` format, and the shader turns the picture's encoded
values into linear light, so that the swapchain keeps the picture's own bytes. A plain `RGBA8`
swapchain has no such conversion: the compositor reads it as linear light, and a picture written
into it as it is shows lighter than the same picture in a surface (a gray of 128 shows as
191). The pass draws each picture's top at its image's last row, and the layer needs no
`VERTICAL_FLIP` layout, which a quad of wgpu's images and a surface's quad need.

On a Quest 3 the pass costs 0.4 ms of CPU and 0.03 ms of GPU for a frame of two 320 by 360 pixel
pictures, and 0.4 ms and 0.3 ms for two 1920 by 1080 pixel pictures. A frame comes 30 times a
second in the demo, and the headset runs at 120 Hz. The `stream-costs` feature of `cranpose-xr`
times the passes and logs the costs once a second. Without it the passes run with no timing.

## Crates

- `cranpose-xr`: the API above and the spatial tree. It lays out the panels, casts the
  controllers' rays at them and sends pointer events into each panel's composition. `movable`
  and `resizable` handle grabs.
- Its OpenXR backend: the session, the frame loop, the swapchains, the input actions and the
  extensions. The OpenXR calls sit in a reviewed boundary module.
- A flat backend for every other platform: the `Subspace` draws as a 3D scene in an ordinary
  window, as the desktop demo's XR tab does. The web adds WebXR through the same API.

Cranpose owns the GPU context. On the Quest it uses GLES through wgpu, which the cranvr client
uses today. On Linux and Windows the OpenXR runtime (SteamVR, Monado, Meta Link, Pimax, Varjo)
creates the Vulkan instance and device through `XR_KHR_vulkan_enable2`, and wgpu wraps them. The
backend keeps the difference in its `graphics` module: one file for GLES and one for Vulkan make
the session and the wgpu device and wrap the swapchain images as wgpu textures. The panels, the
rays, the handles, the passthrough layer and the cylinder layers do not change. Four things stay
with Android: the surface swapchains of `SpatialExternalSurface` (a surface panel shows nothing
in a desktop headset), the stream layer of `StreamedScene` (it imports `AHardwareBuffer`s into GL
through an EGL image, so a desktop headset shows a streamed scene as nothing until the stream
layer has a Vulkan import of the platform's decoded frames), the
`XR_FB_composition_layer_image_layout` flip that GL's bottom-first rows need (a Vulkan image
starts with its top row, as wgpu writes it), and the `VideoDecoder`.

The `host` module holds the other difference. An Android activity polls its events between
frames. A desktop process has none. `cranpose_xr::run` asks the OpenXR loader for a runtime and a
head-mounted display first. It starts the headset session when both exist and opens the flat
window when one is missing. The OpenXR loader is found at run time (the `openxr` crate's `loaded`
feature): the binary links no loader, so a machine without one runs the flat window. A Linux
distribution installs the loader with its OpenXR package. A Windows release ships
`openxr_loader.dll` beside the executable, because no runtime puts it on the DLL search path. The
`static` feature would build the loader from source with CMake and a C++ compiler for each
target, which the Android build and the cross checks from a Mac do not need.

## What the backend handles for the app

An app on a Quest needs some details right, or the headset keeps its loading screen up and
shows none of the app's frames. The `xr` service and the backend take care of them:

- The manifest names the app VR only (`com.oculus.intent.category.VR`), so Horizon OS opens it
  in an immersive window instead of a panel window.
- The session gets the activity itself. The runtime finds the app's window through it, and the
  process's NDK context can hold the application context instead.
- Each panel's swapchain image starts with its top row, as wgpu writes it, and the layer says
  so with `XR_FB_composition_layer_image_layout`.

The backend also keeps these rules of every runtime and store:

- A session that is VISIBLE but not FOCUSED, such as under the system menu, gets no controller
  input. The pointer sees both controllers lost, which ends a press and drops a carried panel.
  The panels still draw.
- A session that is not VISIBLE gets frames with no layers and no drawing.
- A session that is not running keeps its state: the composition, the panels' places and the
  swapchains stay until the app ends.
- After the session begins, the app asks `XR_FB_display_refresh_rate` for the highest rate the
  runtime offers, and logs the offered rates, the current rate and each change. Panels draw only
  when their content changes, so the frame cost stays low at any rate.
- The log names the runtime and its version and the system once at the start, so a report from
  any headset says which runtime it came from.
- The app ends when the runtime asks (EXITING, LOSS_PENDING, a lost instance) or when the
  activity is destroyed. A session that still runs goes through exit request, STOPPING and
  `xrEndSession`. Then the session drops before the instance, and the instance before the GL
  context.
- The loop takes the activity's key and touch events from the queue and leaves them unhandled.
  An event left in the queue makes Android show an "app is not responding" dialog five seconds
  later.

## Headsets

One APK carries the manifest entries of every runtime below. A runtime reads its own entries and
ignores the others. The comments of `crates/cranpose/android/manifests/xr.xml` cite the source of
each entry. Every row also carries the Khronos category
`org.khronos.openxr.intent.category.IMMERSIVE_HMD` and the `OPENXR` permissions of the Khronos
loader.

| Headset | Runtime | What the manifest adds | Tested |
|---|---|---|---|
| Meta Quest 3 | Meta, runtime name "Oculus" 209.174.0 | `com.oculus.intent.category.VR`, `com.oculus.supportedDevices`, `com.oculus.vr.focusaware`, `android.hardware.vr.headtracking`, `excludeFromRecents` | Yes: starts immersive, 120 Hz at 120/120 frames with no tear or stale frame, a focus loss under a system dialog, Home and a new start |
| Meta Quest 2, Quest Pro, Quest 3S | Meta | The same entries | No |
| Pico 4, Pico 4 Ultra | PICO OS 5.9 and later with the Khronos loader | `pvr.app.type`, `pvr.sdk.version`, `pxr.sdk.version_code`, `com.picovr.intent.category.VR` | No |
| HTC Vive Focus 3, XR Elite | Wave OpenXR | `com.htc.intent.category.VRAPP`, `com.htc.vr.content.NumDoFHmd`, `NumDoFController` and `NumController` | No |
| Samsung Galaxy XR | Android XR (`libopenxr.google.so`) | `PROPERTY_XR_ACTIVITY_START_MODE` set to the unmanaged full space, `uses-native-library`, `android.software.xr.api.openxr` | No |
| Steam Frame | Valve's Android container (Lepton), controllers shown as Oculus Touch | The Khronos category only: no entry of its own was found | No |
| Play For Dream | Not documented | The Khronos category only: no entry of its own was found | No |

Entries without a source stay out. `com.pvr.hmd.trackingmode` has none, and the `wave.feature.*`
entries would hide the app on stores that filter by features.

The Gradle property `cranposeXrStore` (or `cranpose { xrStore }`) sets the values on which the
stores disagree:

| Store | `android.hardware.vr.headtracking` | `android.software.xr.api.openxr` | `excludeFromRecents` | Tested |
|---|---|---|---|---|
| `meta` (default): Meta Horizon Store | required | optional | true | The merged manifest only. No upload |
| `android-xr`: Google Play for Android XR | optional | required | false | The merged manifest only |
| `open`: Pico, HTC, Steam Frame, sideloading | optional | optional | false | The merged manifest only |

`adb install` reads no feature, so the Quest run cannot show how a store filters by them. The
Quest lists no `android.hardware.vr.headtracking` among its system features.

## Steps

| # | Work | Done when |
|---|---|---|
| 1 | `cranpose-xr` with an OpenXR session, one `SpatialPanel` on a quad layer and a controller ray that presses its button | an APK built from this repository runs on a Quest 3, with no streaming library in its dependencies |
| 2 | `SpatialCurvedRow` on cylinder layers | a curved panel shows text as sharp as a flat one |
| 3 | `SpatialExternalSurface` | a hardware decoder writes into a panel with no copy |
| 4 | 3D content, `movable`, `resizable`, `Orbiter`, passthrough | a 3D scene, handles and the room around the panels |
| 5 | cranvr's Quest client on `cranpose-xr` | its desktop, console and overlay use the API |
| 6 | Store readiness: focus and pause, the entitlement check behind a feature, other runtimes | an app passes the Meta store's checks |

The cranvr client still uses a streaming library for games. Its own protocol replaces it after
step 5, outside this repository.
