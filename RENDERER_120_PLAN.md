# Five renderer items for 120 fps

Base commit: `f766fc3e` on `perf/huawei-scroll`.

The measured state on the Huawei P40 Pro, library screen with 66 documents,
p50 frame 30.0 ms (33 fps). Stage medians: present 11, render 8, update 3.5, sync 1.3.
The one ablation above the noise: every frosted backdrop off gives 21.6 ms.

Each item below is a separate branch, a separate worktree and a separate agent.
No item may hardcode a special case for one widget, one screen or one effect.
An item that only pays on the library screen is not done.

## 1. The backdrop reads the composited target at composite time

Today a backdrop is a property of a layer that changes how the whole tree is captured.
`layer_has_backdrop` removes motion stable capture. `has_backdrop_underlay &&
contains_descendant_backdrop` refuses the raster cache. One self backdrop child sends
the whole frame off the direct root road. So five list rows re-render into 1200x420
offscreens every frame because each row holds one frosted button.

Make the backdrop an operation that runs at composite time and samples the target the
frame has already drawn into. The layer above it keeps its own capture, its raster cache
and the direct root road. The scene window a backdrop reads then has no reason to exist.

Files: `crates/cranpose-render/wgpu/src/surface_executor/render_paths.rs`,
`crates/cranpose-render/wgpu/src/render.rs`,
`crates/cranpose-render/common/src/graph.rs`,
`crates/cranpose-render/wgpu/src/normalized_scene.rs`.

Watch out: a composite time read must see the composites already flushed into the frame
target this frame. My earlier attempt built the input from the scene below the child and
the bottom bar lost the rows above it.

## 2. The passes after the scene build take the dirty node set

`replace_dirty_layers_from_applier` already patches only the nodes that changed. Four
passes then walk the whole tree: `recompute_raster_cache_hashes` 1.5 ms, the semantics
tree 1.1 ms, surface planning 0.95 ms, layer content collection 1.1 ms. That is 4.6 ms
of a frame where five rows moved.

Give those four passes the dirty node set, keep each node's result from last frame, and
recompute a node only when it or a parent is marked.

Files: `crates/cranpose-render/common/src/scene_builder.rs`,
`crates/cranpose-app-shell/src/shell_frame.rs`,
`crates/cranpose-render/wgpu/src/frontend.rs`, the semantics build in `crates/cranpose`.

## 3. Damage rects in the frame packet, present only the changed area

There is no damage anywhere in the renderer. Every frame repaints and presents the whole
surface. Carry a rect list in `FramePacket`: the union of each dirty node's bounds this
frame and its bounds last frame. Scissor the render pass to that list. Present only that
area where the platform allows it.

Files: `crates/cranpose-render/wgpu/src/frame_packet.rs`,
`crates/cranpose-render/wgpu/src/render.rs`,
`crates/cranpose/src/android.rs`.

Watch out: the swapchain hands back an older buffer, so a partial repaint must know what
that buffer holds. Either keep the damage of the last N frames and repaint their union,
or keep a full copy and blit.

## 4. A coverage record per draw, and drops of hidden draws

There is no occlusion of any kind: no depth attachment, no occlusion query, no order by
depth, no test for a draw that a later opaque draw covers. Every row card, shadow, icon
and text run under the bottom bar is drawn and then painted over.

Record for each draw whether it fills its own rect with an opaque color. Walk the draw
list from front to back and drop a draw whose rect a nearer opaque draw already covers.
The existing `shape_opaque_covers_rect` and `rounded_fill_covers_rect` are the start of
the coverage test.

Files: `crates/cranpose-render/wgpu/src/render.rs`,
`crates/cranpose-render/wgpu/src/surface_executor/render_paths.rs`.

## 5. The packet build and the submit on two threads

`GpuRenderer::render` builds the frame packet and consumes it on one thread. Producer
p95 11.85 ms and present p95 12.58 ms are measured in `RENDERER_PERF_PLAN.md`, so a
frame that costs the slower of the two lands near 12.6 ms instead of 24. `stage_executor`
is already written for two submitters and `Stage::Present` has no caller yet.

Files: `crates/cranpose-render/wgpu/src/stage_executor.rs`,
`crates/cranpose-render/wgpu/src/frontend.rs`,
`crates/cranpose-render/wgpu/src/lib.rs`, `crates/cranpose/src/android.rs`.

## Rules for every item

* The picture stays the same. A screen shot of the library, a document and the settings
  screen must match the base build byte for byte.
* Only cranpose changes. The cranscan app source stays as it is.
* Builds run on samarch-1, one build tree per agent.
* Measurement runs on the Huawei, one agent at a time, through the phone lock.
* A/B is paired and interleaved: base apk, then the branch apk, then base again, at
  least three rounds each. The phone warms up over ten minutes and run to run noise is
  about 4 ms, so a single pair proves nothing.
* `cargo test -p <crate>` stays green for every crate the branch touches.
