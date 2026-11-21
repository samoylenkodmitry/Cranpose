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
| `SpatialVolume`: 3D content of `scene3d` and `mesh_surface` | one projection layer for all volumes, with depth where the runtime takes it | core, `XR_KHR_composition_layer_depth`, `XR_FB_composition_layer_depth_test` |

A panel draws only when its content changes. The compositor shows the last image of a layer
until a new one comes, so a still panel costs no GPU time at all.

A Quest client proved the surface swapchain and the quad layer on a Quest 3 by hand. Its desktop is
a surface swapchain fed straight by the hardware decoder. Its console is a quad layer of Cranpose
content. The controllers' rays and the handles of the panels are quad layers with one
static picture each (`ray_layers.rs`, `handle_layers.rs`), so they cost no draw. Only
`SpatialVolume` draws a 3D scene, into a projection layer.

## The API

Names follow Jetpack XR Compose (`androidx.xr.compose`), so Google's documentation serves
Cranpose developers too:

- `Subspace { ... }`: the 3D area of an app. Outside a headset it shows its panels side by side in
  the window, and a `SpatialVolume` in it shows as a 3D view in its box.
- `SpatialPanel(modifier, shape) { ... }`: 2D Cranpose content on a flat panel.
- `FollowingSubspace(follow) { ... }`: a `Subspace` whose origin is in front of the user's head.
  Jetpack XR 1.0.0-beta02 spells it `Subspace(follow = FollowTarget.view(FollowMode.snap()))`. It
  replaced `FollowingSubspace(target, behavior)` and the `UserSubspace(lockingBehavior)` of the
  alpha releases. Rust has no overloads, so the composable keeps the name `FollowingSubspace`.
  `FollowTarget::View(FollowMode::Snap)` places it when it first composes and again after a
  recenter. `FollowMode::Tight` follows the target in every frame, as Jetpack XR's
  `FollowMode.tight` does. `FollowTarget::Controller(Hand, FollowMode)` follows a controller's
  grip pose. Jetpack XR has no controller target. `FollowTarget::Anchor(Anchor, FollowMode)` holds
  the panels to an anchor, as Jetpack XR's `FollowTarget.anchor` does. Its `FollowMode.soft` is not
  there yet.
- `SpatialRow`, `SpatialColumn`, `SpatialBox`, `SpatialCurvedRow`, `SpatialSpacer`: layouts in
  meters.
- `SpatialExternalSurface(stereo_mode, modifier) { scope }`: hands the app a surface that a
  decoder writes into. The scope's `on_surface_created` and `on_surface_destroyed` receive an
  `ExternalSurface`, and its `on_pointer_event` receives the pointer events of a controller's ray
  on the surface.
- `StreamedScene(frames, views, stats)`: frames that another machine rendered, shown as a projection
  layer. Jetpack XR has no such composable. It has no `Spatial` prefix, because it is not placed
  in a `Subspace`: it fills the viewer's view. Outside a headset it fills its box in the layout
  with the left eye's picture of the newest frame.
- `SpatialCurvedRow(modifier, curve_radius) { ... }`: panels bent around the viewer at
  `curve_radius` dp. Each panel is a cylinder layer, and a controller's ray follows its curve.
  A runtime without `XR_KHR_composition_layer_cylinder` shows the panels flat.
- `local_spatial_capabilities()` (Jetpack XR's `LocalSpatialCapabilities`):
  `is_spatial_ui_enabled()` and `is_passthrough_control_enabled()`.
- `local_spatial_environment()`: the `SpatialEnvironment` with `preferred_passthrough_opacity`
  and `current_passthrough_opacity`, which control the room around the panels.
- `SpatialFrameEffect(on_frame)`: runs a callback once in each frame of the headset with an
  `XrFrame`, which holds the head, the eyes and the controllers, and, for what the app asks for,
  the hands, the body, the face and the eyes' gaze. See "The headset's state" and "Tracking hands,
  body, face and eyes".
- `local_tracking_capabilities()`: the `TrackingCapabilities` that tell, for each `TrackingFeature`,
  whether the runtime offers it.
- `local_haptics()`: the `Haptics` handle that vibrates a hand's controller.
- `local_anchors()` and `local_planes()`: the `Anchors` and `Planes` handles of the room. An anchor holds
  content on a table or a wall and keeps it there after a restart. See "Anchors and planes".
- `MovePolicy::anchor(orientations, semantics, on_move)`: a movable panel or volume that snaps to a
  plane of the room when the user lets it go near one.
- A keyboard for the text fields of the app, with no composable to write: see "Text entry".
  `use_keyboard(Keyboard)` chooses between the system's keyboard and the one that Cranpose draws.
- `Orbiter`: content attached to the edge of a panel, such as a handle or a toolbar.
- `SubspaceModifier`: `width`, `height`, `depth`, `size`, `offset`, `rotate`, `with_pose`,
  `movable`, `movable_with`, `resizable`, `resizable_with`, `fill_max_size`, `padding`.
- `SpatialRoundedCornerShape`: the shape of a panel.

- `SpatialVolume(modifier, content)`: 3D content in a box, drawn by Cranpose's own scene. See "3D
  content".
- `SpatialGltfModel(state, modifier, content)`: a glTF 2.0 model in a volume, behind the `gltf`
  feature. `rememberSpatialGltfModelState(source)` loads it. See "glTF models".
- `SpatialPointSource(modifier, source)` and `SubspaceModifier::point_source(source)`: sound that
  comes from a pose in a `Subspace` or from a panel or a volume. It follows the head. See "Spatial
  audio". `local_spatial_capabilities()` tells with `is_spatial_audio_enabled()` whether the app
  has the sound.

## 3D content

`SpatialVolume(modifier, content)` puts 3D content in a `Subspace`. The content is ordinary
composables whose nodes carry `mesh_surface` or `mesh_surfaces`. Cranpose's own scene draws them, as
`scene3d` draws them in a window, so a mesh shows live composable content on its surface. Jetpack
XR 1.0 has `SpatialGltfModel`, which renders a glTF model, and `SceneCoreEntity`, which attaches any
SceneCore entity. Cranpose has no SceneCore entities. Its composable keeps its own name, after the
box that the content stands in. A glTF model has `SpatialGltfModel` (see "glTF models").

The modifier gives the box: `width`, `height` and `depth` in dp, which are millimeters, 500 each by
default. The modifier's `offset`, `rotate`, `movable` and `resizable` work as on a panel. The
content's space has its origin at the box's center, x to the right, y down and z away from the
viewer, and a unit is a dp. That is the space of a `scene3d`, so the same content shows in a window.
The box is where a ray stops. The content may reach outside it.

### One scene, composed once

A headset draws the content of all volumes in one scene, because one scene has one depth buffer: a
nearer volume hides a farther one. The volumes' content composes where the app wrote it, but the
composition does not emit it there. Each `SpatialVolume` gives its content to the space of the
headset. A root composable, `VolumeScene`, composes it again at the root of a window root of the
app's composition (`Modifier::window_root`, the root that Cranpose's host overlays use). The window
root has its own renderer and the size of one eye's picture. The atlas of the panels does not draw
it, so a volume that animates redraws no panel.

The content composes once. Its state and its effects are the composition's own. A composition
local that the app provides between the subspace and the volume does not reach it, because the
scene composes at the root. The headset provides its own locals there.

Each frame draws the same composition twice. For each eye the headset writes the eye's camera into
a state that the scene's `scene3d` reads while it draws. Then it calls `GuestView::draw_window`,
which updates the composition and renders the window root into the eye's texture. The update
collects the scene with the new camera, so no composition and no layout run between the eyes. A volume's pose is a state too. `transform3d` of the volume's node reads it while
the scene draws, so carrying a volume composes nothing.

### The eye's camera

A scene's camera is a pinhole whose image center is the middle of its box. An eye's display is off
center: a Quest's eyes reach farther outward than toward the nose. `EyeCamera` turns an eye's pose
and field of view from `xrLocateViews` into a `Camera` of the scene. The camera shears x and y by z,
which moves the image center by a fixed amount at every depth. It has one focal length for both
directions, the smaller of the horizontal and the vertical, so the picture has square pixels and
covers the eye's field of view in both directions. The image therefore covers more than the eye's
field of view along the other direction. The layer submits the field of view that the image covers
(`EyeCamera::field_of_view`), which holds the eye's own along the limiting direction. A layer that
submitted the eye's own field of view would shift the picture.

The scene's axes are the room's turned half a turn about x, in millimeters. `scene_from_volume` maps
a volume's content to the scene from the volume's pose in the room, and the eye's camera maps the
scene to the eye, so the two share the scene's units. The camera cuts away what lies nearer to the
eye than a 64th of its focal length in pixels: 12 mm for a focal length of 800 pixels.

A GL swapchain's first row is its bottom one. The camera draws the picture upside down, so the
runtime reads it upright. This also holds for the depth, which has the same rows. The layer chains
no `XR_FB_composition_layer_image_layout` to the projection, because that extension's reach over a
depth image is not specified. A Vulkan image starts with its top row, so its camera does not flip.

### The projection layer and its depth

All volumes draw into one projection layer with a view for each eye. Each view has its own swapchain
of the eye's recommended size and its own pose and field of view. The layer's flags say that the
picture's alpha blends over what lies under it (`BLEND_TEXTURE_SOURCE_ALPHA`), so the room and the
panels' layers show through where no mesh draws. The picture is premultiplied, as the panels' are.
The renderer draws the scene into a target of its own, the page, and converts the page into the
swapchain's image, as it does for the panels' atlas. It draws straight into an image only when the
image also takes copies. An sRGB image would then need a view of another format, and a swapchain's
image has none. The scene takes four samples in each pixel where the device takes four in its
colour and depth formats (see "Anti-aliasing").

With `XR_KHR_composition_layer_depth` the layer carries the depth of each picture. The renderer's
scene pass can leave its depth in a view that the host gives it
(`WgpuRenderer::render_with_scene_depth`), so no copy of the depth is made. The depth swapchain has
the format that the scene's pipelines draw against, `Depth24PlusStencil8`, which is
`GL_DEPTH24_STENCIL8` on GLES. The scene's depth is `1 - near / z` for a point at distance `z`
along the eye's axis: 0 at the near plane and 1 at infinity. The layer's depth info therefore says
`nearZ` is the camera's near plane in meters and `farZ` is 10 km. The extension allows an infinite
`farZ`, and a plane 10 km away gives the same depth to a part in a million, so no runtime has an
infinity to mishandle.
Wgpu maps `Depth24PlusStencil8` to one of two Vulkan formats by the driver and offers no query that
says which, so a Vulkan headset has no depth swapchain and submits none. A runtime that offers no
depth swapchain format submits none either. The picture still shows.

### Anti-aliasing

The scene takes four samples in each pixel (`Multisample::X4`) where the device takes four in the
format that the renderer draws the scene in (`Rgba8Unorm`, the non-sRGB form of the swapchain's
format) and in `Depth24PlusStencil8` (`volume_samples`). A device that takes fewer draws with one.
The headset asks the adapter once, at the first frame with a volume, and the log names the answer.

**The path.** The pass draws into colour and depth attachments of four samples that are transient
(`TextureUsages::TRANSIENT_ATTACHMENT`), cleared at the start and discarded at the end. Its colour
attachment has the page as its resolve target. A tiled GPU keeps the four samples in tile memory,
averages them as it writes the page, and never writes them out. On GLES, which is the Quest's
graphics API in this backend, the wgpu fork's GL backend binds the page with
`glFramebufferTexture2DMultisampleEXT` when the device has `GL_EXT_multisampled_render_to_texture`,
the resolve target is a plain texture, the colour is discarded and the attachment is the first. That
extension is multisampled render-to-texture: no multisampled buffer exists, and the driver averages
in tile memory. Without it the backend resolves with a blit after the pass. On Vulkan, which Linux
and Windows headsets use, the same pass is a subpass with a resolve attachment.

Before this change a scene of four samples drew into the samples, averaged them into a texture of its
own and then laid that texture over a cleared page with a composite pass. A scene that covers a page
which still holds only its transparent clear now averages straight into the page. The pass replaces
the page's clear and the composite, so a frame of volumes has one pass for the colour where it had
three (clear, scene, composite). The path needs the page to be blank under the scene, and the scene
to cover it. A scene with glass, with other content beneath it or that does not cover the page takes
the earlier path.

**The depth.** One wgpu pass holds one sample count, and wgpu has no depth resolve. Storing a depth
of four samples to resolve it in a later pass costs four times the bandwidth of the depth that the
layer carries. A second pass with one sample draws the opaque surfaces and the volumes again with the
depth pipelines (`ScenePipeline::Depth`, which discards the texels that the colour pass discards) into
the view of the depth swapchain. It runs after the colour pass and draws only the steps that the
colour pass drew, so the depth never shows what the picture does not. Its colour attachment is a
transient placeholder that it clears and drops. Each pixel then holds the depth of its centre. A
pixel that an edge covers in part carries the surface's depth where the centre is covered and the
far plane where it is not, which the panels over the layer see as a fringe of one pixel at most. A
scene of four samples that cannot average into the page leaves the far plane in the depth. The
doorways of a scene write no depth in this pass.

**What was measured.** A test on the GPU of the development machine draws a red quad of 38 pixels,
turned by half a radian so that its edges slant, in a picture of 128 by 128 pixels, with one sample
and with four. The mesh has seams for edges: a `Mesh::plane` smooths its outline on its own, and a
solid's edge does not. With one sample no pixel is in between (alpha neither 0 nor 255). With four
samples 84 pixels are, and at least nine in ten of the rows that an edge crosses hold one. With four samples
every opaque pixel carries the quad's depth, every empty pixel the far plane, and an edge pixel one of
the two.

### Layer order and the depth test

The frame's layers, from the bottom: the room's passthrough layer, the streamed scenes, the one
projection layer of the volumes, the flat and curved panels, the surfaces and the panels' handles,
and the rays. The projection layer lies under the panels, because the compositor draws a later
layer over an earlier one.

Where the runtime has `XR_FB_composition_layer_depth_test` and the projection layer carries depth,
each panel, surface, handle and ray layer chains an `XrCompositionLayerDepthTestFB` with
`LESS_OR_EQUAL` and no depth write. The compositor then tests that layer's fragments against the
depth that the projection layer wrote. A panel behind a 3D object shows behind it. A panel in
front of the object shows in front of it. Panels still draw over each other in the order of their
distance, and a panel's rounded corners still blend.

The compositor's depth buffer starts at the far plane. The depth that a projection layer carries in
its views does not reach the buffer. A layer writes its depth only when it chains the test struct
with `depthMask` set. The projection layer chains one with `ALWAYS` as the compare operation and the
write on (`volume_plan::writes_depth`). Without that struct a panel passes its test against a
buffer that nothing wrote, and it covers the whole torus.

Without the test, the projection layer lies under all panels, and a panel covers the 3D content
wherever the panel stands, also where the content is nearer. A rule that sorts the one layer among
the panels fails for two volumes with a panel between them, because a layer has one place in the
list. The order is the same on every runtime. The depth test corrects it where it exists.
`volume_plan` holds the rule as a function of the number of volumes, the
eyes the runtime located and the extensions, and its tests cover it.

A frame with no volume has no projection layer, no swapchain and no draw. The first frame with a
volume makes the swapchains. The frame after the last volume leaves frees them.

### Pointer

A controller's or a hand's ray meets a volume in two steps. The slab test of `Ray::hit_box` finds
whether the ray crosses the volume's box. For a volume whose content has meshes, the pointer then
finds the nearest triangle of those meshes, and the ray stops there. A ray that crosses the box and
meets no triangle passes through, to the panels behind. A volume with no mesh stops the ray at its
box, as before.

**Where the meshes come from.** The scene's layer tree holds each `mesh_surface` and
`mesh_surfaces` node with its map into the scene. `GuestView::visit_window_meshes` walks the layers
under the node of one volume (`VolumeNode` records that node) with no allocation, and reports each
mesh with the map from its own space to the volume's content space. The walk is `SpaceTree`'s own,
generic over what it keeps, so picking and drawing agree on where a node stands: the layout offset,
the `transform3d` nodes above it and the surface's transform. The headset keeps the list in
`PickMeshes`, in storage that it reuses. It takes the list again for each volume that a ray points
at or that a press is on, from the scene's last draw. A volume that no ray points at keeps its last
list. A mesh that moves in a frame is therefore hit where it stood in the frame before.

**The pick.** `ContentRay::of` turns the room's ray into the volume's content space with the
direction in dp for each meter, so the distance that a mesh gives is in meters and compares with the
distance to a panel. Each mesh takes the ray into its own space through the inverse of its map. The
mesh's box (`Mesh::bounds`, worked out once) turns most rays away with a slab test. `Mesh::cast`
then reads the triangles, from either side, and gives the nearest in front of the ray's origin. A
triangle's edge counts as a hit. The nearest mesh wins. The pick allocates nothing, and a mesh
whose map has no inverse takes no ray. A volume with no mesh in the list takes the box.

