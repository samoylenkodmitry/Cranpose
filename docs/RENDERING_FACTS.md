# Cranpose Rendering Facts

> This document describes the current rendering pipeline as it exists in the repo on March 12, 2026.
>
> It is intentionally factual. It is for reasoning about the current implementation, not the target architecture.
> For the architecture critique and the required renderer rewrite direction, see `docs/render_arch.md`.

## 1. What the real frame loop does

The app shell drives three phases in order:

1. layout
2. dispatch queues
3. render

Small code example from `crates/cranpose-app-shell/src/lib.rs`:

```rust
fn process_frame(&mut self) {
    fps_monitor::record_frame();
    self.run_layout_phase();
    self.run_dispatch_queues();
    self.run_render_phase();
}
```

That is the top-level truth. If a model says the renderer is driven directly from composition without layout retention or app-shell phasing, that is wrong.

## 2. The shell rebuilds scenes from the applier, not from a rebuilt layout tree

The active render path in the app shell is `rebuild_scene_from_applier(...)`.

Small code example from `crates/cranpose-app-shell/src/lib.rs`:

```rust
if let Some(root) = self.composition.root() {
    let mut applier = self.composition.applier_mut();
    if let Err(err) =
        self.renderer
            .rebuild_scene_from_applier(&mut applier, root, viewport_size)
    {
        self.renderer.scene_mut().clear();
    }
} else {
    self.renderer.scene_mut().clear();
}
```

Important consequence:

- The renderer reads retained node state directly from live `LayoutNode` / `SubcomposeLayoutNode` instances.
- `LayoutTree` still exists and is still produced by layout code, but the app shell does not use it for the real WGPU scene build path.

## 3. Layout nodes retain their own measured state

Each `LayoutNode` stores retained layout data instead of relying on a per-frame reconstructed tree.

Small code example from `crates/cranpose-ui/src/widgets/nodes/layout_node.rs`:

```rust
pub struct LayoutState {
    pub size: Size,
    pub position: Point,
    pub is_placed: bool,
    pub measurement_constraints: Constraints,
    pub content_offset: Point,
}
```

And the node stores that state:

```rust
pub struct LayoutNode {
    // ...
    layout_state: Rc<RefCell<LayoutState>>,
}
```

This matters because scene building later reads:

- `position`
- `size`
- `content_offset`
- `is_placed`

If a model talks as if rendering only depends on immutable draw lists emitted during composition, that does not match Cranpose.

## 4. Modifiers are flattened into slices before the renderer looks at a node

The renderer does not traverse arbitrary modifier nodes directly. It reads a cached `ModifierNodeSlices` snapshot from each node.

Small code example from `crates/cranpose-ui/src/modifier/slices.rs`:

```rust
pub struct ModifierNodeSlices {
    draw_commands: Vec<DrawCommand>,
    pointer_inputs: Vec<Rc<dyn Fn(PointerEvent)>>,
    click_handlers: Vec<Rc<dyn Fn(Point)>>,
    clip_to_bounds: bool,
    text_content: Option<Rc<crate::text::AnnotatedString>>,
    text_style: Option<TextStyle>,
    text_layout_options: Option<TextLayoutOptions>,
    prepared_text_layout: Option<TextPreparedLayoutHandle>,
    graphics_layer: Option<GraphicsLayer>,
    // ...
}
```

These slices are the bridge between UI nodes and the render pipeline:

- draw commands
- text payload
- text style/layout options
- graphics layer state
- click/pointer handlers
- clip flags

## 5. `BasicText` is not painted by a normal draw callback

`BasicText` is implemented as `Layout(..., EmptyMeasurePolicy)` plus a `TextModifierElement`.

Small code example from `crates/cranpose-ui/src/widgets/text.rs`:

```rust
let text_element = modifier_element(TextModifierElement::new(current, style, options));
let final_modifier = Modifier::from_parts(vec![text_element]);
let combined_modifier = modifier.then(final_modifier);

Layout(
    combined_modifier,
    EmptyMeasurePolicy,
    || {},
)
```

The `TextModifierNode` measures text and exposes semantics, but its `draw()` method is intentionally a placeholder.

Small code example from `crates/cranpose-ui/src/text_modifier_node.rs`:

```rust
impl DrawModifierNode for TextModifierNode {
    fn draw(&self, _draw_scope: &mut dyn DrawScope) {
        // actual text rendering is handled by the renderer
    }
}
```

This is an easy place for external reasoning to go wrong. In Cranpose today:

- text measurement starts in UI code
- actual text rasterization is renderer-owned
- the renderer reads text from modifier slices

