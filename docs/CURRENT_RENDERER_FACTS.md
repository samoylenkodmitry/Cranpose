# Current Renderer Facts

This is the short current-state snapshot for the active `renderer` branch.

It describes the code that exists now. It does not describe abandoned designs.

## Frame Pipeline

```text
AppShell::process_frame
  -> run_layout_phase()
  -> run_render_phase()
    -> Renderer::rebuild_scene_from_applier(...)
      -> scene_builder::build_graph_from_applier(...)
      -> collect_hits_from_graph(...)
      -> scene.graph = Some(RenderGraph)
    -> WgpuRenderer::render(...)
      -> GpuRenderer::render_graph(...)
```

Relevant files:

- `crates/cranpose-app-shell/src/lib.rs`
- `crates/cranpose-render/common/src/scene_builder.rs`
- `crates/cranpose-render/common/src/hit_graph.rs`
- `crates/cranpose-render/wgpu/src/render.rs`

## Shared Render Graph

The semantic scene is hierarchical.

```rust
pub enum RenderNode {
    Primitive(PrimitiveEntry),
    Layer(Box<LayerNode>),
}

pub struct LayerNode {
    pub node_id: Option<NodeId>,
    pub local_bounds: Rect,
    pub transform_to_parent: ProjectiveTransform,
    pub motion_context_animated: bool,
    pub translated_content_context: bool,
    pub graphics_layer: GraphicsLayer,
    pub hit_test: Option<HitTestNode>,
    pub has_hit_targets: bool,
    pub isolation: IsolationReasons,
    pub cache_policy: CachePolicy,
    pub cache_hashes: LayerRasterCacheHashes,
    pub cache_hashes_valid: bool,
    pub children: Vec<RenderNode>,
}
```

Source: `crates/cranpose-render/common/src/graph.rs`

Important facts:

- subtree motion is carried by `transform_to_parent`
- active scroll/drag/fling ancestry is carried by `motion_context_animated`
- scroll and lazy-scroll subtrees carry persistent `translated_content_context`
- interactive-subtree presence is carried by `has_hit_targets`
- raster-cache hashes are lazy through `cache_hashes_valid`

## Graph Build

The hot applier path is now a single traversal. It no longer allocates a full
snapshot tree and then walk that tree again.

```rust
pub fn build_graph_from_applier(
    applier: &mut MemoryApplier,
    root: NodeId,
    scale: f32,
) -> Option<RenderGraph> {
    Some(RenderGraph {
        root: build_layer_node_from_applier(applier, root, scale, false)?,
    })
}
```

The builder still:

- preserves local primitive geometry
- turns placement plus `GraphicsLayer` into `transform_to_parent`
- folds parent `content_offset` into child transforms
- reads `ModifierNodeSlices::motion_context_animated()` from the UI modifier chain
- reads `ModifierNodeSlices::translated_content_context()` from the UI modifier chain
- resolves unspecified text under active motion ancestry to `TextMotion::Animated`
- prepares `TextPrimitiveNode` during graph build

The builder does not eagerly recompute raster-cache hashes for every layer.
It does not treat nonzero `content_offset` by itself as active motion.

## Hit Testing

Hit testing is graph-based and transform-aware.

```rust
pub fn collect_hits_from_graph<S: HitGraphSink>(...) {
    if !layer.has_hit_targets {
        return;
    }
    collect_hits_from_graph_inner(...);
}
```

Each stored hit region keeps:

- broadphase rect
- exact transformed quad
- inverse transform to local space
- transformed clip chain

This is implemented in:

- `crates/cranpose-render/common/src/hit_graph.rs`
- `crates/cranpose-render/common/src/graph_scene.rs`

## WGPU Execution Shape

The WGPU backend is a recursive compositor over the graph, not a global flat
scene renderer.

Root decision:

```rust
fn render_graph(...) -> Result<(), String> {
    self.layer_surface_rect_cache.clear();
    self.layer_surface_requirements_cache.clear();
    if root_can_render_directly_cached(&graph.root, &mut self.layer_surface_requirements_cache) {
        let collected = collect_layer_contents(...);
        if collected.scene.effect_layers.is_empty() && collected.scene.backdrop_layers.is_empty() {
            return self.render_root_direct(..., collected, ...);
        }
    }
    let root_surface = self.render_layer_surface(...)?;
    ...
}
```

Two scene shapes exist on purpose:

- semantic graph: `RenderGraph`
- per-target execution scene: `CompositorScene`

```rust
pub(crate) struct CompositorScene {
    pub shapes: Vec<DrawShape>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadow_draws: Vec<ShadowDraw>,
    pub effect_layers: Vec<EffectLayer>,
    pub backdrop_layers: Vec<BackdropLayer>,
    pub next_z: usize,
}
```