**What the app gets.** `SpatialVolumeScope::on_input_event` receives an `InputEvent`, after Jetpack
XR's: the source, the hand, the time, the ray in the content's space, an `InputAction` and a
`HitInfo`. The actions are `HoverEnter`, `HoverMove` and `HoverExit` with no press, and `Down`,
`Move` and `Up` for a press. Jetpack XR's `CANCEL` has no counterpart: a controller that is lost
ends a press with an `Up`. The `HitInfo` names the composable node whose mesh the ray meets
(Jetpack XR's `inputEntity`), which of the node's meshes it is, the place of the hit in the content
space in dp (`hitPosition`) and the triangle. As in Jetpack XR, a press keeps the entity that it
began on: its `Move`s and `Up` name that mesh, and `hit_position` is `None` where the ray has left
it. Jetpack XR's list of every entity that the ray crosses is one nearest hit here, and its
`transform` has no counterpart. For a glTF model `SpatialGltfModelState::node_of` maps a hit to the
`GltfModelNode`: each material's node records the composition node that carries it, and the draw
that the mesh index names gives the file's node, with its name.

The demo's readout panel shows the result in its `Ray meets` line: `torus`, `crate, glTF node
crate` (the name of the file's node) or `nothing`. The ray passes through the torus's hole.

`SpatialVolumeScope::on_pointer_event` still gives Compose's pointer events. Its position is the
hit's place projected onto the box's front face where the volume has meshes. The Compose events go
to the volume as a whole. A composable on a mesh does not receive the ray. A window shows the volume
under the mouse, where the meshes take input as `mesh_surface` documents.

The handles of a movable or resizable volume stand at its front face, half the depth toward the
viewer. A handle at the center plane of a deep box would lie behind the entry of a ray that points
at it. The resize measures on the front face's plane and changes the width and the height. The
content does not scale with the box.

### glTF models

`SpatialGltfModel(state, modifier, content)` is a `SpatialVolume` whose content is a glTF 2.0 model.
It needs the `gltf` feature of `cranpose-xr`, which adds the `gltf`, `image`, `base64` and
`percent-encoding` crates. An app without the feature has none of that code. The web builds do not
link `cranpose-xr`, so their size does not change.

**Loading.** `rememberSpatialGltfModelState(source)` starts an effect that reads the file in
`withBlocking`, on the shared pool of worker threads. The state holds `Loading`, `Loaded` or
`Failed(GltfError)`. The composable shows nothing until the state is `Loaded`, as Jetpack XR's does.
The `gltf` crate parses the JSON and the binary container. The loader resolves the buffers and the
images itself: the binary chunk of a `.glb`, `data:` URIs, and files beside a `.gltf`. The crate's
own `import` feature would do the same, but it needs `base64` 0.13, a second version of a crate that
the workspace has once. It would also decode every image of the file, where the loader decodes the
images that a base color texture names, with the PNG and JPEG decoders of the `image` crate. A
texture that has no source and names its image in `KHR_texture_basisu`, as the controller models of a
Meta Quest do, has a KTX2 file of Basis Universal data. The `basisu` crate, a transcoder in Rust,
turns its first mip level that fits a layer into RGBA pixels. A `SpatialGltfModelSource` is a path or
bytes. A source made from bytes has no directory, so it
finds no external buffer.

**What the loader keeps.** One `Mesh` for each triangle primitive (lists, strips and fans), with
the file's positions, normals and the texture coordinates of the base color texture. One picture for
each material: the base color texture multiplied by the base color factor, baked once into
the pixels (the factor is linear light, the texture is sRGB), with the alpha mode applied. A
material with no texture is one color. The node tree of the default scene, each node as
translation, rotation and scale. The animations. The loader leaves out skins and morph targets,
cameras, lights, vertex colors, the other material properties and the texture samplers. The scene's
sampler clamps, so a texture does not repeat. A texture beyond 2048 pixels is halved until it fits,
because each texture is one offscreen layer.

**Axes and fit.** glTF has y up and z toward the viewer, in meters. The volume's content has y down
and z away, in dp. One matrix, `ModelFit::root`, takes the model's middle to the box's middle,
scales by the fit and turns the model half a turn about x. The node transforms and the animation stay
in glTF space, and a node's world transform is `local.then(parent)`. The half turn keeps the
handedness, so no triangle changes its winding. The fit follows Jetpack XR's measure policy. Each
side has the ratio of the modifier's size to the model's own side. A side with no size set has the ratio one. The
model scales by the smallest ratio, up or down. The box takes the modifier's size on the sides that
set one and the scaled size elsewhere. With no size set the model has its own size, 1000 dp for a
meter. The box of the model at rest gives its sides. An animation does not change the fit.

**Drawing.** The volume's content has one node for each material, however many nodes of the file
use it. The node's content is the material's picture (an `Image` or a color), and its
`mesh_surfaces` holds one surface for each primitive of the material, in the node's frame. Each
surface's transform is the node's world transform times the fit. A material shared by thirty nodes
costs one offscreen layer. The surfaces are lit. The scene's light is the default light of the
model, from the upper left, until the `content` callback sets another.

**Animation.** The loader keeps the keys of each channel: translation, rotation and scale with
`LINEAR` (a spherical interpolation along the short arc for rotation), `STEP` and `CUBICSPLINE`.
A `GltfAnimation` holds a state and a clock. A composable of the model runs an effect while one
animation plays. The effect advances the clocks by the frame time (at most 0.1 s a frame), poses the
nodes into one preallocated array of world transforms, and bumps a counter that the draw closures
read. The closures build their surfaces again for the new transforms, so an animation composes
nothing. A model with no playing animation runs no effect and builds its surfaces once. A start and a
stop between two frames leave the state unchanged and change the pose, so each start and stop also
counts into the effect's key.

A layer keeps a few spaces (`LayerSpaces`: the `Arc<LayerSpace>` that a node's graphics layer
holds) and an evaluation moves the surfaces of one that no node holds any more, in place. A node
keeps the space of its last evaluation until the next replaces it, so two spaces serve in turn and a
third covers a frame in flight. An animation of a material with any number of primitives allocates
no list of surfaces and no `Arc` in a frame. A space that is still held is never changed.

### glTF models on a Quest 3

Measured on a Quest 3 from the compositor's screenshots (`adb exec-out screencap -p`) and
`scripts/dev/xr_model_check.py`. The crate's texture has magenta and lime tiles in a white frame. The
`XR_DEMO_VOLUME=model` build shows the crate alone, so its screenshots hold those hues only in the
crate. The full demo's test patterns have the same hues, so the numbers of the full demo come from
the crate's region (`--region 3500,1500,4100,2000`, in the screenshot's pixels).

| Build | Left eye | Right eye |
| --- | --- | --- |
| `XR_DEMO_VOLUME=model`: the crate alone, standing still, magenta pixels | 103,340 | 102,475 |
| the same, lime pixels | 99,682 | 99,116 |

Three screenshots of one run differ by 0.1 percent. The two eyes see the crate from their own places,
so their counts differ by about 1 percent. The picture shows three faces in each eye with the tiles
upright. The full demo shows the crate beside the torus at 120 frames a second. At the demo's first
head pose the right eye sees the crate at its lower right, and the left eye's view ends before it. In
the crate's region of the right eye, 33,137 magenta and 32,355 lime pixels show, and 63,423 pixels
change their class between two screenshots a second apart, so the animation runs in the headset's
frame loop. The crate shares the projection layer of the torus's volume, so it adds no layer. The
log shows the layer count that the earlier measurement of the depth test gave (18 asked, 16 taken,
two panels left out by the code of that time, which had no layer budget), with no change after the
model loads. It has no
`glTF model:` failure and no `the 3D content ends`.

The `gltf` feature adds 770 KB to the stripped arm64 library of the demo, which is 11,835 KB without
the feature and 12,605 KB with it. Both come from a local `release-fast` build. The build without
the feature leaves out the demo's crate too. Of the 770 KB, the `zune_jpeg`, `image` and `png`
crates take 209 KB, `serde_json` 181 KB, `gltf` and `gltf_json` 58 KB, and `cranpose_xr` 49 KB. The
shipping `release` profile has other absolute sizes. An app without the feature has none of it. The
`basisu` transcoder adds 1,120 KB to that library (14,140 KB to 15,261 KB, stripped arm64, the
Gradle `release` build of the xr-demo).

### Controller models

A headset draws a model at the grip pose of each controller in use. The models are part of the
volumes' scene. `VolumeScene` composes a node tree for each hand beside the volumes' content, so one
projection layer and one depth buffer carry both. Where the runtime tests depth, a panel and a model
hide each other by their distance from the head, as a panel and a volume do. The scene exists while
a volume exists or a controller shows a model. An app with no volume gets the projection layer, its
swapchains and the pass only while a controller shows a model. With the models off it gets none of
them.

**Which controller shows a model.** `controller_grips::shown` takes the frame's two `Controller`s.
A controller shows a model while the runtime tracks its grip pose and the hand's interaction
profile is not that of a hand (`InteractionProfile::is_hand`). A runtime keeps the hand profile's
poses valid with no hand in view, and a tracked hand with no controller holds nothing to draw, so
the hand profiles show no model. The state of the pointer decides nothing: a controller that lies on
a desk is in use. `ControllerStates::track` applies the app's switch. With `set_enabled(false)` no
hand shows a model.

**Which model a hand shows.** `controller_look::look` decides from three inputs: the app's
`ControllerModel` for the hand, the runtime's `Offer`, and how far the load of the glTF source has
come. The offer is `Pending` until the backend has asked the runtime. It is `Absent` where the
runtime has no model for the hand, where the app did not declare `Use::controller_models()` and
where the build has no `gltf` feature. It is `Bytes` once the backend has copied the runtime's glTF
binary out.

| App's choice | Runtime's offer | Load | The hand shows |
| --- | --- | --- | --- |
| `Plain` | any | any | the plain model |
| `Runtime` | `Pending` | none | nothing, until the backend has asked |
| `Runtime` | `Absent` | none | the plain model |
| `Runtime` | `Bytes` | loading | nothing |
| `Runtime` | `Bytes` | loaded | the runtime's model |
| `Runtime` | `Bytes` | failed | the plain model |
| `Gltf(source)` | any | loading | nothing |
| `Gltf(source)` | any | loaded | the app's model |
| `Gltf(source)` | any | failed | the plain model |

A controller in use shows a picture at all times except while its model is on its way.

**The runtime's model.** A Meta Quest hands out glTF binaries through `XR_FB_render_model`. The
instance enables the extension when the app declared `Use::controller_models()` (the manifest
feature `com.oculus.feature.RENDER_MODEL` and the permission `com.oculus.permission.RENDER_MODEL`)
and the runtime lists it. `render_models::RenderModels` asks for each hand before the frame that the
runtime waits for. It lists the render model paths, picks the path that has a `controller` segment
and the hand's side (`model_path::controller_path`), reads the model's properties with the glTF 2.0
subset 1 capability, then with both subsets if the first answer has no model, and loads the bytes.
The calls that list the paths and read the properties are made in `render_model_calls`, because the
`openxr` crate's wrappers pass structs that the runtime may leave unwritten. `model_asks::ModelAsks` schedules the questions. A hand
that the runtime answered with no model asks again when a controller comes into it. The loader of
`SpatialGltfModel` decodes the bytes on a worker thread. `ControllerLoaders` stays in the
composition while the scene is empty, so a controller that leaves and returns finds its model
loaded. The XR session stays on the frame thread, and only bytes cross to the worker.

**Where the model stands.** The runtime's models are glTF files in meters with x to the right, y up
and z toward the user. The origin of a model lies at the controller's grip pose.
`controller_placement::scene_from_grip` carries a point of the model to the scene, whose units are
millimeters with y down and z away. The half turn about x that takes glTF's axes to the content's
cancels the half turn that `scene_from_volume` makes. A point `p` of the model therefore lies where
the grip puts `p`. No box fits the model. `ModelFit` centers a model in a volume's box, and a
controller keeps the file's origin.

**Allocation.** The frame loop writes one `MutableState<Mat4>` for each hand, the map from the model
to the scene. The hand's `shown` flag changes only when a controller comes or goes. The surfaces of
a model carry the placement, and each layer's draw closure reads the state. `SurfaceRing` keeps a
few `LayerSpace`s for each layer and moves their surfaces in place, as the animations of a glTF
model need, so no draw builds a space or a list after the ring fills. A node with `transform3d`
would allocate a space in each evaluation, which is why the models carry their placement in the
surfaces. The plain model's parts share meshes that a process makes once. The write of the state is
the framework's. A write of a changed value keeps a record, as the pose of a carried volume and the
eye's camera do. A test of the ring shows that a moving model reuses its spaces. No allocation
counter has measured a frame.

**The plain model.** `plain_controller` builds seven parts from `Mesh::cuboid` and `Mesh::sphere`:
a handle along z, a head in front of it, a thumbstick's dome, a trigger, two button domes and a tip
that marks the front. Each part has one flat color. A part is a node whose content is a flat swatch,
drawn on a mesh by a graphics layer, as a glTF material is. `Mesh::cylinder` is an open tube, so no
part uses it.

**The scene's light.** A volume sets the scene's light. Where no volume sets one, the controllers'
models stand in the default light of the glTF models.

**What no headset has run.** The runtime's listing of the extension under the manifest entries, the
render model paths, the contents of the runtime's file, the loader on that file, the alignment of
the file with the grip pose, the depth test against panels and the frame rate of an app with no
volume are untested. The permission name, the path names and the origin of the models at the grip
pose follow the extension's documentation and stay assumptions until a headset confirms them. If
the runtime does not list the extension on its first run, the permission name is the first suspect:
the other services of `cranpose-capabilities` write a `horizonos` name beside the `com.oculus` one.
The extension has three calls and none gives the state of a button, so a model moves its buttons
only if its file holds clips. `scripts/dev/xr_controller_session.sh` and
`scripts/dev/xr_controller_check.py` are the procedure for the first run. Neither has run on a
headset. The check script ran on pictures that a script drew.

**The demo.** The counter panel has a button that cycles the choice of both hands: the runtime's
model, the plain model and none. The environment variable `XR_DEMO_CONTROLLERS` builds the
measurements. It holds names that a comma separates. `only` shows nothing but the models. `plain`
and `off` start with that choice. `panel` adds a magenta panel of 40 cm in the vertical plane through
the left controller's grip, so that the part of the model behind the plane hides and the part in front
shows. A build with `only` or `panel` logs the eyes' poses and fields of view, the grips and the
frames of the last second, once a second. `scripts/dev/xr_controller_session.sh` runs one build on a
Quest and saves the log and the compositor's picture. `scripts/dev/xr_controller_check.py` marks the
pixels that differ between the picture of the `only` build and that of the `only,off` build, joins
them into blobs and projects the logged grips into each eye.

### What it costs

A frame with volumes updates the composition and renders the window root twice. The scene's draw
closure reads the camera, so each eye's collection builds the scene again. The depth view that the
renderer keeps in its pass is the swapchain's own, so the depth costs no copy. The color takes the
renderer's conversion pass for each eye. With four samples a frame adds the second pass of one
sample for the depth: its vertex work and a store of the depth swapchain, which the one-sample pass
stored too. The CPU side allocates what `scene3d` allocates for each evaluation of its closure. The
layer code allocates nothing. The pointer's pick allocates nothing either: it takes the list of
meshes again only for a volume that a ray points at or a press is on, and keeps its storage.

A call of the runtime or of the renderer that fails ends the 3D content with one line in the log
(`the 3D content ends`). The session and its panels keep running. A runtime that refuses the depth
swapchain leaves the picture without depth, and the layers over it test nothing.

A volume takes two of the compositor's layers when it is movable: the projection layer and the
handle. The projection layer is one layer however many volumes there are.

### The layer limit

A runtime takes a limited number of layers in a frame. The system properties report the number,
and the specification guarantees 16. A Quest 3 reports 32 and ends a frame of 17 or more layers with
`XR_ERROR_LAYER_LIMIT_EXCEEDED`. So the session plans its frames for 16 layers until the runtime
takes a frame of the reported number (`layer_limit`). The first frame that the runtime shows tries
it: copies of an empty layer, a clear picture of 4 by 4 pixels, fill that frame up to the reported
number. A runtime that refuses the frame gets the same frame again in the same `xrEndFrame` turn
without the copies, so no frame is lost. The limit then stays at 16. A runtime that ends a frame
when it refuses it takes no second end of it, so that one frame is lost. The log names the result
once (`the runtime takes 16 layers of the 32 that it reports`).

When a frame needs more layers than the limit, the projection layer of the 3D content draws some
pictures in place of their layers (`layer_budget`, `projected_pass`). No panel goes missing.

- The room's layer, the streamed scenes, the projection layer and the rays keep their layers. A
  surface keeps its layers too, because only the compositor reads what its producer writes.
- The handles go into the projection layer first. A handle is a small sphere, so its picture needs
  no sharpness. Then the content panels of the lowest rank go there.
- A panel ranks by its `SubspaceModifier::layer_priority` first, then by whether a hand points at
  it or holds one of its handles, then by the distance of its plane from the head: the nearer panel
  ranks higher. A panel that the last frame drew into the projection layer counts 5 cm farther than
  it stands, so two panels at about one distance do not trade places at each small move of the head.
- A frame with no 3D content gets a projection layer for the pictures. That layer takes one place
  of the budget. 17 panels with no room's layer keep 15 layers and draw 2 into the
  projection layer.
- Surfaces that do not fit beside the fixed layers are left out, the farthest first. A frame whose
  3D content failed has no projection layer. That frame leaves out the pictures of the lowest
  rank.

The pass draws each picture into both eyes' images after the scene of the volumes, back to front, as
a flat rectangle or as 32 strips of a cylinder for a curved panel. A panel samples its part of the
panels' atlas, which the composition already draws. A handle samples a texture of its sphere.
Where the layer carries depth, the pass tests against the scene's depth and writes its own, so a
picture hides behind a nearer volume. The panels over the projection layer test against the
pictures as they test against the volumes. A runtime with no depth test shows every panel of its own
layer over the pictures. The default rank keeps that order right, because the pictures are the
farthest panels. A priority or a pointer that keeps a far panel on its layer can put it over a
nearer picture there. The pictures in the projection layer are sampled twice, once by the pass and
once by the compositor, so their text shows softer than on a layer of their own. Each content panel
keeps its own swapchain filled, so a panel that gets its layer back shows its picture at once.

A frame within the limit plans with one count of its layers. It draws nothing more and submits the
same layers in the same order as before the budget existed. The pass, its pipelines and the
textures of the handles exist from the first frame that draws a picture. The log names each change
of the plan (`the frame needs 19 layers and gets 16 of the 16 that the runtime takes: the projection
layer draws 4 of them, 0 are left out`) and of the submitted count.

A Quest 3 (runtime "Oculus" 210.137.0) refuses the trial frame of 32 layers. The same frame then
ends with its 16 layers, so no frame is lost. The runtime logs one warning for the trial
(`optimized layer count 32 exceeds compositor resource limit 16`). The build `XR_DEMO_PANELS=19`
needs 21 layers: 19 panels and two rays. It gets 16: 13 panels, the rays and a projection layer
that draws the other 6 panels. All 19 panels show in both eyes of the compositor's screenshot
(`scripts/dev/xr_layer_budget_check.py`). The frame rate is 120 in every second after the start.
The app's GPU time is 0.82 ms, the GPU's share 35.3 percent and the compositor's time 1.89 ms. A
build of 14 panels fills the 16 layers with nothing in the projection layer: 0.14 ms, 10.5 percent
and 0.65 ms. The same 14-panel build of the code before the budget gives the same three numbers, so
a frame within the limit costs nothing that the runtime measures. The full demo with both scripted
controllers needs 19 layers. Its projection layer draws 3 handles, and every panel and surface keeps
its own layer, the clip's video surface included.

### Measured

On the Android XR emulator (runtime "Android XR" 1.0.0, system "Moohan", GLES) the demo's torus
shows lit and upright in the place of its pose, with its handle under it, beside the panels. The
runtime offers `XR_KHR_composition_layer_depth`, `XR_FB_composition_layer_depth_test` and
`XR_FB_composition_layer_image_layout`. Its swapchain formats are `GL_SRGB8_ALPHA8`, `GL_RGBA8`,
`GL_RGB10_A2` and `GL_RGBA16F`, so it offers no depth format and the layer carries no depth there.
Its eye pictures are 2560 by 2558 pixels. With the torus moved through the counter panel's plane the panel, which a later layer draws, covers
the torus, as the order without a depth test says. The emulator's runtime offers no depth format,
so the depth test has run only on a Quest.

On a Quest 3 (Horizon OS, runtime "Oculus" 210.137.0) the runtime takes a `Depth24PlusStencil8`
swapchain for the depth. The screenshot that `adb exec-out screencap -p` gives holds both eyes as
the compositor draws them. The builds `XR_DEMO_VOLUME=through` and `XR_DEMO_VOLUME=alone` show a
torus that a panel pierces and the same torus alone. `scripts/dev/xr_depth_check.py` compares the
torus's pixels in the two pictures:

| Panel | Share of the ring that shows, left and right eye |
| --- | --- |
| At the ring's center, with the projection layer's depth write | 0.68 and 0.68 |
| At the ring's center, without that write | 0.00 and 0.00 |
| 200 mm nearer to the head than the ring | 0.00 and 0.00 |
| 200 mm farther from the head than the ring | 0.98 and 0.98 |

With the write the near part of the ring shows over the panel and the far part is hidden.
The crate's tests run the scene on the GPU of the development machine: two eyes from one
composition, the depth that the scene leaves, a volume that moves, and a frame with no volume.

## The room around the panels

An app asks for the room through `SpatialEnvironment` when `is_passthrough_control_enabled()` is
on. The backend picks how the runtime shows it:

- `XR_FB_passthrough` (Meta Quest): a passthrough layer under all other layers. The backend
  makes and starts it once. It joins a frame while the opacity is above 0, and the opacity is
  the layer's style.
- The blend mode `ALPHA_BLEND` (Android XR, Pico, Magic Leap): the frame's blend mode is
  `ALPHA_BLEND` while the opacity is above 0 and `OPAQUE` otherwise. This path has no dimming.
- Neither: the capability is off and the app's own background shows.

## A subspace that follows the user

A `Subspace`'s origin is a meter ahead of where the head was when the app started, in the room's
LOCAL space. A `FollowingSubspace` has an origin that follows a target. The panels read the origin
from a composition local, so every panel and surface of the subspace uses it. A `SpatialCurvedRow`
uses it too.

The local holds the id of the following subspace and the identity pose. It does not hold the live
pose. Each panel keeps its pose relative to that origin. The frame loop calls `Space::track` once
in each frame, after it sampled the controllers and before the pointer reads the panels. The call
takes the head's pose, the pose of each controller and the recenter. It computes the origin of each
following subspace. For a subspace whose origin moved it sets the pose of each of its panels to the
origin times the panel's own pose. A frame costs one origin per following subspace and one
multiplication of poses per panel of a subspace that moved. It allocates nothing and writes no
state, so a moving target does not compose the panels again. A composition that draws later in the
frame reads the live origin in the `place` of each panel. It does not move the panel back.

A target and a mode make six cases:

- `View(Snap)`: the origin is one meter ahead of the head along its heading, at its height,
  facing it. The headset has no head pose at the first composition. The subspace stands at the
  plain origin until the first frame in which the runtime tracks the head. A subspace that
  composes while the head is tracked takes its place at once. A recenter takes effect at the
  `change_time` of its event, which can lie after the frame that reports it. The loop places the
  subspaces again in the first frame at or after that time.
- `View(Tight)`: the same origin in every frame. A frame in which the runtime loses the head keeps
  the origin of the frame before.
- `Controller(hand, Tight)`: the origin is the place of the hand's grip pose, with no offset.
- `Controller(hand, Snap)`: the origin is that place in the first frame that tracks the controller.
  A recenter places it again. The panels do not show before the first place.
- `Anchor(anchor, Tight)`: the origin is the anchor's pose in every frame, with all six degrees of
  freedom, because an anchor on a wall holds a panel that lies against it. A frame in which the
  runtime does not place the anchor keeps the origin of the frame before.
- `Anchor(anchor, Snap)`: the origin is the anchor's pose in the first frame that has one. A recenter
  places it again. The panels do not show before the first place. An anchor that the app created has
  its place at once, and one that the app loaded from storage waits for the runtime to recognize the
  room. A detached anchor leaves the panels where they were.

For a view and a controller only the target's turn about the vertical axis counts, so the panels stand upright. A ball that
sits 10 cm above a controller stays straight above it when the hand tilts. Jetpack XR's
`TrackedDimensions.All` would tilt the ball with the hand. The turn is the heading of the pose's
forward axis seen from above. A pose whose forward axis is within about 6 degrees of vertical has no
heading, so the origin keeps the last one.

A controller gives its place from the grip pose and its turn from the aim pose. The OpenXR grip
pose has its forward axis along the controller's handle. The handle points steeply upward when
the hand holds the controller as usual, so the grip's heading would flip by 180 degrees when the
hand rises a few tens of degrees. A quad layer shows its front face only, so a flipped panel would
vanish. The aim pose points where the ray points. That direction stays steady. A controller counts
as tracked when the runtime tracks both poses.

A panel that the user carried keeps its pose relative to the origin, so it moves with the origin.
A panel that another following subspace takes over, as when the app names a new anchor, is placed by its modifier again, because the place that the user carried it to is relative to the old origin.

A following subspace of a controller has no place while the runtime does not track the controller.
The loop marks each of its panels `Ride::Hidden`. The panel stays registered with its swapchain
image, so it shows again at once. The end of the frame leaves it out of the layers. The pointer
ignores it. A panel that rides a controller is `Ride::On(hand)`. The pointer skips it in the ray of
that hand, for hits and for handles, because a ball above the left controller would stop the left
ray at the user's own hand. The other hand's ray meets it like any panel. The filter is one function
of `hit.rs`. `nearest_hit` and `nearest_handle` both use it, so the cursor, the press, the grab
and the length of the beam agree.

A panel with rounded corners and no background around them takes the shape of its corners. The
composition draws the atlas on a transparent background with premultiplied blending. The quad
layer's flags name source alpha only, so the compositor reads the image as premultiplied. A 40 by
40 dp panel with `rounded_corners(20.0)` shows as a circle.

## Carrying a panel

A `movable` panel or surface has a handle under it: a sphere 4 cm across, 2 cm below the panel's
bottom edge. A controller's ray that points at the handle grabs the panel when its trigger goes
down, and the panel follows the controller until the trigger goes up. The thumbstick pushes it
away or pulls it near. The ray meets the sphere from any side, so a user who stands behind the
panel grabs it as well. The handle is a quad layer with one static picture of a lit sphere. The
quad turns its face to the head in each frame, so the handle looks round from every side and costs
no draw.

`movable_with(MovePolicy)` follows Jetpack XR's `movable(movePolicy = ...)`. `MovePolicy::system`
lets the headset move the panel and calls `on_move` with each step. `MovePolicy::custom` only
reports the steps, and the app moves the panel by setting its `SubspaceModifier`. A
`SpatialMoveEvent` has the phase (`Start`, `Moving` or `End`), the new pose and the pose of the
event before. The pose is in meters and relative to the origin of the panel's subspace. In the frame
loop the pointer makes one step for the grab, one for each frame in which the panel's pose changes
and one for the release. A controller that the runtime stops tracking ends the carry with an `End`
event at the last pose. The callbacks run on the thread of the composition, with no borrow of the
space held.

The panel keeps the pose that the user gave it, relative to its subspace's origin, until it leaves
the composition. `SubspaceModifier::with_pose` takes the pose of an event, so an app that saves the
last `End` pose places the panel there when it starts again.

## Resizing a panel

A panel or surface with `resizable` or `resizable_with` has a second handle at its bottom right
corner. It is a sphere of the carry handle's size: a quad layer with one static picture of a lit
sphere, amber where the carry handle is bluish white. Its center stands 4 cm to the right of the
panel's right edge and 4 cm below its bottom edge, so the grab sphere stays clear of the panel's
rectangle. A press next to the handle, on the panel's corner, still presses. A ray that points at
the handle takes priority over the panel, as the carry handle does. A panel that carries and
resizes has both handles, and the one that the ray meets first is active and shows larger. A
panel on a curve has no corner handle, because its corner does not lie on the tangent plane that
the resize measures on.

A pull on the handle grabs the corner. While the trigger stays down, the pointer takes the point
where the ray meets the panel's plane, with no clip to the panel's rectangle. The panel's center
stays where it is, so the pose does not change. The corner moves by the shift of that point, and
the size changes by twice the shift. The grab keeps the offset between the ray's point and the
handle's center, so the first step does not jump. A ray that is parallel to the plane, or that
points away from it, leaves the size as it was. The pointer rounds each size to whole dp, and it
reports a size only when it differs from the size before. A hand that holds still reports nothing.
The pointer reads the ray after the aim filter, which cuts the shake of a resting hand and follows
a fast turn with little lag.

`ResizeLimits` holds the parameters of Jetpack XR's `resizable` that are not the policy: the
smallest size, the largest size and `maintain_aspect_ratio`, which defaults to false as in
Jetpack XR. Each side is clamped to its limits after the rounding. The pointer never makes a side
smaller than 20 dp, so that a minimum of zero leaves a panel that the user can still grab. It
never makes a side larger than 4000 dp, because a ray that grazes the panel's plane meets it far
away and a maximum of infinity would take that size to the atlas. A maximum below the minimum
becomes the minimum. With `maintain_aspect_ratio` the pointer takes the
ratio of the size at the grab and clamps one scale factor of that size, so the size stays on the
line of its proportions. Limits that the proportions cannot meet hold, and the proportions give
way. Surfaces follow the same rule and default to a free ratio. The picture of a decoder fills the
whole surface, so a free resize stretches it. A surface that shows a video of one ratio calls
`maintain_aspect_ratio(true)`, as the demo's clip surface does.

`resizable_with(limits, policy)` follows Jetpack XR 1.0.0-beta02's `resizable(enabled,
minimumSize, maximumSize, maintainAspectRatio, resizePolicy)`. Rust has no named or default
arguments, so the three limits come in one `ResizeLimits` value, and `resizable(policy)` is the
call with no limits. Jetpack XR's `enabled` is the app's own `if`. The alpha releases had
`onSizeChange: (IntVolumeSize) -> Boolean` in its place, which `ResizePolicy` replaced.
`ResizePolicy::custom` is the shape of `MovePolicy::custom`: the headset reports the steps, and
the app applies the size. Jetpack XR's `ResizePolicy.system`, where the headset applies the size
itself, has no counterpart. A `SpatialResizeEvent` has the phase (`Start`, `Resizing` or `End`)
and the size in dp, where Jetpack XR reports integer pixels. The `Start` event has the size at the
grab. The `End` event has the size of the last step. A controller that the runtime stops tracking
ends the resize. The callback sets the state that feeds `SubspaceModifier::size`, and the next
composition lays the content out at that size and moves the handle to the new corner.

The size of a content panel costs more than the size of a quad. A change of size changes the
panel's part of the atlas, so the panels below it stack again, the atlas grows or shrinks, and the
panel's swapchain is made anew. An app that applies the size in each frame of a drag would pay for
all of that in each frame. A panel in a resize holds a room instead: its size, rounded up to the
next 128 dp whenever the size outgrows the room. The room never shrinks during the resize. The
room, not the size, takes part of the atlas, and the room's pixels make the panel's swapchain. The
layer shows the rectangle of the panel's size from the image's top left corner. The copy takes two
more pixels of the atlas's empty margin, so that the filter at the rectangle's edge finds clear
pixels and no leftover of a larger size. A drag therefore stacks the panels and makes a swapchain
once for each 128 dp of growth in a direction. A drag that only shrinks the panel does neither.
At the release the room settles to the size that the app applied, which stacks the panels once
more and makes one swapchain at that size. The settling runs in the release step, so it does not
depend on a recomposition. The composition draws the content at the new size in each frame in
which the app applies a size. That draw is the cost of any animated layout.

A surface has no part of the atlas, and `SurfaceLayers::sync` makes its swapchain once, at the
panel's size when it appears. A resize changes only the size of the surface's quad, so it costs
nothing. The decoder keeps writing buffers of the size that it got.

No measurement on a device backs this design, and 128 dp and 20 dp are choices. The crate's tests
run the pointer's math and the space's rooms on a desktop. The cost of a swapchain, the
compositor's filter at the rectangle's edge and the feel of the aim filter in a drag have not run
on a headset.

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

A surface has no composable content, so the pointer treats it as a target of its own. The pointer
finds the nearest hit among the panels and the surfaces and keeps one press state for both: the
trigger's thresholds, the point that a press takes from before the pull, and the click that holds
its point against a shaking hand all work as on a panel. Each gesture goes to the target of the hit,
the app's composition or one surface. A press remembers its target, so its release reaches the
pressed surface also when the ray left it or the controller was lost. When the ray crosses from a
panel onto a surface, the composition sees the pointer leave. `on_pointer_event` of the scope
receives Compose's `PointerEvent` with the position in the surface's dp from its top left corner.
The events are `Enter` for the first move over the surface, `Move`, `Down`, `Up` and `Exit`.
Jetpack XR gives such input through the `interactionPolicy` parameter, with `SpatialInputEvent`
positions relative to the surface's center. Rust has no default arguments, and Compose's pointer
events carry the same facts, so the scope takes the callback. A surface with no callback takes no
events and costs nothing for them.

The compositor draws layers in list order, and the last one covers the others. The frame loop
lists the layer of the room and the streamed scenes first. Then come the flat panels, the curved
panels and the surfaces, sorted back to front by the distance from the head to the plane of each
one, so a panel in front of a surface shows over it. The plane's distance keeps a small panel at
the edge of a large surface in front of it, where the distance to the layer's center would put it
behind. A curved panel sorts by the plane at its center. Its arc leaves that plane by 2 cm for a
panel of 0.4 m on a cylinder of 1 m. A distance that follows the arc changes when the head steps
aside, and it would sink a flat panel that stands 3 cm in front of a curved panel under that
panel. The handles of a panel follow the panel's own layers, so a panel in front of a handle shows
over it. The rays follow all of them, so they show over everything. The sort runs in
place on the panels' list and allocates nothing. The session keeps the raw structs of the frame's
layers in lists that it reuses from frame to frame.

## Video decoding

`VideoDecoder` of `cranpose-services` decodes H.264, HEVC and AV1 on the headset's hardware. On
Android it drives `MediaCodec` with the keys for the least delay: no reordering wait, priority
0 and the top operating rate. The app feeds access units from one thread. A thread of the
decoder drains the pictures. The feeding thread waits in `submit` for a free input buffer, up to
`DecoderConfig::input_wait`, 30 ms by default: a busy Quest 3 refused pictures at 4 ms. The wait
blocks the feeding thread only. The frame loop reads pictures through `DecodedFrames` and waits
for no decode. `submit` answers `DecoderError::Full` after the wait and
`DecoderStats::refused` counts it. Each picture returns with the timestamp and
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

`VideoDecoder::capabilities(codec)` gives the limits of the decoder that `VideoDecoder::new`
opens for a codec: its name, whether it runs on dedicated hardware, its largest width and height,
its highest bit rate, and through `DecoderCapabilities::max_frame_rate(width, height)` its frame
rate at a size. A picture beyond a limit decodes with errors, so a streaming app reads the
limits before it asks its host for a size. On Android the limits come from `MediaCodecList` and
`MediaCodecInfo.VideoCapabilities` through the `CranposeVideoCodecs` Java class, because the NDK
has no codec list at the API levels that headsets run. The query runs when an app asks for it.
The first decoder of the list that supports the MIME type is the one that `MediaCodec` opens, so
the limits are its own, and not the largest of all decoders. VideoToolbox on macOS and iOS has
no query for sizes, rates or bit rates. Its answer holds the name, `VTIsHardwareDecodeSupported`
for the codec, and `None` for each limit, which the type documents. AV1 reports
`DecoderError::NoDecoder` there, as the decoder does.

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
- `device_linear_velocity` and `device_angular_velocity`: how fast the head moves and turns, from
  the same location of the VIEW space, along the axes of the LOCAL space. Each is `None` while
  the runtime marks it invalid.
- `stage_pose`: the OpenXR STAGE space located in the LOCAL space, or `None` when the runtime
  lists no STAGE space or does not locate it. The stage's origin is on the floor at the center of
  the play area, so `floor_height()` gives the floor's height in the room.
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

## Tracking hands, body, face and eyes

An app reads the user's hands, body, face and eyes in each frame, with Jetpack XR's names where
Jetpack XR has them. Four kinds of tracking are opt-in. An app that asks for none pays nothing:
the instance enables no tracking extension, no tracker exists, no call happens in a frame and the
APK holds no tracking permission.

### How an app asks

The extensions of an OpenXR instance are fixed when the instance is made. Cranpose makes it before
the content composes, so a modifier or a composable cannot ask for one. Jetpack XR's
`Session.configure(Config(handTracking = ...))` runs after the session exists. Cranpose reads
the build script's declaration instead, which also writes the Android permissions:

| Declaration | Feature | OpenXR extensions | Meta Quest permission | Android XR permission |
|---|---|---|---|---|
| `Use::hand_tracking()` | `TrackingFeature::Hands` | `XR_EXT_hand_tracking`, `XR_FB_hand_tracking_aim` | `com.oculus.permission.HAND_TRACKING`, `horizonos.permission.HAND_TRACKING` | `android.permission.HAND_TRACKING` |
| `Use::body_tracking()` | `Body` | `XR_FB_body_tracking`, `XR_META_body_tracking_full_body` | `com.oculus.permission.BODY_TRACKING` | `android.permission.BODY_TRACKING` |
| `Use::face_tracking()` | `Face` | `XR_FB_face_tracking2`, else `XR_ANDROID_face_tracking` | `com.oculus.permission.FACE_TRACKING`, `horizonos.permission.FACE_TRACKING` and `android.permission.RECORD_AUDIO` | `android.permission.FACE_TRACKING` |
| `Use::eye_tracking()` | `Eyes` | `XR_FB_eye_tracking_social`, `XR_EXT_eye_gaze_interaction` | `com.oculus.permission.EYE_TRACKING`, `horizonos.permission.EYE_TRACKING` | `android.permission.EYE_TRACKING_FINE` |

The permission names come from Meta's Move API page and Android's XR permissions page. A Quest 3
on Horizon OS (Android 14) lists them with `pm list permissions`: the hand and body permissions are
`normal`, which the system grants at install, and the face and eye permissions are `dangerous`. The
Quest defines the hand, face and eye permissions under both the `com.oculus.permission` and the
`horizonos.permission` names, in one permission group, and the body permission under the first
only. Meta's body page was not reachable, so only the device vouches for `BODY_TRACKING`. A Quest 3
estimates the face from the microphone, so face tracking needs `RECORD_AUDIO`. Each declaration also
names an optional Oculus feature, written as not required.

The app passes the declaration to its launcher with `with_capabilities(&CAPABILITIES)`, and the
backend reads it before `xrCreateInstance`. `TrackingRequest` holds the four answers.
`ExtensionOffer::enabled_for` keeps from the runtime's list the extensions that the request needs
and no others. An extension comes only with the one it extends: the aim extension needs hand
tracking and the full body needs body tracking. A face comes from one extension, Meta's before
Android's.

### What the runtime offers

`local_tracking_capabilities()` returns the `TrackingCapabilities` of the session's start, in the
pattern of `local_spatial_capabilities()`. The runtime answers a system property structure only for
an extension that is enabled, so what it can say depends on what the app asked:

- A feature that the app did not ask for is `Offered` when the runtime lists an extension for it,
  and `Unavailable` otherwise. A listed extension does not prove that the headset has the sensor.
- A feature that the app asked for also gets the system's answer
  (`supportsHandTracking`, `supportsBodyTracking`, `supportsFullBodyTracking`,
  `supportsVisualFaceTracking` or `supportsAudioFaceTracking`, `supportsFaceTracking`,
  `supportsEyeGazeInteraction`, `supportsEyeTracking`). A Quest 3 lists the eye extensions and has
  no eye cameras, so its eyes become `Unavailable` when the app asks.
- The log holds the request and the capabilities once, at `cranpose xr: asked for`.

`XrFrame::tracking` holds the live value: `AwaitingPermission` until the user answers,
`PermissionRefused` after a refusal and `Active` once the tracker runs.

### Permissions

The Java class `CranposePermissions` comes with any of the four declarations. The backend calls
`cranpose::android_permission` and `cranpose::request_android_permissions` through it, which are
`cranpose`'s public path to a run-time permission that differs between devices. Each feature has
groups of alternative permissions: any one name of a group grants it, because a Quest defines the
`com.oculus` and `horizonos` names and Android XR the `android` ones. The Java side reports 0 (not
asked), 1 (granted), 2 (refused) or 3 (refused for good, which Android shows no prompt for any
more). A group of which the device defines no permission counts as granted, so a headset that gates
its trackers by no permission gets them at once.

The prompt comes before the session. On a Quest 3 the system takes an immersive app off the headset
to show a dangerous permission's prompt, and the runtime ends a session that is running: the
session went from `VISIBLE` through `STOPPING` and `IDLE` to `EXITING` in 30 ms, and the app's loop
ended. So `ask_before_session` runs before the instance is made, after the loader listed the
extensions. It requests, in one call, every missing permission of every feature that the app asked
for and the runtime offers, so a Quest 3 does not ask for the eyes it lacks. It waits until each feature is
answered, the activity ends or five minutes pass. The wait handles the activity's events, so the
app stays responsive.

`PermissionStep` is the decision for a feature: `Ready` when every group is granted, `Wait` while a
group has no answer and `Refused` when the user refused a group. The backend does not ask again
after a refusal. The tracker is made only at `Ready`: Meta's `xrCreateFaceTracker2FB` succeeds
before the user grants the permissions, and a tracker made then stays empty, so waiting for the
refusal of the runtime would not work. After the prompt, a frame does no work for a feature that
still waits, except once a second: it reads the answers and makes the tracker when they are
granted, which is how a grant in the system's settings takes effect during the session. After the
tracker runs or the user refuses, the feature costs no more than the frame's own work. At `Ready`
a refusal other than `XR_ERROR_PERMISSION_INSUFFICIENT` means that the runtime has no such tracker,
so the feature becomes `Unavailable`.

### What a frame holds

Every field of `XrFrame` is a fixed-size array or a plain value, so the frame loop allocates
nothing:

- `HandState`: `tracking_state` (Jetpack XR's `TrackingState`), 26 `HandJoint`s (`pose`, `radius`,
  `valid` and `tracked`) at the index of each `HandJointType`, and `aim`, the `HandAim` of
  `XR_FB_hand_tracking_aim`: the aim pose, the `Pinch` (strength and a flag) of each finger and the
  system gesture. `HandAim::ray()` gives the ray along which the hand points, so a hand works as a
  pointer. Jetpack XR's `Hand` is the tracked hand and its `HandSide` the side. Cranpose's `Hand`
  has been the side since the controllers came, so the tracked hand is `HandState`, Jetpack XR's
  `Hand.State`.
- `BodyState`: `active`, `confidence`, `joint_count` (70 for the upper body and the hands, 84 with
  the legs and feet) and 84 `BodyJoint`s at the index of each `BodyJointType`, which have Meta's
  names. Jetpack XR has no body API.
- `FaceState`: Jetpack XR's `Face.State`, with a weight for each of its 68 `FaceBlendShapeType`s and
  a confidence for each `FaceConfidenceRegion`. `XR_ANDROID_face_tracking` writes it directly. For
  `XR_FB_face_tracking2` the backend maps the 63 shapes that both share, and the tongue's `Out`,
  by index. The tongue shapes `Left`, `Right`, `Up` and `Down` stay at 0 there. Meta's two
  confidences go to the lower region and to both upper regions.
- `MetaFaceState`: every weight of `XR_FB_face_tracking2`, by `FaceExpression`, with its two
  confidences and its `FaceDataSource`: `Visual` from cameras, `Audio` from the microphone, as a
  Quest 3 gives. `FaceExpression::blend_shape` and `FaceBlendShapeType::expression` map between
  the two names.
- `Eyes`: an `EyeState` for each eye (`XR_FB_eye_tracking_social`: pose and confidence), and the
  combined gaze pose of `XR_EXT_eye_gaze_interaction`. Jetpack XR's `Eye.State.isOpen` has no
  source in these extensions.

`XR_EXT_eye_gaze_interaction` is an action, so the backend declares its action set before the
session attaches its sets, with the controllers', and syncs both in each frame. A tracker that
the runtime does not locate leaves its state at rest and `Paused`. While the app has no input
focus every tracked state is at rest.

### The cost of a frame

An app that asked for nothing returns from `Trackers::sample` after one check. An app that asked
pays one call for each tracker it runs: two `xrLocateHandJointsEXT` (the aim state rides in the
same call), one `xrLocateBodyJointsFB`, one `xrGetFaceExpressionWeights2FB` or `xrGetFaceStateANDROID`,
and one `xrGetEyeGazesFB`, or a located space for the gaze. The arrays live in the frame or on the
stack.

### Two pointers, every button

A user holds two controllers, and each works on its own. Before this design the left controller
could not press or drag on a panel while the right controller carried another panel by its handle.
Four layers each held the state of one pointer:

1. `pointer.rs` kept one `hand` index, one `press`, one `held` panel and one `over` target.
   `choose_hand` returned at once while a press or a held panel existed, so the left hand's aim was
   read and thrown away until the right hand let go.
2. The frame loop sent the events through `AppShell::set_cursor`, `pointer_pressed` and
   `pointer_released`, which drive the window's one primary pointer with one cursor, one set of held
   buttons and one set of hovered nodes. `secondary_pointer_*` takes a second finger, but it routes
   the finger to the primary pointer's captured nodes, so it cannot press another panel. Every panel
   with content lives in one atlas, which is one surface of the shell.
3. `Space::set_cursor` held one dot, `SurfaceInput` held one pressed flag, and the runner kept one
   pressed surface and one active handle.
4. The UI nodes that track a gesture, the press and hover interaction sources, the scroll gesture and
   the long press, skipped every pointer whose id was not `PointerId::PRIMARY`. A pointer that does not
   share the window's own id would have pressed and then scrolled nothing.

The design gives each layer a pointer of its own:

- `Pointer` (`pointer.rs`) is the state of one hand: its `Reach` (the ray, the panel and the handle
  that it meets, `reach.rs`), its `Press`, its held panel (`pointer/held.rs`) and its `over` target.
  `Pointers` (`pointers.rs`) holds the two of them and runs the left hand and then the right hand in
  each frame, so both read the same panels. The only thing they share is a claim: the panel that one
  hand holds by a handle has no handle for the other hand. `Gestures` gathers the frame's output: the
  gestures for the composition, tagged with their hands, the gestures for surfaces and volumes, and the
  steps of the carries and resizes. The storage lives across frames, so a frame allocates nothing.
- `Hand::pointer_id` gives each hand its id: 1 for the left hand and 2 for the right hand. The
  primary id 0 stays with a window's own mouse or finger.
- `AppShell::independent_pointer_moved`, `_pressed`, `_released`, `_scrolled` and `_exited` (and the
  same on `SurfaceMut`) drive a pointer with its own place, hovered nodes, held buttons and gesture,
  keyed by its `PointerId`. The first press hit-tests at the pointer's place and keeps the reached
  nodes as that pointer's path in the `HitPathTracker`, as `ray_pointer` does for a ray in a 3D scene.
  The moves and the release reach the path. Without a press, the pointer hovers: exit, enter and move
  events with its id. The primary pointer's code shares the function that sends the exits and enters.
  `atlas_input::deliver` sends a frame's atlas gestures to the shell, and the integration tests call
  the same function.
- `PressTracker` and `HoverTracker` of `interaction.rs`, and the scroll gesture of
  `modifier/scroll/drag_gesture.rs`, hold the pointer that went down first and let the others pass by
  until it lifts, as `clickable` did. A hover lasts while any pointer is over the node. On a phone the
  first finger is the primary pointer, so the touch behavior stays the same. The long press and the
  zoom gesture still take only the primary pointer, so a long press with a controller does not fire.
- `Space::set_cursor(hand, cursor)` shows one dot for each hand on a panel. `SurfaceInput` is kept for
  each hand, so a surface's callback gets each hand's events under that hand's id. The runner reads
  `pressed_surfaces` and `handles` as two entries, so each hand highlights its own handle.

Two hands may press one panel. The panel's nodes get a `Down` of each id, as two fingers make, and a
node that holds a gesture serves the first pointer.

**Buttons.** `Aim` carries the squeeze, the two face buttons, the thumbstick click and the menu
button next to the trigger. The trigger keeps the pointer's press machine: the press holds the point
from before the pull, it clicks through a shake, and it drags when the ray turns. The button that the
press emits comes from `PointerMapping`. The other inputs are extra buttons (`pointer/extras.rs`). Each
goes down at the pointer's place, to the target of the trigger's press, or else to the target that
the ray is over, and it comes up at the same target. The shell sees a `ButtonDown` or a `ButtonUp`
for a button other than the primary one and keeps it in the pointer's held set, so the moves while it
is down reach the nodes that took it. The thumbstick scrolls the target under the pointer in each
frame by `scroll_speed` times the frame's time, with a dead zone of 0.15 and no step longer than 100 ms.
A positive y scrolls up as a wheel does, and a push to the right moves the content to the left.
A hand that carries a panel gets no extra button and no scroll: the stick pushes the panel.

`Controller::trigger.pressed` and `squeeze.pressed` come from the value with a mark of 0.6 to go down
and 0.4 to come up (`press_threshold.rs`), which the pointer's own trigger rule uses as well.
`Controllers::sample` computes them from the frame before. A profile whose trigger is a click gives 0
or 1, so no `trigger/click` path is bound: a path that one profile does not own makes the runtime
reject that profile's whole list. The pointer-only fallback list gained the stick's x axis, because
the scroll needs it.

**Settings.** `PointerSettings` is the handle that `local_pointer_settings()` gives. It is an
`Arc<Mutex>` of the mapping and a visibility flag, like `Haptics`. The frame loop reads it in each
frame (`HeadsetInput::pointer_choice`), so an app changes the mapping while it runs, and the tests
of two apps in one process do not share a global.

**Beams.** A hand's beam exists only while some panel is in reach of that hand (`Reach::in_reach`:
a panel that is not `Ride::Hidden` and does not ride that hand's own controller) and while the app
lets the beams show. `RayLayers::layers` makes a layer for each `Some` beam, so a Subspace with no
visible content submits none.

**The default mapping.** The trigger is the primary button, the thumbstick click is the middle
button, the first face button (A on the right hand, X on the left) is the secondary button, and the
stick scrolls at 800 dp each second. The grip is left free: Meta's controller guidance gives it to
grabbing, and an app that grabs an object with it would also right-click. The thumb is free while the
fingers hold the controller and point. The mapping rests on these two reasons. No convention of
the desktop apps for headsets is confirmed.

**Scripted controllers.** `ScriptedController` (feature `scripted-hand`) writes the whole
`Controller` of a hand after `Controllers::sample`, with the Touch Plus profile, so a hand's
`pointer_aim` takes the controller path even when the runtime reports the hand interaction profile.
Steps move the controller and its aim in straight lines, which makes a drag and a carry possible. A
script for each hand plays at once (the scripted hand has a slot for each hand as well).

**Tests.** `src/tests/two_pointer_tests.rs` runs a real composition in a headless window and the
frame loop of the headset: two hands drag two lists, press one panel, carry a handle while the other
presses, right click, scroll, remap the buttons, mix a hand with a controller, and hide the beams.
`crates/cranpose-testing/tests/independent_pointer_test.rs` tests the shell's calls alone.
`src/tests/scripted_controller_tests.rs` plays two scripted controllers through the pointer: a carry,
a right click and a scroll at the same time.

Run on the Android XR emulator (system image android-34 `google-xr`, runtime "Android XR", system
"Moohan"), with the demo built with `XR_DEMO_SCRIPTED_CONTROLLERS=two`: the session reaches `FOCUSED`
(the permission prompt for hand tracking was declined), the log
reports the `MetaTouchPlus` profile and `Controller` pointers for both hands, and screenshots show the
following. The counter panel and the readout panel rise and come back together, with both triggers at
100 percent in the readout, each beam on its own handle and both handles drawn larger. The readout
panel's pad counts the secondary and the middle press (`Right 4, middle 2` after two script cycles), and
its list scrolls under the right stick (`scrolled 320 dp` at a lean of 0.3), while the left controller
presses the counter's button (`Pressed 6 times`). The pad and the list stand between the pattern surface
and the torus's volume: a ray at the panel's top meets the pattern surface first, and a ray at its
bottom meets the torus first, and a surface or a volume stops the ray as a panel does.

With the controllers on, the frame has 16 layers of the 16 that the runtime takes, and the code of
that time, which had no layer budget, left 3 panels out. The clip's video surface is gone from the
picture, which fits the back-to-front order: it stands farthest from the head. The layer budget of
"The layer limit" shows every panel of that frame. The other two layers were not identified. With no controller active, the same frame has 16 layers and none left
out, because no beam shows. On a Quest 3 the system's "Finding position in room" dialog held the launch of the demo, and a press
clears that dialog, so no frame ran there. The layer count of a Quest 3 and the buttons of a real
controller are not measured. The long press and the zoom gesture work with a
pointer that is not the primary one.

### Hands as pointers

The pointer of each hand (`pointer.rs`) takes an `Aim` for its side: a ray, a pose, a trigger value,
the stick's lean and the other buttons. `pointer_aim.rs` chooses the `Aim` of a side in each frame, from that side's `Controller`
and `HandState`:

1. A controller whose aim the runtime tracks, and whose profile is not a hand profile, gives the
   `Aim` with source `InputSource::Controller`.
2. Otherwise, when the app declared `Use::hand_tracking()`, a hand whose `HandState` is `Tracking`
   and carries a `HandAim` gives the `Aim` of that aim pose, with the pinch strength of the index
   finger as the trigger and source `InputSource::Hands`. `XR_FB_hand_tracking_aim` fills it.
   A hand that makes the system gesture or presses the menu gives none.
3. Otherwise, when the side's profile is `ExtHand` or `MicrosoftHand`, the declaration is made and
   the cameras track the hand (`Tracking`) with no `HandAim`, the controller's actions give the
   `Aim`: the hand interaction profile binds the hand's `aim` pose and its pinch
   (`aim_activate_ext/value`) to the same actions as a controller's. A runtime that tracks the
   joints and lacks the FB aim extension reports the hand this way. A hand interaction profile
   never counts as a controller in rule 1. It gives no aim while no hand is tracked: a Quest 3
   reports the profile for both hands at the desk with a valid, active aim pose and a pinch value
   near 0, so a pointer from it would show two rays from nowhere. The first build of this change
   showed `Pointers L/R: hands / hands` there, with no hand in view, and the gate removed them.

`Reach::update` in `reach.rs` remembers the source of its last aim. A different source in the next
frame resets the side as a lost aim does: the press ends with a `Release`, a carry or a resize
ends, and the trigger of the new source counts as held from before if it is down. A hand that
appears with a closed pinch gets the same treatment, so a hand that the cameras lose and find, or
that ends the system gesture with its fingers together, does not press by surprise. A controller
that appears with its trigger down still presses, as before.

The ray, the aim filter, the press rules, the handles and the beam are the controller's. The beam
starts at the hand's aim pose. A hand has no thumbstick and no button besides the pinch, so a carried
panel follows the aim pose and cannot be pushed or pulled, and the pinch is the trigger's pointer
button alone. `Haptics` asks for a side that a hand points for never reach
`xrApplyHapticFeedback`.

`HeadsetInput::sample` writes the chosen source of each side into `XrFrame::left_pointer` and
`right_pointer` before the app's frame effects run. The demo's pointer line reads them.

On a Quest 3 at the desk with the right Touch Plus controller awake, the demo reports
`Pointers L/R: none / controller`: the controller points, and the hand interaction profile of the
left hand gives nothing. On a Quest 3 and in the Android XR emulator the runtime lists
`XR_FB_hand_tracking_aim` and `XR_EXT_hand_interaction`. The emulator reports the `ExtHand` profile for both hands at rest.
Neither runtime was shown a hand of its own. No hand is in the Quest's view at the desk. The
emulator release in use (37.2.13 with system image revision 7) rejects the hand messages that its
own window sends to the guest ("Received unexpected message type from simulated input client"), so
its hand simulation does not run.

The `scripted-hand` feature of `cranpose-xr` plays a hand in place of the cameras'. `script_hand`
installs a `ScriptedHand`: an aim pose that points at a place in the room, and a `PinchCycle` of
rest, closing, held and opening times for the index finger. `Trackers::sample` writes it into the
frame's `HandState` of its side right after the trackers wrote the real hands, so the pointer's
choice of input, the ray, the press and the button take it by the path of a tracked hand. The
script stands for what a hand reads. What a real hand's pinch strength and aim pose do while the
fingers close is untested. The feature is off by default, and the backend has no such code then.

The selection rules and the pointer's behavior with hand states are covered by
`src/tests/pointer_aim_tests.rs`, `src/tests/pointer_tests.rs` and `src/tests/scripted_hand_tests.rs`.

### The demo

`apps/xr-demo` shows the tracking in its counter panel. The section lists each feature with its
availability and what the frame gives, and it draws a map of the hands: a dot for each fingertip
that the cameras track, seen from the front. The map is part of the panel. The compositor of a
Quest 3 takes 16 layers at most (`xrEndFrame` fails above that, although the system properties
report 32), and the demo already submits that many, so a panel for the section would put another
panel into the projection layer. The demo's volume takes two of them, one for the 3D content and one for its handle. A panel for each fingertip would add ten layers, and a hand's own pointing ray would stop
at the dot at its fingertip. The build script asks for hand tracking, which needs no prompt on a
Quest. `XR_DEMO_TRACKING=none` asks for nothing, and `XR_DEMO_TRACKING=all` adds the body, the face
and the eyes, which the system asks the user to allow.

`XR_DEMO_SCRIPTED_HAND=right` (or `left`) builds a scripted hand that pinches the counter's button
in a cycle of three seconds: two seconds at rest, 0.2 seconds closing, 0.6 seconds held and 0.2
seconds opening. In the Android XR emulator the counter counted from 5 to 9 between two
screenshots, with the beam running from the hand's aim pose to the button and the panel's line
"Pointers L/R: none / hands". The panel's pointer line reads `XrFrame::pointer`, and the log's
once-a-second line holds the pointers and the hands' aims.

The full demo shows the room around the headset from its start. A screenshot of the compositor then
holds a picture of the room, which a device check must not keep. `XR_DEMO_PASSTHROUGH=off` starts the
demo with the room hidden, so that its screenshots hold the panels on black. A check can test the
picture for it: in the screenshots of the builds `XR_DEMO_PASSTHROUGH=off`, `XR_DEMO_VOLUME`,
`XR_DEMO_CONTROLLERS=only` and `XR_DEMO_ROOM=script`, 46 to 99 percent of the pixels are exactly black
and under 0.4 percent are a dark grey (a brightest channel from 1 to 23).
`scripts/dev/xr_vrapi_stats.py` reads the frame rate, the app's time for a frame and the GPU's share
from the line that a Quest's runtime logs each second for the app.

`XR_DEMO_VOLUME=through` builds a scene of a panel and a torus that pierces it, a meter ahead of
the head where the demo started, and shows nothing else. A compositor that tests depth shows the
ring's near part over the panel and hides the rest. One that does not shows the panel over all of
it. `XR_DEMO_VOLUME=alone` builds the same scene without the panel. The log names, once for each
change, whether the volumes' layer carries depth, how many of the frame's layers test it, how many
write it and how many panels the frame leaves out.

### Measured on a Quest 3

On a Quest 3 (Horizon OS, runtime "Oculus" 210.137.0) at the desk, with no hand in view of the
cameras:

- An app that declares nothing sees `XR_EXT_hand_tracking` and `XR_FB_hand_tracking_aim` in the
  runtime's list, and none of the body, face and eye extensions. The runtime lists those only to an
  app whose manifest names their permissions, so such an app reads them as `Unavailable`.
- An app that declares all four sees the hand, aim, body, full-body and `XR_FB_face_tracking2`
  extensions, and no eye extension. The eyes are `Unavailable`, because the Quest 3 has no eye
  cameras.
- The prompt for the face and the microphone showed before the session started, and the app waited
  with the face granted and the microphone not. It started the session when the second permission
  was granted. A prompt that comes while the session runs ended the session in 30 ms.
- The hand, body and face trackers run. The body gives 84 valid joints, with the legs estimated.
  The face gives valid weights with the data source `Audio`. The frame effect runs 120 times a
  second.

The desk shows no hand joints, no aim and no fingertip dots: no hand was in the cameras' view.

## Anchors and planes

An anchor is a place in the room that the runtime holds still while it learns more of the room. A
plane is a flat surface that the runtime found, such as a wall, the floor, the ceiling or a table.
An app uses an anchor to keep content on a table or a wall, and to bring it back to the same place
at its next start. The names follow Jetpack XR's `androidx.xr.arcore` package and its Compose
package.

| Jetpack XR | Cranpose |
|---|---|
| `Anchor.create(session, pose)` | `Anchors::create(pose)` |
| `Plane.createAnchor(pose)` | `Plane::create_anchor(pose)` |
| `Anchor.state`: `Anchor.State` with `pose` and `trackingState` | `Anchor::state()`: `AnchorState` with `pose` and `tracking_state` |
| `Anchor.persist()`, which suspends and returns a `UUID` | `Anchor::persist()`, then `Anchor::persistence()` as an `AnchorPersistence`, and `Anchor::uuid()` |
| `Anchor.getPersistedAnchorUuids(session)` | `Anchors::persisted_uuids()` |
| `Anchor.load(session, uuid)` | `Anchors::load(uuid)` |
| `Anchor.unpersist(session, uuid)` | `Anchors::unpersist(uuid)` |
| `Anchor.detach()` | `Anchor::detach()` |
| `AnchorResult`: `AnchorCreateSuccess`, `AnchorCreateResourcesExhausted`, `AnchorCreateTrackingUnavailable`, and exceptions | `AnchorResult`: `Success`, `ResourcesExhausted`, `TrackingUnavailable`, `Unavailable`, `InvalidUuid` |
| `Plane.subscribe(session)` | `Planes::all()` |
| `Plane.type`, `Plane.state` (`Plane.State`) | `Plane::plane_type()`, `Plane::state()` (`PlaneState`), `Plane::with_vertices`, `Plane::subsumed_by` |
| `PlaneType`, `PlaneLabel` | `PlaneType`, `PlaneLabel` with the same values |
| `FollowTarget.anchor(anchor, mode)` | `FollowTarget::Anchor(anchor, mode)` |
| `MovePolicy.anchor(orientations, semantics, onMove)`, with `PlaneOrientation` and `PlaneSemantic` | `MovePolicy::anchor(orientations, semantics, on_move)`, with the same two types |

Where Jetpack XR passes a `Session`, the app reads a composition local: `local_anchors()` gives
`Anchors` and `local_planes()` gives `Planes`. Both are `None` outside a headset. Jetpack XR's
SceneCore has the same idea as `AnchorPlacement.createForPlanes`, which a `MovableComponent` takes, and
Compose XR has it as `MovePolicy.anchor`. Cranpose follows the Compose name. Jetpack XR reparents a panel to the plane after the snap. Cranpose keeps the panel in its
subspace and gives the app the pose, which the app turns into an anchor. `SpatialMoveEvent` has one
field that Jetpack XR does not have, `room_pose`, which is the pose in the app's room, where
`Anchors::create` takes its pose. Jetpack XR converts between its spaces with `transformPoseTo`.

### How an app asks

An app declares `Use::anchors()` and `Use::plane_tracking()` in its build script, in the pattern of
the tracking services. They are Jetpack XR's `Config.anchorPersistence` and `Config.planeTracking`.
Until the app declares them, the backend enables no extension, makes no call in a frame, adds no
permission and answers `AnchorResult::Unavailable`. The runtime's extension list decides the rest,
as it does for tracking: `AnchorCapabilities` (`Anchors::capabilities()`) and
`Planes::availability()` give a `TrackingAvailability` for each part.

| Service | Meta Quest | Android XR |
|---|---|---|
| `Use::anchors()` | `XR_FB_spatial_entity`, `XR_FB_spatial_entity_storage` and `XR_FB_spatial_entity_query`. The manifest holds `USE_ANCHOR_API` as `com.oculus.permission` and `horizonos.permission`, a normal permission | `XR_ANDROID_trackables` and `XR_ANDROID_device_anchor_persistence`. Anchors need no permission. The storage needs `android.permission.SCENE_UNDERSTANDING_COARSE` |
| `Use::plane_tracking()` | `XR_FB_scene` and the same three. The user allows `USE_SCENE` with the system's prompt | `XR_ANDROID_trackables`. The user allows `SCENE_UNDERSTANDING_COARSE` |

The Horizon OS runtime lists its spatial entity extensions to an app only while the manifest holds
`USE_ANCHOR_API`, and its scene extension needs the same permission. A Quest log of 16 November names
the missing string for each hidden extension. A runtime that lists both families gets Android XR's.
`XR_META_spatial_entity_persistence` is not used: the storage extension of `XR_FB_` keeps and loads
anchors, and the query extension lists them.

A Quest lists the stored anchors some frames after the start. Until then
`AnchorCapabilities::persistence` is `AwaitingPermission`, so an app that loads what it kept waits for the
list. The runtime takes the storage location of a query from the `next` of the query's filter and
refuses a uuid or a component filter that has none with `ERROR_VALIDATION_FAILURE`. A location filter
alone fails the same way, so the list of the anchors is a query with no filter.

The prompt for `SCENE_UNDERSTANDING_COARSE` and `USE_SCENE` comes before the session, as the tracking
prompts do, because the system takes an immersive app off the headset to show it. A feature that the
user has not answered stays `AwaitingPermission`, and the backend looks at it once a second. A
refusal gives `PermissionRefused`. The backend never starts a scene capture
(`xrRequestSceneCaptureFB`), which takes over the headset. A room that the user has not set up gives
no planes.

### Where the state lives

`Anchors`, `Planes`, `Anchor` and `Plane` are handles on one store that the composition makes. The
backend writes the store in each frame and the app reads it, both on the thread of the composition. An
`Anchor` holds a shared cell with its pose and tracking state, a `Plane` holds one with its state, and
both end when the last handle drops, which ends the runtime's space, or when the app detaches the
anchor. The store keeps one state that counts the changes a composition can observe: the tracking
state and the persistence of an anchor, the list of persisted identifiers, and the planes that a
frame adds, removes or relabels. A pose that the runtime refines is not such a change. A frame reads
each anchor's space and each plane's pose with `xrLocateSpace` or the runtime's plane call, writes
them into the cells and allocates nothing. A frame effect that wants the live pose reads
`Anchor::state()`. A following subspace reads the same cell in `Space::track`. The lists of anchors
and planes and the polygon of a plane allocate when the app creates an anchor or the runtime finds a
plane, not in a frame.

### Persistence

`Anchor::persist()` sets the persistence to `Persisting` and the backend passes the ask to the runtime
once. On Android XR, `xrPersistAnchorANDROID` returns the identifier at once, and
`xrGetAnchorPersistStateANDROID` says when the runtime keeps it. On a Quest every step is a request
that an event answers: the anchor first enables its storage component, then the runtime saves it into
the app's local storage, and the save's event carries the identifier. `Persisted(uuid)` follows, and
the list of identifiers is read again. `Anchors::load(uuid)` makes an anchor from storage. Android XR
creates the anchor's space from the identifier. A Quest queries the storage by identifier, takes the
space of the result and enables its location. Until the runtime places the anchor, its state is
`Paused` and a following subspace hides its panels. The storage query of a Quest leaves out entities
that have semantic labels, which are the room's scene. The backend lists the storage once the
permission allows, so `persisted_uuids()` is empty for the first frames of a start.

The room's origin is where the head was at each start, so the pose of one anchor in the room differs
between two starts. The anchor stays at one place in the world. The demo logs the anchor's pose in the
stage as well, which the headset's floor defines and a restart does not move.

### Snapping to a plane

`MovePolicy::anchor` makes `Space::carry` ask the store for a resting place at each step of a carry.
For each plane that the two lists admit and the runtime tracks, the carry measures the distance from
the content's center, moved by half its depth toward the plane, to the plane. The content snaps when
that distance is at most 25 cm and its center lies over the plane or within 30 cm of its polygon's
edge. The nearest plane wins. As in Jetpack XR's `AnchorPlacement`, the content's +Z axis points out
of the plane, so a panel lies flat along it with its center on the plane. A volume rests with its
back face on the plane. A wall holds the content upright and facing into the room, on the side where
the user holds it. A floor, a table or a ceiling holds it with its top edge pointing away from where
it faced, so a panel that faced the user reads from the user's side. The pose is applied while the
user carries, so the content follows the controller again when the user moves it beyond the
distance, and the last pose is the one that the end event reports. The math is `plane_snap.rs`.
Jetpack XR's two entry points differ for an empty filter: SceneCore's `AnchorPlacement.createForPlanes`
anchors nothing, and Compose XR's `MovePolicy.anchor` anchors to any plane. Cranpose follows Compose.

The planes' own frame is Jetpack XR's: the +Y axis of `center_pose` is the normal, +X runs along the
width and +Z along the depth. Android XR gives it so. Meta's scene entity has its rectangle in its
X-Y plane and its normal along Z, so `plane_frame.rs` turns it by a quarter turn about X. A runtime's
sign of the normal is a convention. The label corrects it for horizontal planes: a floor and a table
face up and a ceiling faces down. Meta's labels that Jetpack XR does not name, such as a couch or a
door frame, give `PlaneLabel::Unknown`.

### The demo

`XR_DEMO_ROOM` builds the demo's room panel, in `apps/xr-demo/src/room_panel.rs`. The build declares
`Use::anchors()` and `Use::plane_tracking()` only for that variable, because the planes bring the
user's permission prompt, and the default demo has none. `XR_DEMO_ROOM=with` adds the panel to the demo
and `only` shows it alone. `script` shows it alone and places it with no user: a start that finds no
kept anchor makes one at a set place, as if the user had dropped the panel there, and asks the runtime
to keep it. A device that has no controller to carry the panel shows the persistence that way.

The panel is movable with `MovePolicy::anchor`. When the user lets it go, the demo makes an anchor at
`SpatialMoveEvent::room_pose`, asks the runtime to keep it and holds the panel to it with
`FollowTarget::Anchor`. The anchor that held the panel before is unpersisted and detached, so the app
keeps one. At the next start the demo loads the identifier that the runtime lists, and the panel
stands there. The panel lists the anchor's state and the planes: label, direction, size and height.
The demo logs the anchor's identifier and its pose in the room, in the stage and the head's pose
when the runtime first tracks it.

### Measured on the Android XR emulator

The emulator is the system image android-34 `google-xr` revision 7. Its runtime lists both families,
`XR_ANDROID_trackables` with `XR_ANDROID_device_anchor_persistence` and the `XR_EXT_spatial_*`
extensions. The backend uses the first. The demo built with `XR_DEMO_ROOM=script` ran twice after
`pm clear` and a grant of `SCENE_UNDERSTANDING_COARSE`, with the log of each start kept:

- First start: the runtime read its storage within the first frames and kept 0 anchors. The demo made an
  anchor at (0.5, -0.1, -1.2) in the room, and the runtime kept it 49 ms later under the identifier
  `04f2281f-f84b-49a3-b65d-3b420397f3a2`. The anchor was tracked at that pose, which is (0.5, 1.5, -1.2) in
  the stage.
- Second start, in a new process: the storage listed that identifier, the demo loaded it with
  `Anchors::load`, and the runtime tracked it at (0.5, -0.1, -1.2) in the room and (0.5, 1.5, -1.2) in
  the stage. The panel stood where the first start left it. No anchor was made.
- The runtime reported 12 planes in both starts: 8 walls, a floor of 8.6 by 7.8 m at a height of -1.59 m
  and 3 tables at heights of -0.85, -0.99 and -1.11 m. The panel lists them with their labels.

The emulator's head stays at the origin, so the room's origin is the same in both starts. The check
shows that the identifier survives a restart and that the runtime places the loaded anchor at the stored
pose. It does not show a room whose origin moved between two starts. A controller's carry of the panel,
and so the snap to a plane with a real carry, was not run on the emulator: the snap is covered by the
tests of `plane_snap.rs` and of `Space::carry`.

### What is not measured

No Meta Quest ran this code. The Quest was off adb while the work ran and then too low on charge. The
Meta backend (`meta_anchors.rs`, `meta_planes.rs`, `meta_calls.rs`) is written from the extensions'
specifications and checked by the compiler for Android, and its geometry and label handling have unit
tests. These are open until a Quest runs it: the Horizon OS permission names that make the runtime list the
extensions (taken from a Quest log of 16 November), the order of the events of a save and a query, the
axes of a scene entity (`plane_frame.rs` assumes +Z is the normal and corrects the sign by the label),
whether a query of the local storage returns the room's scene as well (the backend leaves out entities
with semantic labels), and what the storage holds after the user clears the app's data.

## Text entry

A text field that takes focus asks the platform for its software keyboard, through
`PlatformTextInputHandler` of `cranpose-ui` (`show_keyboard` on focus and on each tap on the
field, `hide_keyboard` when focus leaves). Android and iOS install a handler that opens the
system's keyboard. The headset backend installs one of two handlers on its shell.

### The keyboard that Cranpose draws

Its handler sets the state `open` of the keyboard. A composable that the backend adds after the
app's content (as `VolumeScene` is) composes a `SpatialPanel` of keys while `open` is true. The
panel's modifier marks it as standing below the panel of the focused field. The pose is no state of
the panel's composition: the space derives it each time it reads the panels. It puts the keyboard
24 dp below the field's panel, in the same plane, and takes the field panel's `Ride`. So a panel that
the user carries, or that follows a controller, takes its keyboard along with no composition. The
keyboard shows nowhere while no panel is known to hold the field.

The frame loop runs three steps for the keyboard after it sent the pointers' events to the shell.
It asks the shell for the focused field's bounds, and the space names the panel of content whose
part of the atlas holds the bounds' center. Then it hands the keys that were typed to
`AppShell::on_key_event`. A key's click runs inside the shell's own pointer dispatch, so a key
cannot call the shell. It pushes the key down and the key up of a hardware keyboard into a queue
instead. The text reaches the field through the path of a hardware keyboard: the key handlers of
the app, the focus keys and then the field.

A key is a `Button`. The integration tests press keys with a controller's ray and with a scripted
hand, and the field keeps its focus through every press. The keys follow the US
layout, so a symbol that a US keyboard reaches with shift carries shift and the key of its digit or
punctuation. Backspace, enter and the hide key carry no text, because the field matches a key event
with text before it matches the key.

The keyboard is one more layer in a frame. `LayerLimit` already drops the panels that stand
farthest from the head when a frame holds more layers than the runtime takes, and the keyboard
stands near the head.

### The system's keyboard on Android XR

Android XR shows the Android input method over an immersive OpenXR app. The evidence comes from the
Android XR emulator (system image android-34 `google-xr` revision 7, Gboard as the input method):

- An activity that has no editor view cannot show it. `InputMethodManager` logs "Ignoring
  showSoftInput() as view=DecorView ... is not served".
- With the editor view of `cranpose/android/java` (`CranposeTextInput`), which the Android backend
  of Cranpose uses on a phone, the input method shows: `dumpsys input_method` reports
  `mInputShown=true` and `mWindowVisible=true`, and the keyboard is a panel in the compositor's
  picture below the app's panels.
- The panel takes the input focus. The session goes from `FOCUSED` to `VISIBLE`, so the app's
  lifecycle is `Paused` and its pointer gets no aim. The back action hides the keyboard, and the
  session returns to `FOCUSED`. The next tap on the field shows the keyboard again.
- What the keyboard types arrives as input connection events. `adb shell input text hello` produced
  five `commitText` events, and the field of the demo showed the text.

So on a runtime with `XR_ANDROID_` extensions the backend installs `AndroidInputMethod` of `cranpose`
on the shell: the handler of the Android backend, with the editor view and the event queue,
without the window insets and without the placement of the editor view. The loop calls its `pump` in
each frame, which hands the queue's events to the shell and the focused field's text to the editor
view. The drawn keyboard never opens then. `use_keyboard(Keyboard::Drawn)` keeps the drawn keyboard.

Not run: a real Android XR headset, and tapping the keys of the system keyboard with a hand or a
controller (the emulator's hand simulation does not reach the guest, and `adb shell input text` types
through the same input connection without the keys). Whether the Quest's runtime shows the Android
input method over an immersive app is not run either. The Quest was not on adb for this change.

### Meta's virtual keyboard

`XR_META_virtual_keyboard` gives a Quest app the keyboard that the Meta system draws in its own
apps, with its suggestions and its animation. The runtime owns the layout and the typing logic, and
the app draws the keyboard. The app is not built for it yet. A Quest 3 runtime lists
`XR_META_virtual_keyboard` (the extension list that an app logged on 16 November). It lists
`XR_FB_render_model` only for an app whose manifest declares `com.oculus.feature.RENDER_MODEL`.

What an app needs, from Meta's integration guide for the extension:

1. The manifest declares `com.oculus.feature.VIRTUAL_KEYBOARD` (not required),
   `com.oculus.feature.RENDER_MODEL` (required) and the permission `com.oculus.permission.RENDER_MODEL`.
   The guide names Quest 2 and Quest Pro and Horizon OS v54 or later.
2. The instance enables `XR_META_virtual_keyboard` and `XR_FB_render_model`. The system properties
   (`XrSystemVirtualKeyboardPropertiesMETA`) say whether the runtime supports it.
3. The app creates the keyboard (`xrCreateVirtualKeyboardMETA`) and its space
   (`xrCreateVirtualKeyboardSpaceMETA`), and suggests a pose and a scale
   (`xrSuggestVirtualKeyboardLocationMETA`). The runtime has the final say, so the app reads the pose
   (`xrLocateSpace`) and the scale (`xrGetVirtualKeyboardScaleMETA`) back.
4. The app loads the keyboard's model through `XR_FB_render_model`: the path
   `/model_meta/keyboard/virtual`, with `XR_RENDER_MODEL_SUPPORTS_GLTF_2_0_SUBSET_2_BIT_FB`,
   because the model has several meshes and transparent textures. `xrLoadRenderModelFB` returns a
   binary glTF (`.glb`).
5. The app needs a glTF 2.0 loader for what the model holds: meshes, UVs, normals, tangents, vertex
   colors, materials, transparent textures, translation-rotation-scale and weight animations, and
   morph targets (16 for each key). The animation channels carry an `additiveWeightIndex` extra,
   and an index of -1 means every weight.
6. The model's glyph textures have a custom URI,
   `metaVirtualKeyboard://texture/{textureID}?w={width}&h={height}&fmt=RGBA32`. The loader needs a
   handler that makes a writable texture for each URI and keeps it by its id. Each frame the app
   asks for the dirty textures (`xrGetVirtualKeyboardDirtyTexturesMETA`) and writes their pixels
   (`xrGetVirtualKeyboardTextureDataMETA`).
7. Each frame the app applies the animation states (`xrGetVirtualKeyboardModelAnimationStatesMETA`:
   an animation index and a fraction), marks the morphed nodes dirty and rebuilds their meshes once,
   after the textures and all animations.
8. The app sends the input of each controller ray or hand (`xrSendVirtualKeyboardInputMETA`): the
   input source, its pose, whether it presses, and the interactor's root pose, which the runtime may
   change to limit the touch. The app draws its input model at the pose that comes back.
9. The app tells the runtime which text field has focus (`xrChangeVirtualKeyboardTextContextMETA`),
   shows the model (`xrSetVirtualKeyboardModelVisibilityMETA`) and waits for the shown event
   before it accepts input. The model's node named "collision" holds the geometry for the app's own
   collision.
10. The runtime sends the typing as events: `XrEventDataVirtualKeyboardCommitTextMETA`,
    `XrEventDataVirtualKeyboardBackspaceMETA` (repeated while the key is held),
    `XrEventDataVirtualKeyboardEnterMETA`, and the shown and hidden events. The app turns them
    into the key events of `AppShell::on_key_event`, as the drawn keyboard does.

Items 4 to 7 need a glTF loader and a renderer for skinned, morphed meshes with textures that
change. The app's glTF models (`SpatialVolume` with a loader) bring both, so this keyboard can
follow them. Its handler then replaces the drawn keyboard's on a runtime that lists the extension,
and the keyboard of Cranpose stays for the others.

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

On Android the surface draws through the renderer's Vulkan device. The pictures there are
`AHardwareBuffer`s of a private format, which Vulkan reads through a sampler with a YCbCr
conversion. A device that renders with OpenGL ES, or a Vulkan device that lacks an extension below,
draws nothing: `YcbcrConverter::new` answers `None`. The steps:

1. The device opens through `wgpu::hal::vulkan`'s `open_with_callback`, because wgpu's device
   request has no field for an extension that it does not know. The callback adds
   `VK_ANDROID_external_memory_android_hardware_buffer`, `VK_EXT_queue_family_foreign`,
   `VK_KHR_sampler_ycbcr_conversion`, `VK_KHR_external_memory`, `VK_KHR_dedicated_allocation`,
   `VK_KHR_bind_memory2` and `VK_KHR_get_memory_requirements2` when the driver lists all of them.
   The driver's list is the one that wgpu-hal read when it made the adapter, so no second query
   runs. The callback chains the `samplerYcbcrConversion` feature, which wgpu-hal leaves off. Any
   other adapter opens as before. Only an app built with the `video` feature of `cranpose` opens
   its device this way: without it, the device opens as plain wgpu opens it. The Android shell
   gives the composition its `GpuContext`, and gives it the new one after a device loss.
2. `vkGetAndroidHardwareBufferPropertiesANDROID` returns the buffer's memory requirements and its
   `VkAndroidHardwareBufferFormatPropertiesANDROID`. The format of a private buffer is
   `VK_FORMAT_UNDEFINED` with an `externalFormat`, and the properties suggest the model, the range
   and the chroma offsets.
3. A `VkSamplerYcbcrConversion` takes the external format and the suggested values. An immutable
   `VkSampler` takes the conversion, and the descriptor set layout holds it as an immutable
   sampler. The sampler's filters are linear when the format supports a linear chroma filter, and
   nearest otherwise. The pipeline belongs to this format, so the converter builds it when the
   first picture comes and builds it again when a picture has another format.
4. A `VkImage` is made with `VkExternalMemoryImageCreateInfo` and `VkExternalFormatANDROID`.
   Its memory is imported with `VkImportAndroidHardwareBufferInfoANDROID` and a dedicated
   allocation. The image view carries `VkSamplerYcbcrConversionInfo`. The size of the image is the
   size that `AHardwareBuffer_describe` reports, which is larger than the picture where the decoder
   pads it: a picture of 360 rows lies in a buffer of 368 rows.
5. The command buffer acquires the image from `VK_QUEUE_FAMILY_FOREIGN_EXT` with a barrier to the
   shader read layout. One render pass draws a triangle that covers the target. The fragment shader
   samples the picture at the picture's `CropRect`. The highest coordinate that it samples lies a
   pixel inside the crop, so the filter of the chroma samples does not read the padding.
6. The pass writes into a texture of an `ExternalFrames` ring, which `Image` draws. wgpu has no
   binding for an immutable sampler with a conversion, so the pass is raw Vulkan. It records into a
   wgpu command encoder through `CommandEncoder::as_hal_mut` and goes to the queue through
   `wgpu::Queue::submit`, so the renderer's thread and the converter never use the `VkQueue` at the
   same time. The ring makes its textures through the hal device and tells wgpu that each one is
   sampled and initialized. The render pass leaves the texture in the layout of that state, so wgpu
   inserts no barrier and no clear for it.
7. The frame stays alive until the GPU has read it. The image, its memory, its view, the
   framebuffer, the target texture, the descriptor set and the `VideoFrame` go into the callback
   of the queue's submission, because wgpu does not see the raw commands. wgpu runs the callback when it polls the queue's fences, which every submission does.
   The `AImageReader` of the decoder has `depth` plus the images in flight.

The shaders are GLSL, because a YCbCr sampler needs a combined image sampler, which WGSL cannot
write. `scripts/dev/compile_ycbcr_shaders.sh` compiles them with the NDK's `glslc` and checks the
result with `spirv-val`. The repository keeps the SPIR-V beside its source.

The model and the range come from the driver's suggestion for each buffer, which follows the data
space that the decoder sets. The range followed the decoder in every stream that the tablet played.
The model did not for a BT.709 stream of 640 by 360 pixels with the full range: the decoder
reported `color-standard` 75 and a BT.601 data space, and the driver suggested BT.601. That stream
shows chroma that differs by up to 9 levels of 255 from the encoder's colors. The BT.601 stream
with the limited range shows the encoder's colors within one level. A BT.709 stream of 1920 by 1080
pixels was decoded as BT.709: its colors match a BT.709 decode of the samples within one level.

The cost of one picture on that tablet, with two decoders running at 120 and at 60 pictures a
second in release builds: the CPU takes 0.85 to 1.0 ms for a picture of 640 by 360 pixels and 1.2
to 1.4 ms for a picture of 1920 by 1080 pixels. The import takes 0.2 and 0.4 ms of that, and
`Queue::submit` takes 0.4 and 0.6 ms. The pass takes 0.31 ms on the GPU at 1920 by 1080 pixels,
measured with timestamp queries around the commands.

A device without the extensions needs the GLES path of `cranpose-xr`, which binds the buffer as an
EGL image.

## Streamed scenes

A game that a computer renders for both eyes arrives as a stream of frames, and each frame
belongs to the head pose that it was rendered for. `StreamedScene(frames, views, stats)` takes the
`DecodedFrames` of a `VideoDecoder` in frames mode. `views` maps the `u64` that the app submitted
with an access unit to a `StreamedViews`: for each eye, the render pose in the app's room, the
`FieldOfView` and the rectangle of the eye's picture in the frame. A `RenderViewpoint` of
`XrFrame` carries the same `FieldOfView`, so an app passes the field of view that the headset
reports straight into a streamed view.

The backend takes each decoded frame into a queue of up to three frames when the frame comes out of
the decoder, and asks `views` for its views once. Each frame holds an image of the decoder, so the
decoder's `depth` is at least 5: the three queued frames, the frame shown last, and one for the
decoder to write. At each of the headset's frames the queue picks the frame to show. A frame's
`StreamedViews::target_display_time` is the time at which the host wants its pictures on the
display, on the clock of `XrFrame::predicted_display_time`. The queue chooses the frame whose target
lies closest to the predicted display time of the frame that the backend submits, among the queued
frames and the frame shown last. It drops the frames that came before the one it chose, so the
picture never goes back. The frame shown last stays while no queued frame is closer, and a full
queue takes it out of the choice, so a host whose targets run far ahead of the display cannot freeze
the picture. Frames with no target follow the newest-frame rule. A `StreamStats` handle that the
app gives to `StreamedScene` counts the frames shown, the frames skipped and the headset frames that
repeated a picture: a 30 Hz stream on a 120 Hz headset shows 30 and repeats 90 each second.

The backend imports the chosen frame's `AHardwareBuffer` as an EGL image, binds it to an external
texture, and draws each eye's rectangle into a swapchain of that eye's size. One pass of a small
shader per eye does it, and the external sampler turns the decoder's YUV into RGB. The frame's two
eyes become the views of an `XrCompositionLayerProjection`, with the poses and fields of view of
the frame. The compositor reprojects each picture from the pose it was rendered for to the head's
newer pose. While no other frame is chosen, the layer shows its last pictures again. The scene lies
over the room and under the curved panels, the flat panels and surfaces, the handles and the rays.

A host may send each eye with full density in a center and compressed edges, which lets its encoder
code a third of the pixels. `StreamedView::foveation` describes ALVR's axis-aligned foveated
encoding: the aligned center size and shift, the edge ratio, the share of the rectangle that the
eye fills, the full size of the eye and whether the picture is stored mirrored, which ALVR does for
the right eye. A foveated view takes a second shader, which the backend links when the first
foveated frame comes, so an app with no foveated stream pays nothing for it. For each pixel of the
eye's full view the shader computes where the compressed picture holds it: the center is a line,
and each edge is the root of a quadratic. The swapchain has the full size, so the projection layer
shows the picture as the host rendered it. `FoveatedUnwarp` computes the constants of the three
pieces on the CPU, and its `sample` function holds the same formulas as the shader, so the host test
that warps a point and unwarps it covers the shipped math.

The backend keeps the drawn frame until the next one is drawn, because the GPU may still read its
buffer. The swapchains have the `SRGB8_ALPHA8` format, and the shader turns the picture's encoded
values into linear light, so that the swapchain keeps the picture's own bytes. A plain `RGBA8`
swapchain has no such conversion: the compositor reads it as linear light, and a picture written
into it as it is shows lighter than the same picture in a surface (a gray of 128 shows as
191). The pass draws each picture's top at its image's last row, and the layer needs no
`VERTICAL_FLIP` layout, which a quad of wgpu's images and a surface's quad need.

On a Quest 3 the pass costs 0.4 ms of CPU and 0.03 ms of GPU for a frame of two 320 by 360 pixel
pictures, and 0.4 ms and 0.3 ms for two 1920 by 1080 pixel pictures. A frame comes 30 times a
second in the demo, and the headset runs at 120 Hz. With a second scene that unwarps a foveated
picture of two 640 by 720 pixel eyes, both scenes together cost 0.45 ms of CPU and 0.08 ms of GPU
for each frame on average, and the foveated shader compiled in 14 ms when its first frame came. The `stream-costs` feature of `cranpose-xr`
times the passes and logs the costs once a second. Without it the passes run with no timing.


### Outside a headset

Without a headset, `StreamedScene` composes `PlayerSurfaceIntake` of `cranpose`, which has the box,
the content scale, the converter and the GPU context of `PlayerSurface`. `PlayerSurface` takes the
newest picture of the decoder. `PlayerSurfaceIntake` calls an `intake` closure each time the decoder
has pictures. The closure takes the pictures with `DecodedFrames::next_frame` and gives the one to
show to `FramePresenter::present(frame, part)`, which converts it on the GPU and shows `part` of it
through a `BitmapRegionPainter`. No GPU pass is added: the region crops the picture where `Image`
draws it. `present` returns `false` when the platform cannot import the picture, when the GPU holds
too many pictures or when `part` lies outside the picture.

The scene's closure follows `Stream::intake` of the headset backend. It asks `views` for each frame
in the order of the stream, checks the views with the headset's checks and takes the rectangle of the
left eye. The newest accepted frame shows. Each other frame counts as skipped. `StreamStats` gets
`shown` for a frame that `present` took, and `repeated` stays 0: the shell draws a frame of the window
when something changes, and a callback that counted the frames of the window would keep the shell
drawing at the display's rate. A foveated picture shows as the host encoded it, because the unwarp is
a pass of the headset backend, and the log says so once.

## Crates

- `cranpose-xr`: the API above and the spatial tree. It lays out the panels, casts the
  controllers' rays at them and sends pointer events into each panel's composition. `movable`
  and `resizable` handle grabs.
- Its OpenXR backend: the session, the frame loop, the swapchains, the input actions and the
  extensions. The OpenXR calls sit in a reviewed boundary module.
- A flat backend for every other platform: the `Subspace` draws its panels side by side in an
  ordinary window, and a `SpatialVolume` draws as a 3D view in its box. The web adds WebXR through
  the same API.

Cranpose owns the GPU context. On the Quest it uses GLES through wgpu. On Linux and Windows the
OpenXR runtime (SteamVR, Monado, Meta Link, Pimax, Varjo) creates the Vulkan instance and device
through `XR_KHR_vulkan_enable2`, and wgpu wraps them. The backend keeps the difference in its
`graphics` module: one file for GLES and one for Vulkan make the session and the wgpu device and
wrap the swapchain images as wgpu textures. The panels, the rays, the handles, the passthrough
layer, the cylinder layers and the projection layer of the volumes do not change, except that a
Vulkan headset carries no depth for the volumes (see "3D content"). Four things stay
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
- On Android XR the host takes the activity's events until the activity is resumed and has a
  window, before it creates the OpenXR instance. The host does so when the runtime offers an
  `XR_ANDROID_` extension. Android XR's `xrCreateInstance` waits for that state. It polls the
  calling thread's looper and reads the commands of `android_native_app_glue` from it. This app's
  glue is android-activity, which keeps its commands in a pipe on the same looper, and the
  runtime never reads them. Without the wait the call spins at full CPU use and never returns:
  the log ends after `xrCreateInstance called`. With the wait the runtime logs "already in resume
  state with window available, skip" and goes on. Other runtimes start as they always did.

The backend also keeps these rules of every runtime and store:

- A session that is VISIBLE but not FOCUSED, such as under the system menu, gets no controller
  input. The pointer gets no aim from any source, a controller or a hand. It sees both controllers
  lost, which ends a press and drops a carried panel. No ray shows. The panels and the volumes
  still draw, and the app's lifecycle is `Paused`.
- A session that is not VISIBLE gets frames with no layers and no drawing, and the app's
  lifecycle is `Stopped`.
- An app closes itself with `cranpose_services::request_exit()`, as in a flat app. On each turn the
  loop reads `exit_requested()`. While the session runs, it calls `xrRequestExitSession` once. The
  runtime moves the session to STOPPING, the backend ends it, and the loop leaves when the session
  is no longer running. A request that comes while no session runs makes the loop leave at once.
  After the session is gone the Android host finishes the activity with `cranpose::finish_activity`,
  and the desktop host returns from `run`. The loop takes the request with `take_exit_request()`
  only after that landed, so a failed close leaves the app's request recorded.
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

## Store readiness

The Meta Horizon Store checks a store app for its pause and focus behavior and for its entitlement.

### Focus and pause

Meta's Virtual Reality Checks (VRCs) for Quest apps give an immersive app three rules about focus
and pause.

- VRC.Quest.Input.4: An app that has lost input focus keeps rendering. It hides the hands and the
  controllers of the user or shows them at rest. It ignores their input. The app loses input focus
  when the user opens the Universal Menu or when a windowed app takes the focus. Audio input is
  not restricted. When the focus returns, the app shows the hands and the controllers again.
  It takes input again.
- VRC.Quest.Functional.2: A single-player app pauses its interactive elements when Horizon OS asks
  it to. Non-interactive elements such as ambient sound may go on. A multiplayer app is exempt.
  The system asks when the user opens an immersive system view such as passthrough or the
  boundary setup. It asks when the user turns the display off with the power button. It asks when
  Auto Sleep ends after the user took the headset off. A Quest 3S has no proximity sensor, so it
  has no such trigger. The rule lets an app pause on the loss of input focus instead.
- Taking the headset off: Meta calls the app's reaction optional, such as a pause menu or marking
  the user inactive. The system pauses the app when Auto Sleep ends.

The rules conflict for the state in which the app has no input focus and the system has not
paused it. Input.4 wants frames. Functional.2 wants the interactive elements to stop and lets the
app choose how. The backend follows Input.4 for the picture. It gives the app the state for the
rest. An app without input focus gets no input, shows no ray and keeps its layers on display. It
keeps composing and drawing. The app stops what it considers interactive when its lifecycle
leaves `Resumed`.

| OpenXR session state | Backend status | Lifecycle (`local_lifecycle_state()`) | Layers | Composition and drawing | Input |
|---|---|---|---|---|---|
| FOCUSED | `Focused` | `Resumed` | the app's | yes | controllers and hands |
| VISIBLE | `Visible` | `Paused` | the app's | yes | none, no ray |
| READY, SYNCHRONIZED | `Hidden` | `Created` until the runtime first shows the app, then `Stopped` | none | none | none |
| STOPPING, IDLE | `Idle` | `Stopped` | no frames | none | none |
| EXITING, LOSS_PENDING, activity destroyed | `Exit` | `Destroyed` | no frames | none | none |

The lifecycle maps as Compose maps a window. It is `Resumed` while the window has focus, `Paused`
while it shows without focus and `Stopped` while it does not show. The backend publishes the
lifecycle on each change of the status, before the frame that the change affects. A new run in
the same process starts at `Created` again. The listeners of the lifecycle that the crates of an
XR app hold act on it as follows:

- `collectAsStateWithLifecycle` and `repeat_on_lifecycle` collect from `Started` on. They go on
  under `Paused` and stop under `Stopped`.
- A prompt of `device_auth` shows when the app is `Resumed`. A prompt that waits during a pause
  shows when the focus returns.
- The system languages refresh on `Resumed`.
- The media service stops its playback on `Destroyed`.
- `Paused` runs the durable saves that the app registered, as an Android activity's pause does.
  The frame loop waits for them for up to `durable_save_deadline()`, which is 2 s unless the host
  says otherwise. An app with no durable save waits for nothing.

Jetpack XR Compose has no composition local for the session's state. An XR app reads the
lifecycle of its activity, so `local_lifecycle_state()`, `LifecycleEffect` and `observe_lifecycle`
of Cranpose serve it. `XrFrame::focused` gives the same answer in a frame effect.

Each change of the status logs one line with the frames that the app ran in the status that it
left:

```
cranpose xr: Focused ran 943 frames, now Visible, lifecycle Paused
```

### Focus and pause on a Quest 3

The Quest 3 (runtime "Oculus" 210.137.0) gave the sequences below. `scripts/dev/xr_focus_check.sh`
runs the first and the third through adb. It saves the compositor's picture of each step.

| Trigger | Activity | Session states in order | Lifecycle |
|---|---|---|---|
| The Files panel opens over the app (`systemux://file-manager`) | none | VISIBLE | `Paused` |
| Another immersive app starts. The app starts again 10 s later | `onPause`, `onStop`, then `onStart`, `onResume` | VISIBLE, SYNCHRONIZED, STOPPING, IDLE (no frame for 10.6 s), READY, SYNCHRONIZED, VISIBLE, FOCUSED | `Paused`, `Stopped`, then `Resumed` |
| The app starts again while the Files panel is open | `onPause`, `onResume` within 10 ms | FOCUSED only, or SYNCHRONIZED, STOPPING, IDLE, READY, SYNCHRONIZED, VISIBLE, FOCUSED in one batch of events (both happened) | `Resumed`, or `Stopped` then `Resumed` |
| The power button (`adb shell input keyevent POWER`) | `onPause`, `onStop` | VISIBLE, SYNCHRONIZED, STOPPING, IDLE within 100 ms of `onPause` | `Paused`, `Stopped`: the loop ran no frame while the display was off |
| Home | `onPause`, `onStop`, destroy | VISIBLE, SYNCHRONIZED, STOPPING, IDLE | `Stopped`, then `Destroyed`: the loop ends. The next start in the same process begins at `Created` and reaches `Resumed` |

Under the Files panel the app ran about 115 frames each second. The picture of both eyes kept the
panels, the video surface and the 3D torus. No ray showed. The readout of the demo said
`Lifecycle Paused`. After the app came to the front it said `Lifecycle Resumed`. The ray was back.
The demo's decoder showed 30 frames each second in FOCUSED, under the Files panel and after the
return. In IDLE the frame effect that logs the count did not run. The log has no line for the
10.6 s. After the 10.6 s in IDLE the same process resumed with its composition: the log has no
second `ready` line.

Not run on a device:

- The Universal Menu that the controller's system button opens. Its passthrough entry. A person
  must press them.
- The simulated removal of the headset and the Auto Sleep that follows.

A runtime that sends `VISIBLE` for these cases gets the treatment of the Files panel. A runtime that
sends `STOPPING` gets the treatment of the second row.

### The entitlement check

The Meta Horizon Store needs a store app to check at its start that the viewer owns the app, and
to quit when the check fails. Meta's Platform SDK does the check. It is a Java library with two
native libraries (`libovrplatformloader.so`, `libpsdk_jni.so`). All of it comes in one Maven
Central artifact (`com.meta.horizon.platform.ovr:android-platform-sdk`). A pure Rust app
reaches it through a small Java class, as the billing service does.

**The license.** The SDK is under the Meta Platform Technologies SDK License Agreement
(<https://developers.meta.com/vr/licenses/oculussdk/>). The agreement says that downloading or
using the SDK is acceptance. It binds the person or the entity that does so. It bars
sublicensing and any use that puts the SDK under an open-source license. So the repository holds
none of the SDK. It names the Maven coordinate behind three switches that are off by default, and
whoever turns them on accepts the agreement. The docs of the feature and of the Gradle service say
so.

**The parts.**

- The cargo feature `meta-entitlement` of `cranpose-xr` (and of `cranpose`) compiles the check in.
  Without it no code calls the Java class. `local_entitlement_check()` is `None` then.
- The Gradle service `meta-entitlement` adds `crates/cranpose/android/java-meta-entitlement`
  (`CranposeMetaEntitlement`) and the dependency on the SDK. An app that does not list it fetches
  and packs nothing of the SDK: no `com.meta.horizon.platform` class and no `libovrplatformloader.so`.
- The app id comes from the build: `cranposeMetaAppId` (or `cranpose { metaAppId }`) fills the
  `com.meta.horizon.platform.ovr.OCULUS_APP_ID` entry of the merged manifest. The build fails when
  the service is listed and the id is missing or not all digits. The Java class reads the entry as
  an object, because the manifest compiler stores an all-digit value as a number.

**The flow.** `Host::begin_entitlement` calls `CranposeMetaEntitlement.cranposeMetaEntitlementBegin`
before the backend connects to the runtime, because Meta asks for the check within 10 seconds of
the launch and the connection can wait for a permission prompt. The class calls
`Core.asyncInitialize(appId, activity)` and, when that succeeds, `Entitlements.getIsViewerEntitled()`.
The SDK answers on the Java main thread. The answer sits in a volatile field. The app's handle
(`EntitlementCheck`) lives in the composition, as the state of any composable does. The loop
reads it. The loop asks for the answer every 100 ms while the check runs (`EntitlementWatch`) and
stops asking at the outcome, so a frame pays for no JNI call. An exception, an initialization that fails, an error answer, a missing
Gradle service and 30 seconds with no answer all end the check as `NotEntitled`, because Meta's
guide counts each as a failed check and an app must not run unchecked when the platform is silent.

**A quit app keeps its process.** Android caches the process after the activity finishes. The
SDK's message pump goes on logging at verbose level. The Java class keeps a running check and an
entitled answer for the process. A failed check starts again with the next run of the loop in the
same process, so a failure that came from a busy platform does not lock an owner out until Android
ends the process. Each run on the Quest below started after a force-stop, so this restart is not run
on a device.

**What the app does next.** The default is `OnNotEntitled::Quit`: the backend logs the result and
calls `request_exit`. The loop that already closes an app ends the session and finishes the
activity. An app picks `OnNotEntitled::Continue` through `EntitlementCheck::set_on_not_entitled`
in its first composition and reads `EntitlementCheck::entitlement` to limit itself. Jetpack XR has
no entitlement API, so the names are this crate's.

**Measured on a Quest 3** (runtime 210.137.0), with the demo (`just android-xr-entitlement <id>`)
installed through adb and a placeholder app id (`1234567890123456`). The placeholder names no
store app. An adb install is not a store install, so the check must fail. It does. The
Platform SDK says why: the user does not have the app in the library.

| Build | Log of the app | What the app did |
|---|---|---|
| Default policy | `Meta entitlement: the check starts for app 1234567890123456`, 0.64 s later `Meta entitlement: the viewer is not entitled: code 8, http -1, user does not have app in library`, then `cranpose xr: entitlement check: NotEntitled (...); the app quits` and `cranpose xr: ends in Exit, lifecycle Destroyed` | The loop logged the result 0.3 s after the SDK answered, because it asks every 100 ms and waits up to 100 ms per turn before the session shows. The session ended 54 ms later and the activity finished. The Quest showed the app that was open before. The process stayed as a cached process |
| `XR_DEMO_ENTITLEMENT=continue` (the app picks `OnNotEntitled::Continue`) | The same lines, ending in `NotEntitled (...); the app goes on` | The app ran on. Its readout showed `Entitlement NotEntitled` and `Lifecycle Resumed` |
| The feature on and the Gradle service missing | `ClassNotFoundException` for `dev.cranpose.android.CranposeMetaEntitlement`, `the Meta entitlement check cannot start; list the meta-entitlement service in the Gradle build`, then `entitlement check: NotEntitled (the build carries no entitlement service); the app quits` | The app quit at once: a build never runs unchecked |
| The feature off | No line about the check | The APK holds no `com.meta.horizon.platform` class, no `CranposeMetaEntitlement` and neither `libovrplatformloader.so` nor `libpsdk_jni.so`. The build with the service has all of them |

`scripts/dev/xr_entitlement_check.sh` runs a build through adb and prints these lines.

Not run on a device: a store install that the check finds entitled. It needs an app in the Meta
Horizon store, a user who owns it and a real app id. The success path has tests of the decision
logic and of the numbers that Java gives. It has no run across JNI.

## Spatial audio

A sound source stands at a pose in a `Subspace`, or at a panel or a volume. In each frame the sound
reaches each ear with the level and the delay that the place of the source relative to the head gives.
The model is Cranpose's own panner in the audio mixer. Every backend of Cranpose's audio engine runs
the mixer, so the model needs no help from the runtime.

### The choice

Three ways to place a sound in a headset exist. This crate takes the first.

| Way | What it gives | Why this crate does not take it |
|---|---|---|
| Cranpose's own panner in `cranpose-audio`'s mixer | A distance gain, a level and a delay for each ear and a head shadow. They come from the pose of the source and the pose of the head | |
| `android.media.Spatializer` (API 32) | Queries about the device: `isAvailable`, `isEnabled`, `getImmersiveAudioLevel`, `canBeSpatialized` and `isHeadTrackerAvailable` | It renders the channels of a stream as virtual speakers around the listener. It has no call that gives a source a position or a distance. A sound at a pose cannot use it |
| Jetpack XR's `SpatialSoundPool.play(session, soundPool, soundID, params, entity, ...)` | A point source of the system that follows an `Entity` | It takes a SceneCore `Session` and an `Entity`. Cranpose draws through OpenXR and holds neither |

Meta's XR Audio SDK is a licensed download, so this crate does not use it. A backend for Android XR could
hand the sources to SceneCore. This crate has no such backend.

### The API

The names follow Jetpack XR where it has one:

| Cranpose | Jetpack XR | Difference |
|---|---|---|
| `PointSourceParams` | `PointSourceParams` | It has `distance_attenuation` only. Jetpack XR adds `directivity` and `spread`. The panner has a point that sounds the same in every direction |
| `DistanceAttenuation` (`Auto`, `None`, `Linear { .. }`, `Natural { .. }`) | `DistanceAttenuation` (`Auto`, `None`, `Linear`, `Natural`, `Custom`) | It has no `Custom` curve. A `min_distance` below 0.1 m counts as 0.1 m. Jetpack XR throws for it |
| `PointSource { sound, params, playback, looping }` | The arguments of `SpatialSoundPool.play`: `soundID`, `params`, `volume`, `priority`, `loop`, `rate` | `playback` is Cranpose's `PlaybackParams` with the volume, the rate and the bus. It has no priority. `looping` is the `loop` of -1 in Jetpack XR. A count of loops does not exist |
| `SpatialPointSource(modifier, source)` | `SpatialSoundPool.play(.., entity)` at an `Entity` | It is a composable. The sound plays while the composable is in the composition. Jetpack XR has no composable for it |
| `SubspaceModifier::point_source(source)` | The `entity` argument of `play` | The sound comes from the own pose of the panel or the volume. A carry and a following subspace move that pose |
| `SpatialCapabilities::is_spatial_audio_enabled()` | `SpatialCapabilities.isSpatialAudioEnabled` | It is on in a headset when the audio player of the app has a spatial scene. Cranpose's engine has one |
| `SpatialSoundPool.getSpatialSourceType`, `SoundFieldAttributes`, `SpatialMediaPlayer`, `SpatialAudioTrack` | | They do not exist here. Cranpose has no ambisonic sound field and no player other than its mixer |

```rust,ignore
use cranpose::{PlaybackParams, default_audio};
use cranpose_xr::{PointSource, SpatialPointSource, SpatialVolume, Subspace, SubspaceModifier};

#[composable]
fn Scene() {
    // The app installed Cranpose's audio engine: `cranpose::install_audio()`, or the `audio`
    // feature on Android
    let hum = remember(|| default_audio().load(include_bytes!("hum.wav")).ok()).with(|id| *id);
    Subspace(move || {
        // A sound at a pose, 80 cm to the right of where the subspace starts
        if let Some(sound) = hum {
            SpatialPointSource(
                SubspaceModifier::new().offset(800.0, 0.0, 0.0),
                PointSource::new(sound).looping(),
            );
        }
        // A sound at a volume. It moves when a controller's ray carries the volume
        SpatialVolume(
            SubspaceModifier::new()
                .movable()
                .point_source(PointSource::new(hum.unwrap_or_default()).looping()
                    .playback(PlaybackParams::new().volume(0.6))),
            |_| { /* the 3D content */ },
        );
    });
}
```

A source plays while it is in the composition. A source that does not loop plays once, when it enters
the composition. A source starts a new voice when its `PointSource` value changes. Outside a headset
the composables play nothing. `is_spatial_audio_enabled()` is off there.

### The model

The mixer takes the place of the source in the axes of the head: x to the right, y up and z behind.
The model keeps a first-order filter for each ear and nothing else. Each part is a plain formula:

- **Distance.** The `DistanceAttenuation` of the source gives one gain for both ears. `Auto` is
  `Natural` from 1 m. The gain is 1 up to a meter. Beyond a meter it is `1 / distance`, which is 6 dB
  for each doubling.
- **Level.** `sideways` is the x of the unit vector to the source. It is the sine of the angle between
  the source and the forward axis of the head. The ear that is farther from the source is quieter by
  `6 dB * |sideways|`. The other ear keeps the gain. The loudspeakers of a headset sit at the ears and
  need this cue.
- **Delay.** Woodworth's formula for a spherical head of 8.75 cm radius gives the delay. The far ear
  hears the sound `radius / speed * (asin(sideways) + sideways)` later. That is 656 microseconds for a
  source at the side. Each ear reads the clip at its own position, so the delay costs a second read and
  no delay line.
- **Head shadow.** The filter of Brown and Duda (1998) is `(1 + alpha s T) / (1 + s T)` with
  `T = radius / (2 * speed)`. The bilinear transform makes it digital. The `alpha` of an ear is 2 for a
  source on the axis of that ear. It falls to 0.1 for a source 150 degrees from that axis. The high
  frequencies of an ear rise by up to 6 dB and fall by up to 20 dB with it. The shelf starts near
  1.2 kHz. The gain at zero frequency is 1.

The model has no elevation cue. A source above the head sounds like one below it. A source ahead sounds
like one behind. The model has no Doppler shift, no reverb and no occlusion. A stereo clip plays as the
mean of its channels, because a point source has one signal. A voice that plays from an emitter ignores
`PlaybackParams::pan`.

### The audio thread

The callback allocates nothing and takes no lock. The frame loop hands three kinds of state to the audio
thread through `cranpose_services::SpatialScene`. The audio engine makes the scene. The app reaches it
through `AudioPlayer::spatial_scene()`.

- **The pose of the head.** The loop writes it into a cell of atomic words in each frame. The callback
  reads the cell once for each render call. A write that overlaps the read makes the read fail. The
  callback then keeps the pose that it read before. A write takes no lock and never waits for a reader.
- **The emitters.** The `Emitter` of a source is a slot in a fixed array of 32 in the scene. Its place
  and its distance curve sit in a cell as well. Making an emitter claims a slot with a compare and swap.
  Dropping the emitter frees the slot. A voice names its emitter with the slot and a generation. A voice
  stops at the next render call when its emitter ended or when another emitter took its slot.
- **The pause.** One atomic flag.

A pose does not go through the command queue of the engine. That queue serves rare events. The engine
takes locks and asks the output device whether it runs when it sends a command. A command in each frame
would also fill the queue while the stream is paused. The voice itself starts with one command, when the
source enters the composition.

Across one render call the gain, the delay and the filter coefficients of each ear move in a straight
line from the old values to the values of the new pose. A head that turns between two frames does not
click. A voice that plays from no emitter runs the code that it ran before this feature. Its output has
the same bytes.

### The frame loop

`SoundStage` in `src/sound_stage.rs` runs in each frame, after the moves of the controllers and the
following subspaces:

1. It sets the listener to the pose of the head. The listener stays where it was while the runtime does
   not track the head.
2. It reads the sources from the `Space` into reused storage. The pose of a source is the live pose of
   its panel or volume. A carry moves the sound without a composition. A source in a following subspace
   follows the head or the controller.
3. It makes an emitter and a voice for each new source. It ends the voices of a source that left or
   changed. It writes the place of each emitter.

An app that installed no audio engine has the no-op player, which has no spatial scene. The stage is then
absent, the capability is off and the composables play nothing.

### Pause

The stage tells the scene that the app is paused in every lifecycle state except `Resumed`. Those states
are `Created`, `Started`, `Paused`, `Stopped` and `Destroyed`. The runner sets the flag when it
dispatches a change of the lifecycle, as the app sees it in "Focus and pause". A runtime that shows none
of the app silences the sources without a composition. A paused voice fades out in one render call and
then stands still. The sound goes on from where it stopped. A paused voice keeps the output stream open.
Sound that does not play from an emitter is the app's to pause with `local_lifecycle_state()`.

### The tap

The `audio-tap` feature of `cranpose-xr` turns on a tap on the mixed output. The mixer sums the squares
of the left and the right samples that it writes. For every second of output it publishes the root mean
square of each side into cells. The frame loop reads the cells. The callback does not log. The loop logs
one line for each second:

```
cranpose xr: audio tap, second 7: left 0.3711 right 0.1862 (rms), paused false, the source is at x -0.50 y +0.00 z -0.00 m of the head
```

The line holds the levels of the second that just ended. It holds the place of the source at the time of
the line. Without the feature the tap is off. The mixer then reads one flag in each render call for the
tap. `scripts/dev/xr_audio_check.sh` runs the check on a Quest.

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
| Samsung Galaxy XR | Android XR (`libopenxr.google.so`) | `PROPERTY_XR_ACTIVITY_START_MODE` set to the unmanaged full space, `uses-native-library`, `android.software.xr.api.openxr` | The Android XR emulator only (system image android-34 `google-xr` v7, runtime "Android XR" 1.0.0, system "Moohan"): the session runs through READY, SYNCHRONIZED, VISIBLE and FOCUSED, and the demo's flat, curved and surface panels and its 3D volume show. No pointer input was tested, and no real headset |
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
| 4 | 3D content, glTF models, `movable`, `resizable`, `Orbiter`, passthrough | a 3D scene, a glTF model, handles and the room around the panels |
| 5 | cranvr's Quest client on `cranpose-xr` | its desktop, console and overlay use the API |
| 6 | Store readiness: focus and pause (the loss of input focus and the power button are shown on a Quest 3; the passthrough entry and the headset's removal are not run), the entitlement check behind a feature (the failing check and its quit are shown on a Quest 3; a passing check needs a store app), other runtimes | an app passes the Meta store's checks |

The cranvr client still uses a streaming library for games. Its own protocol replaces it after
step 5, outside this repository.