## 6. Text measurement is globally pluggable, and WGPU installs its own measurer

`cranpose-ui` exposes a global `TextMeasurer`.

Small code example from `crates/cranpose-ui/src/text/measure.rs`:

```rust
thread_local! {
    static TEXT_MEASURER: RefCell<Box<dyn TextMeasurer>> =
        RefCell::new(Box::new(MonospacedTextMeasurer));
}

pub fn set_text_measurer<M: TextMeasurer>(measurer: M) {
    TEXT_MEASURER.with(|m| {
        *m.borrow_mut() = Box::new(measurer);
    });
}
```

The WGPU renderer installs `WgpuTextMeasurer` during construction.

Small code example from `crates/cranpose-render/wgpu/src/lib.rs`:

```rust
let render_text_state = TextSystemState::from_fonts(fonts);
let measure_text_state = Arc::new(Mutex::new(TextSystemState::from_fonts(fonts)));
register_pipeline_text_state(measure_text_state.clone());
let text_measurer = WgpuTextMeasurer::new(measure_text_state);
set_text_measurer(text_measurer.clone());
```

Important consequence:

- UI-side text measurement and renderer-side glyph shaping intentionally share the same text system family.
- This is why text bugs often span both `cranpose-ui/src/text/*` and `cranpose-render/wgpu/src/*`.

## 7. Regular scroll is a layout modifier, not a compositor transform

Regular scroll is implemented by `ScrollNode`, which returns a placement offset from layout.

Small code example from `crates/cranpose-ui/src/scroll.rs`:

```rust
let scroll = self.state.value_non_reactive().clamp(0.0, max_scroll);

let abs_scroll = if self.reverse_scrolling {
    scroll - max_scroll
} else {
    -scroll
};

let (x_offset, y_offset) = if self.is_vertical {
    (0.0, abs_scroll)
} else {
    (abs_scroll, 0.0)
};

LayoutModifierMeasureResult::new(Size { width, height }, x_offset, y_offset)
```

That means:

- regular scroll becomes child placement during layout
- the renderer does not receive a retained "scroll transform node"
- by scene-build time, the subtree is already expressed as moved child geometry

This is one of the central facts behind the current scroll-phase rendering problems.

## 8. Lazy lists are different from regular scroll

Lazy lists do not use `ScrollNode` placement offsets. They compute visible items and place root item nodes directly during lazy measurement.

Small code example from `crates/cranpose-ui/src/widgets/lazy_list.rs`:

```rust
let result = measure_lazy_list(
    items_count,
    state,
    raw_viewport_size,
    cross_axis_size,
    config,
    measure_item,
);

let placements = create_lazy_list_placements(
    &result.visible_items,
    items_count,
    is_vertical,
    effective_viewport_size,
    config,
);
```

And each measured item stores concrete offsets and the root node ids it will place:

Small code example from `crates/cranpose-foundation/src/lazy/lazy_list_measured_item.rs`:

```rust
pub struct LazyListMeasuredItem {
    pub index: usize,
    pub offset: f32,
    pub node_ids: SmallNodeVec,
    pub child_offsets: SmallOffsetVec,
}
```

Important consequence:

- regular scroll and lazy scroll are not represented the same way
- lazy item motion is already flattened into node placements before the renderer sees it

## 9. WGPU scene building traverses live layout nodes and emits a flat scene

The WGPU pipeline traverses `LayoutNode` / `SubcomposeLayoutNode` directly.

Small code example from `crates/cranpose-render/wgpu/src/pipeline.rs`:

```rust
pub(crate) fn render_from_applier_with_root_scale(
    applier: &mut MemoryApplier,
    root: NodeId,
    scene: &mut Scene,
    scale: f32,
    root_scale: f32,
) {
    let root_layer = GraphicsLayer { scale, ..Default::default() };
    render_node_from_applier(
        applier, root, root_layer, scene, None, None, Point::default(), root_scale,
    );
}
```

Inside the traversal, it reads retained node state:

```rust
let abs_x = parent_offset.x + layout_state.position.x;
let abs_y = parent_offset.y + layout_state.position.y;

let rect = Rect {
    x: abs_x,
    y: abs_y,
    width: layout_state.size.width,
    height: layout_state.size.height,
};
```

This is not a retained scene graph with transform nodes. It is a traversal that resolves node geometry immediately.

## 10. The scene is flat and z-ordered

The current `Scene` is a set of flat arrays plus a monotonic `next_z`.

Small code example from `crates/cranpose-render/wgpu/src/scene.rs`:

