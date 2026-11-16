# XR architecture

This note describes how Cranpose shows a scene in a headset today. It also describes the
structure planned for apps on the Meta Horizon store and on Steam.

## What exists

A `scene3d` modifier turns a node into a window on a 3D scene. Its `Camera` is a pinhole with a
view matrix and a focal length in pixels. The camera looks through the middle of the node's box.
The renderer clips each scene to its node's box with a scissor, so two scenes side by side show two
eyes. Mesh surfaces (`mesh_surface`) carry live composables. Pointer input reaches them where they
show on the mesh.

The desktop demo's XR tab (`apps/desktop-demo/src/app/xr`) uses these parts:

- `eye_view.rs` turns a WebXR eye (position, orientation, view tangents) into a camera. A headset
  eye's view is off center. The camera shears x and y by depth to move the image center.
- `scene.rs` builds the scene in meters. The camera cuts away points nearer than a 64th of its
  focal length, so the scene scales meters to millimeters.
- `tab.rs` shows the scene through one camera or through two eyes.
- `page.rs` holds what the page sends. The eyes for each frame live in a state. A scene reads them when it is drawn, so a new pose
  draws the scene again without a new composition.

## The browser path

The page script in `apps/desktop-demo/index.html` owns the WebXR session:

1. It asks `navigator.xr` for `immersive-vr` support and tells the app through `xr_supported`.
   The tab then shows "Enter VR".
2. The button calls `cranposeXr.enter`. The page starts the session and makes an `XRWebGLLayer`
   on its own WebGL2 context. It sizes the app's canvas to the layer's framebuffer.
3. The page sets `window.cranposeFrameSource`. The web event loop then asks the session for frames
   instead of the window. A browser stops the window's frames during an immersive session.
4. In each session frame the page sends both eyes through `xr_views`. It runs the app's frame. The
   app draws the left eye in the left half of its canvas and the right eye in the right half.
5. The page uploads the canvas to a texture in the same task. It draws each half into its eye's
   viewport of the layer.

`?xr=mock` replaces the headset with a script that turns a head on the window's frames. The page
shows the layer's picture in the bottom right corner. A desktop browser can test the whole path
this way.

The copy costs one canvas upload per frame. The app's canvas uses WebGPU or WebGL2. The copy reads
either one.

## The planned structure

An XR app needs the same composables on three targets. One crate, `cranpose-xr`, should own the
parts that do not depend on the target:

- an `XrSession` state with the eyes and the input sources for the frame;
- a `rememberXrSession()` composable that gives the state to the app;
- `ImmersiveScene(content)`, which draws `content` once for each eye of the session;
- controller rays as pointer input on mesh surfaces.

Each target gets a backend:

| Backend | Target | Frame source | Output |
| --- | --- | --- | --- |
| WebXR | Browsers, including the Meta Quest browser | `XRSession.requestAnimationFrame` | the canvas copy described above; later an `XRWebGLLayer` or a WebGPU XR binding as the render target |
| OpenXR | Quest native apps (Android) and SteamVR (Windows, Linux) | `xrWaitFrame` | wgpu renders into the OpenXR swapchain images, one array layer per eye |
| Stream | a Mac with cranvr | the headset's tracking from the ALVR client | cranvr's compositor takes the eye textures through IOSurfaces and encodes them |

The OpenXR backend makes a store app possible. wgpu has no OpenXR binding. The backend has to
create the Vulkan instance and device through `XR_KHR_vulkan_enable2` and wrap them for wgpu with
its hal API. The Meta store takes an Android APK with the OpenXR loader. Steam takes a Windows or
Linux build with the same backend.

The browser copy stays the simplest path. It works today on any WebXR browser. The OpenXR
backend removes the copy and the browser's frame overhead.
