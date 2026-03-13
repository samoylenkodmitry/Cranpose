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
        return self.render_root_direct(...);
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
    pub text_local_surface: bool,
    pub immediate_shadow: bool,
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

Plain translation-only text stays on the direct path.

Text still forces a local surface for some effectful/styled cases:

- span styles
- shadow
- background
- baseline shift
- text geometric transform
- draw style
- specified letter spacing
- non-solid brush

Decoration-only text is direct again. The direct-collapse path now rebases
`TextPrimitiveNode.rect` into parent space before emitting text/decorations, so
collapsed underlined text no longer clips down to a tiny fragment.

Unspecified text inside `motion_context_animated` subtrees is resolved to
`TextMotion::Animated` during graph build, but that flag now tracks active
motion, not mere scroll position. Idle scroll containers stay on the static
path.

The classifier and direct-collapse text rebasing live in
`crates/cranpose-render/wgpu/src/render.rs`.

## Image Policy

Image sampling now follows motion context:

- static images use nearest sampling
- images inside `motion_context_animated` subtrees use linear sampling

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
The current micro contract screenshot is pixel-identical to
`docs/render-reference/main_renderer_micro_contract.png`.

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
- only active scroll motion resolves unspecified text to animated motion
- scrolling images use linear sampling while static images stay nearest
- root direct rendering skips the old root offscreen for direct scenes
- oversized Shaders-tab mixed-content isolates are guarded by a robot test
- screenshot capture is physical-size aware

Current screenshot-based review tools:

- `robot_measure_shaders` can save real stage screenshots under
  `/tmp/cranpose_shaders_visual_compare`
- `robot_renderer_micro_contract` renders a tiny deterministic surface, saves
  `/tmp/cranpose_renderer_micro_contract.png`, and validates exact
  pixels for image/line/fill primitives plus text-region presence
- committed `main` reference screenshot:
  `docs/render-reference/main_renderer_micro_contract.png`

Latest sequential perf spot checks on this machine:

- `renderer` `opaque_scene`: `350.5 fps`
- temp `main` `opaque_scene`: `315.2 fps`
- `renderer` `backdrop_blur`: `207.1 fps`
- temp `main` `backdrop_blur`: `209.5 fps`

These numbers came from the robot perf harness with:

- `CRANPOSE_HEADLESS=1`
- `CRANPOSE_PRESENT_MODE=immediate`
- `CRANPOSE_PERF_DURATION_SECS=3`
- `CRANPOSE_PERF_WARMUP_SECS=2`

## Important Limits

- scroll and lazy-list motion are still produced by layout in `cranpose-ui`
- isolated layers still render through bounded offscreens and projective composite
- frame stats report GPU/compositor work; they do not yet report CPU phase timing