```rust
pub struct Scene {
    pub shapes: Vec<DrawShape>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadow_draws: Vec<ShadowDraw>,
    pub hits: Vec<HitRegion>,
    pub effect_layers: Vec<EffectLayer>,
    pub backdrop_layers: Vec<BackdropLayer>,
    pub next_z: usize,
    pub node_index: HashMap<NodeId, usize>,
}
```

Every push increments `next_z`.

Small code example:

```rust
let z_index = self.next_z;
self.next_z += 1;
self.shapes.push(DrawShape { z_index, ... });
```

Important consequence:

- there is no explicit parent/child scene graph in the renderer
- subtree ordering is encoded by flattening into z ranges
- effect isolation is also encoded over those flat z ranges

## 11. Draw commands become primitives before the GPU sees them

Modifier draw commands are executed during scene build, not on the GPU.

Small code example from `crates/cranpose-ui/src/draw.rs`:

```rust
pub enum DrawCommand {
    Behind(DrawCommandFn),
    WithContent(DrawCommandFn),
    Overlay(DrawCommandFn),
}
```

The WGPU pipeline applies them like this:

Small code example from `crates/cranpose-render/wgpu/src/pipeline/style.rs`:

```rust
for command in commands {
    let primitives = primitives_for_placement(command, placement, size);
    for primitive in primitives {
        emit_primitive(primitive, rect, layer, clip, scene, None);
    }
}
```

Then primitives are flattened into scene records:

- `DrawPrimitive::Rect` -> `DrawShape`
- `DrawPrimitive::RoundRect` -> `DrawShape`
- `DrawPrimitive::Image` -> `ImageDraw`
- `DrawPrimitive::Shadow` -> `ShadowDraw`

## 12. Graphics layers are flattened into primitive geometry during scene build

The renderer combines nested graphics layers before it emits primitives.

Small code example from `crates/cranpose-render/wgpu/src/pipeline.rs`:

```rust
let node_layer = combine_layers(parent_layer, style.graphics_layer);
let transformed_rect = apply_layer_to_rect(rect, rect, &node_layer);
```

And for primitives:

```rust
let local_rect = apply_layer_affine_to_rect(draw_rect, layer_bounds, layer);
let quad = apply_layer_to_quad(draw_rect, layer_bounds, layer);
let transformed = quad_bounds(quad);
scene.push_shape_with_geometry(transformed, local_rect, quad, ...);
```

This is a critical current fact:

- subtree transforms are not preserved as subtree transforms
- they are resolved into per-primitive geometry during scene construction

That is the current implementation reality even if the target architecture should be different.

## 13. Text has its own scene path, separate from generic draw primitives

Node text content is detected from modifier slices and pushed through dedicated text helpers.

Small code example from `crates/cranpose-render/wgpu/src/pipeline.rs`:

```rust
if let Some(value) = modifier_slices.annotated_text() {
    let prepared = modifier_slices
        .prepare_text_layout(Some(measure_width).filter(|w| w.is_finite() && *w > 0.0))
        .expect("modifier text layout");

    push_text_style_draws_with_root_scale(
        scene, node_id, rect, text_rect, &content_layer, &prepared.text,
        text_style_ref, font_size, options, text_clip, root_scale,
    );
}
```

That helper may emit:

- `TextDraw`
- text-owned background rects
- underline / line-through geometry
- effect layers for gradient or stroke text material
- shadow text draws

So text is not "just another shape batch" in Cranpose.

## 14. The WGPU renderer owns scene storage and GPU execution

`WgpuRenderer` owns:

- the current `Scene`
- the GPU renderer
- render-time text state
- root scale

Small code example from `crates/cranpose-render/wgpu/src/lib.rs`:

```rust
pub struct WgpuRenderer {
    scene: Scene,
    gpu_renderer: Option<GpuRenderer>,
    render_text_state: TextSystemState,
    root_scale: f32,
}
```

Scene rebuild and GPU draw are separate:

```rust
fn rebuild_scene_from_applier(...) -> Result<(), Self::Error> {
    self.scene.clear();
    pipeline::render_from_applier_with_root_scale(
        applier, root, &mut self.scene, 1.0, self.root_scale,
    );
    Ok(())
}
```

```rust
pub fn render(&mut self, view: &wgpu::TextureView, width: u32, height: u32) -> Result<(), WgpuRendererError> {
    gpu_renderer.render(
        &mut self.render_text_state,
        view,
        &self.scene.shapes,
        &self.scene.images,
        &self.scene.texts,
        &self.scene.shadow_draws,
        &self.scene.effect_layers,
        &self.scene.backdrop_layers,
        width,
        height,
        self.root_scale,
    )
}
```