Source: `crates/cranpose-render/wgpu/src/scene.rs`

## Direct vs Isolated Layers

Per-frame layer classification is cached by layer pointer.

```rust
struct LayerSurfaceRequirements {
    direct_translation: Option<Point>,
    reasons: LayerSurfaceReasons,
}
```

Runtime reasons:

```rust
pub struct LayerSurfaceReasons {
    pub explicit_offscreen: bool,
    pub effect: bool,
    pub backdrop: bool,
    pub group_opacity: bool,
    pub blend_mode: bool,
    pub immediate_shadow: bool,
    pub text_local_surface: bool,
    pub mixed_direct_content: bool,
    pub non_translation_transform: bool,
}
```

Source: `crates/cranpose-render/wgpu/src/gpu_stats.rs`

Current rule:

- pure translation + no reasons => direct
- otherwise => isolated bounded local surface

## Direct Collection

Direct child layers are no longer collected into temporary child scenes and then
translated/merged upward.

The current collector is one-pass:

```rust
fn collect_layer_contents_into(
    layer: &LayerNode,
    inherited_clip: Option<Rect>,
    layer_offset: Point,
    local_scene: &mut CompositorScene,
    child_layers: &mut Vec<ChildLayerComposite<'_>>,
    ...
)
```

This collector:

- emits local primitives directly into the target scene
- recurses into direct children with accumulated translation
- records isolated children as `ChildLayerComposite`

## Text Policy

Current split:

- plain text and decoration-only text stay on the direct path
- static plain text leaves use one rigid snap anchor
- gradient/stroke/span-material text emits GPU `EffectLayer`s
- complex text uses a bounded local surface via `LayerSurfaceReasons::text_local_surface`

The direct-collapse path rebases `TextPrimitiveNode.rect` into parent space
before emitting text/decorations, so collapsed underlined text no longer clips
down to a tiny fragment.

`push_text_style_draws(...)` no longer rounds static text rects in logical
space. Scene-space text geometry stays unchanged; any remaining snap policy is
render-time only through shared leaf snap anchors. Static pure-text leaves now
participate in that same snap-anchor path; it is not limited to layers that
also contain sibling draw primitives.

Unspecified text inside motion or translated-content ancestry resolves to
`TextMotion::Animated` during graph build. Scroll and lazy-scroll modifiers keep
`translated_content_context` enabled for the whole subtree and use
`motion_context_animated` only for active drag/wheel/fling state.

Complex text local surfaces are now used to preserve rigid-picture invariance
for text that cannot stay visually coherent as loose primitives under parent
translation. Current triggers include:

- gradient or stroke text
- span-level foreground/material overrides
- text shadow
- background
- baseline shift
- non-identity geometric transform
- letter spacing

The direct-path text policy lives in
`crates/cranpose-render/wgpu/src/pipeline.rs` and
`crates/cranpose-render/wgpu/src/render.rs`.

## Image Policy

Image sampling now follows motion context:

- static images use nearest sampling
- images inside translated-content or active-motion subtrees use linear sampling

This keeps static icons/pixel-art crisp while avoiding nearest-neighbor phase
stepping during scroll.

## Robot Capture

Robot screenshot capture now prefers logical layout size and captures at
`capture_scale = 1.0`, matching `main` raw PNG dimensions again.

`RobotScreenshot` still carries logical extents, and robot helpers still map
logical semantic/layout regions back onto the captured image before sampling,
cropping, or region diffing.

This fixed the raw screenshot size mismatch between `main` and `renderer` for
the micro contract.

## Current Limitation

Headless logical-size robot captures are now correct and useful for branch
parity, but presented fractional-scale crispness still has to be judged from
saved demo screenshots, not raw PNG size alone.

Relevant files:

- `crates/cranpose/src/desktop.rs`
- `crates/cranpose-testing/src/robot_helpers.rs`
- `crates/cranpose-render/wgpu/src/lib.rs`

## Current Acceptance Facts

Validated current branch behavior:

- transformed hit testing uses exact quads and inverse transforms
- translation-only wrappers with plain text do zero offscreen work
- scrolled text stays on one translated-content motion path instead of switching on release
- idle standalone text headings also re-enter the static snap path
- complex text uses bounded local surfaces instead of trying to preserve rigid motion as loose primitives
- root direct rendering only applies when the collected root scene has no local effect/backdrop events
- scrolling images use linear sampling while static images stay nearest
- oversized Shaders-tab mixed-content isolates are guarded by a robot test
- screenshot capture is physical-size aware

## Important Limits

- scroll and lazy-list motion are still produced by layout in `cranpose-ui`
- isolated layers still render through bounded offscreens and projective composite
- frame stats report GPU/compositor work; they do not yet report CPU phase timing
