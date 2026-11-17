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
- `SpatialExternalSurface(stereo_mode, modifier) { surface -> ... }`: hands the app a surface
  that a decoder writes into.
- `Orbiter`: content attached to the edge of a panel, such as a handle or a toolbar.
- `SubspaceModifier`: `width`, `height`, `depth`, `size`, `offset`, `rotate`, `movable`,
  `resizable`, `fill_max_size`, `padding`.
- `SpatialRoundedCornerShape`: the shape of a panel.

Jetpack XR Compose shows 3D models through SceneCore entities. Cranpose has its own 3D: a
`scene3d` box and `mesh_surface` content. In a `Subspace` such content draws into the
projection layer, with a camera made from each eye's view.

## Crates

- `cranpose-xr`: the API above and the spatial tree. It lays out the panels, casts the
  controllers' rays at them and sends pointer events into each panel's composition. `movable`
  and `resizable` handle grabs.
- Its OpenXR backend: the session, the frame loop, the swapchains, the input actions and the
  extensions. The OpenXR calls sit in a reviewed boundary module.
- A flat backend for every other platform: the `Subspace` draws as a 3D scene in an ordinary
  window, as the desktop demo's XR tab does. The web adds WebXR through the same API.

Cranpose owns the GPU context. On the Quest it uses GLES through wgpu, which the cranvr client
uses today. Vulkan is a later choice.

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