## 15. GPU execution is batch-based, not one-draw-call-per-node

`GpuRenderer::render(...)` receives the flat scene arrays and replays them in z order.

If there are no effect or backdrop layers, it takes a fast path.

Small code example from `crates/cranpose-render/wgpu/src/render.rs`:

```rust
if effect_layers.is_empty() && backdrop_layers.is_empty() {
    self.render_non_effect_segment(...)?;
} else {
    let accum = self.acquire_offscreen(width, height);
    self.render_range_with_layer_events_to_target(...)?;
    self.effect_renderer.composite_to_view(...);
}
```

Inside a non-effect segment, it groups work into shape/image/text batches:

```rust
for batch in chunk.iter() {
    match batch {
        SegmentBatchPlan::Shape { .. } => { ... }
        SegmentBatchPlan::Image { .. } => { ... }
        SegmentBatchPlan::Text { .. } => { ... }
    }
}
```

This means the current renderer is:

- flat-scene based
- z-segment based
- batch replay based

## 16. Shapes and images are uploaded as geometry buffers

Shapes are converted into vertex/index/uniform-compatible data before a render pass.

Small code example from `crates/cranpose-render/wgpu/src/render.rs`:

```rust
for (idx, shape) in layer_shapes.enumerate() {
    let (local_rect, quad) = apply_pixel_snap_to_geometry(
        shape.local_rect,
        shape.quad,
        root_scale,
        shape.pixel_snap,
    );
    // build ShapeData, vertices, and indices
}
```

Images follow the same general pattern:

```rust
let (_, quad) = apply_pixel_snap_to_geometry(
    image_draw.local_rect,
    image_draw.quad,
    root_scale,
    image_draw.pixel_snap,
);
```

So even though the backend is GPU-based, Cranpose still performs significant CPU-side geometry preparation every frame.

## 17. Text is rendered through glyphon with cached buffers and a text renderer pool

Text is not converted into triangles by Cranpose directly. It is prepared for `glyphon`.

Small code example from `crates/cranpose-render/wgpu/src/render.rs`:

```rust
let batch_signature =
    prepared_text_batch_signature(layer_texts.clone(), width, height, root_scale);
if batch_signature.is_some()
    && self.text_renderer_pool[slot_index].last_signature == batch_signature
{
    self.text_viewport.update(&self.queue, Resolution { width, height });
    return Ok(slot_index);
}
```

Each `TextDraw` becomes a `glyphon::TextArea`:

```rust
text_areas.push(TextArea {
    buffer: &cached.buffer,
    left: left_px,
    top: top_px,
    scale: 1.0,
    bounds,
    default_color: color,
    custom_glyphs: &[],
});
```

Important facts:

- text buffers are cached per node/style/size combination
- glyph shaping and layout reuse are major performance paths
- text rendering has separate batching and separate caching from shapes/images

## 18. Effects, backdrop, and blur use an offscreen target pool

Offscreen resources are pooled GPU textures.

Small code example from `crates/cranpose-render/wgpu/src/offscreen.rs`:

```rust
pub(crate) struct OffscreenPool {
    available: Vec<OffscreenTarget>,
    format: wgpu::TextureFormat,
}
```

Acquire/release is explicit:

```rust
pub fn acquire(&mut self, device: &wgpu::Device, width: u32, height: u32, ...) -> OffscreenTarget
pub fn release(&mut self, target: OffscreenTarget)
```

`EffectRenderer` owns the blur/offset/blit/runtime-shader infrastructure:

Small code example from `crates/cranpose-render/wgpu/src/effect_renderer.rs`:

```rust
pub(crate) struct EffectRenderer {
    pub offscreen_pool: OffscreenPool,
    pub shader_cache: ShaderPipelineCache,
    blur_pipeline: wgpu::RenderPipeline,
    offset_pipeline: wgpu::RenderPipeline,
    blit_pipeline: wgpu::RenderPipeline,
    // ...
}
```

This is why blur/backdrop bugs usually live in `render.rs`, `effect_renderer.rs`, and `offscreen.rs`, not only in shaders.

## 19. Hit testing is scene-owned and based on stable node ids

The scene stores `HitRegion`s and implements `RenderScene`.

Small code example from `crates/cranpose-render/wgpu/src/scene.rs`:

```rust
fn hit_test(&self, x: f32, y: f32) -> Vec<Self::HitTarget> {
    let mut hit_indices: Vec<usize> = self
        .hits
        .iter()
        .enumerate()
        .filter_map(|(index, hit)| hit.contains(x, y).then_some(index))
        .collect();

    hit_indices.sort_by_key(|&index| Reverse(self.hits[index].z_index));
    hit_indices.into_iter().map(|index| self.hits[index].clone()).collect()
}
```

And the render-common contract requires `find_target(node_id)`:

Small code example from `crates/cranpose-render/common/src/lib.rs`:

```rust
fn find_target(&self, node_id: cranpose_core::NodeId) -> Option<Self::HitTarget>;
```

The app shell uses that to resolve stored hit paths after layout changes. It caches node ids, not stale rectangles.

## 20. Pixel snapping in the current WGPU path is explicit policy, not a global rule

The scene records a snap policy:

Small code example from `crates/cranpose-render/wgpu/src/scene.rs`:

```rust
pub enum PixelSnap {
    Independent,
    None,
    FollowPoint(Point),
}
```

Current defaults:

- generic shapes default to `PixelSnap::None`
- generic images default to `PixelSnap::None`
- text only snaps when style explicitly says `TextMotion::Static`

Small code example from `crates/cranpose-render/wgpu/src/pipeline.rs`:

```rust
fn text_pixel_snap(rect: Rect, text_style: &TextStyle) -> PixelSnap {
    if matches!(
        text_style.paragraph_style.text_motion,
        Some(TextMotion::Static)
    ) {
        return PixelSnap::FollowPoint(Point { x: rect.x, y: rect.y });
    }

    PixelSnap::None
}
```

And render-time snap is applied in physical-pixel space:

Small code example from `crates/cranpose-render/wgpu/src/render.rs`:

```rust
fn snap_scalar_to_physical_pixels(value: f32, root_scale: f32) -> f32 {
    let scale = normalized_root_scale(root_scale);
    (value * scale).round() / scale
}
```

If a model says Cranpose globally rounds all geometry, that is no longer true. The renderer still has snap machinery, but the defaults are now much narrower.

## 21. Root scale is important

The scene is built in logical coordinates, and `root_scale` is applied later during GPU prep and text shaping.

Small code example from `crates/cranpose-render/wgpu/src/lib.rs`:

```rust
fn rebuild_scene_from_applier(...) -> Result<(), Self::Error> {
    self.scene.clear();
    pipeline::render_from_applier_with_root_scale(
        applier, root, &mut self.scene, 1.0, self.root_scale,
    );
    Ok(())
}
```

And GPU prep multiplies by `root_scale`:

```rust
position: [quad[0][0] * root_scale, quad[0][1] * root_scale],
```

For text:

```rust
let font_size_px = text_draw.font_size * text_draw.scale * root_scale;
```

So any reasoning about pixel alignment, clips, or blur radii must keep `root_scale` in mind.

## 22. What is structurally true today

These are the high-value facts a reasoning model should keep straight:

- The active scene rebuild path is `rebuild_scene_from_applier`, not layout-tree replay.
- Cranpose uses retained `LayoutNode` state.
- Regular scroll is a layout placement offset.
- Lazy list motion is item placement computed during lazy measurement.
- Modifier draw commands are executed during scene build into primitives.
- Text is renderer-owned and pulled from modifier slices.
- The WGPU scene is flat, not hierarchical.
- Effects and backdrop are replayed through z ranges and offscreen passes.
- The renderer does significant CPU-side staging each frame before issuing GPU draws.
- Hit testing is scene-owned and keyed by stable `NodeId`.
- Pixel snapping exists, but generic content defaults to `None` now.

## 23. What a model should not assume

A model should not assume any of these unless it has looked at the code:

- that scrolling is a compositor-only parent matrix
- that text draw nodes paint directly through `DrawScope`
- that the renderer stores a retained layer tree
- that lazy list uses the same motion representation as regular scroll
- that all scene data is already in device pixels at scene-build time
- that all snapping is disabled
- that the current renderer matches the target architecture described in `docs/render_arch.md`

## 24. Suggested companion files

If another model needs more detail, these are the next files to read:

- `docs/render_arch.md`
- `crates/cranpose-app-shell/src/lib.rs`
- `crates/cranpose-ui/src/widgets/nodes/layout_node.rs`
- `crates/cranpose-ui/src/modifier/slices.rs`
- `crates/cranpose-ui/src/scroll.rs`
- `crates/cranpose-ui/src/widgets/lazy_list.rs`
- `crates/cranpose-render/wgpu/src/pipeline.rs`
- `crates/cranpose-render/wgpu/src/scene.rs`
- `crates/cranpose-render/wgpu/src/render.rs`
- `crates/cranpose-render/wgpu/src/effect_renderer.rs`
