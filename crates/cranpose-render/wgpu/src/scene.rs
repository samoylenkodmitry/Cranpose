//! Scene structures for GPU rendering

use crate::surface_requirements::SurfaceRequirementSet;
use cranpose_core::NodeId;
pub use cranpose_render_common::graph_scene::{ClickAction, HitRegion, Scene};
use cranpose_ui::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, ImageBitmap, ImageSampling, Point, Rect,
    RenderEffect, RoundedCornerShape, Stroke,
};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SnapAnchor {
    pub origin: Point,
    pub device_pixel_step: f32,
}

impl SnapAnchor {
    pub(crate) fn rigid(origin: Point) -> Self {
        Self {
            origin,
            device_pixel_step: 1.0,
        }
    }
}

#[derive(Clone)]
pub(crate) struct DrawShape {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub snap_anchor: Option<SnapAnchor>,
    pub brush: Brush,
    pub shape: Option<RoundedCornerShape>,
    /// `Some` strokes the outline of `local_rect`/`shape` instead of filling
    /// it. `local_rect` and `quad` are already inflated by half the width.
    pub stroke: Option<Stroke>,
    /// `Some` replaces the rect geometry with a circular band, in `local_rect`
    /// units. Mutually exclusive with `stroke`/`shape`.
    pub arc: Option<ArcGeometry>,
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub motion_context_animated: bool,
}

impl DrawShape {
    /// A shape that needs the analytic stroke or arc path in the shader.
    #[cfg(test)]
    pub fn has_stroke_or_arc(&self) -> bool {
        self.stroke.is_some() || self.arc.is_some()
    }
}

#[derive(Clone)]
pub(crate) struct TextDraw {
    pub node_id: NodeId,
    pub rect: Rect,
    pub snap_anchor: Option<SnapAnchor>,
    pub translated_content_context: bool,
    pub text: Rc<cranpose_ui::text::AnnotatedString>,
    pub color: Color,
    pub text_style: TextStyle,
    pub font_size: f32,
    pub scale: f32,
    pub layout_options: TextLayoutOptions,
    pub z_index: usize,
    pub clip: Option<Rect>,
}

#[derive(Clone)]
pub(crate) struct ImageDraw {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub snap_anchor: Option<SnapAnchor>,
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub sampling: ImageSampling,
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    /// Source sub-region in image-pixel coordinates. `None` means full image.
    pub src_rect: Option<Rect>,
    pub motion_context_animated: bool,
}

/// CPU mirror of the shader's per-batch `SimilarityTransform`: rotate by the
/// angle whose (cos, sin) is `rot` and scale by `scale`, about `center`, in
/// device pixels. Freshly converted batches bind [`Self::IDENTITY`] through a
/// buffer shared renderer-wide; replayed batches bind their own value.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SimilarityTransform {
    pub(crate) center: [f32; 2],
    pub(crate) rot: [f32; 2],
    pub(crate) scale: f32,
    _pad: [f32; 3],
}

impl SimilarityTransform {
    pub(crate) const IDENTITY: Self = Self {
        center: [0.0, 0.0],
        rot: [1.0, 0.0],
        scale: 1.0,
        _pad: [0.0; 3],
    };

    pub(crate) fn new(center: [f32; 2], angle: f32, scale: f32) -> Self {
        Self {
            center,
            rot: [angle.cos(), angle.sin()],
            scale,
            _pad: [0.0; 3],
        }
    }
}

/// One replayed shape batch: GPU slots captured from an earlier frame's
/// converted shapes, drawn this frame under `transform`. The heavy per-shape
/// pipeline (emit, walk, convert, upload) never sees these shapes again —
/// the scene carries this one op where thousands of shape ops used to be.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RetainedDraw {
    /// Renderer-side replay slot holding the retained buffers and bind group.
    pub slot: u32,
    pub transform: SimilarityTransform,
    /// Screen-space bounds of the transformed batch, for visibility checks.
    pub bounds: Rect,
    /// First shape drawn within the slot's capture — draws sharing a slot
    /// after a segment split cover disjoint ranges of it.
    pub first_shape: u32,
    /// How many shapes the retained batch draws (6 vertices each).
    pub shape_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DrawOpKind {
    Shape(usize),
    Image(usize),
    Text(usize),
    Shadow(usize),
    /// Index into [`CompositorScene::retained_draws`].
    Retained(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DrawOp {
    pub z_index: usize,
    pub kind: DrawOpKind,
}

/// A shadow that requires GPU blur processing.
#[derive(Clone)]
pub(crate) struct ShadowDraw {
    /// Shapes to render to offscreen target before blur.
    /// Each shape carries its own blend mode (SrcOver for fill, DstOut for cutout).
    pub shapes: Vec<(DrawShape, BlendMode)>,
    /// Texts to render to offscreen target before blur.
    pub texts: Vec<TextDraw>,
    /// Gaussian blur radius in pixels.
    pub blur_radius: f32,
    /// Optional clip rect applied when compositing (inner shadows clip to element bounds).
    pub clip: Option<Rect>,
    /// Z-index for correct draw ordering.
    pub z_index: usize,
}

/// A scene span that should be rendered into an isolated surface.
#[derive(Clone)]
pub(crate) struct EffectLayer {
    pub rect: Rect,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<SnapAnchor>,
    /// Optional effect to apply to the offscreen subtree.
    /// `None` means isolate/composite only (no post-effect shader).
    pub effect: Option<RenderEffect>,
    /// Blend mode used when compositing the offscreen subtree back to the parent.
    pub blend_mode: BlendMode,
    /// Alpha applied when compositing the offscreen subtree back to the parent.
    pub composite_alpha: f32,
    /// Z-index of the first draw item in this effect layer's subtree.
    pub z_start: usize,
    /// Z-index one past the last draw item in this effect layer's subtree.
    pub z_end: usize,
    /// Surface requirements that determine target scale and composite policy.
    pub requirements: SurfaceRequirementSet,
}

/// A backdrop effect applied to already-rendered content behind a node.
#[derive(Clone)]
pub(crate) struct BackdropLayer {
    pub node_id: Option<NodeId>,
    pub rect: Rect,
    pub clip: Option<Rect>,
    pub effect: RenderEffect,
    /// Z-index at which this backdrop effect should be applied.
    pub z_index: usize,
}

pub(crate) struct CompositorScene {
    pub shapes: Vec<DrawShape>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadow_draws: Vec<ShadowDraw>,
    pub draw_ops: Vec<DrawOp>,
    pub effect_layers: Vec<EffectLayer>,
    pub backdrop_layers: Vec<BackdropLayer>,
    pub retained_draws: Vec<RetainedDraw>,
    pub next_z: usize,
}

/// Last frame's element counts, used to pre-size the next frame's scene Vecs.
/// A fully animated scene re-collects every primitive each frame; growing the
/// Vecs from empty re-copies roughly twice the final payload through the
/// doubling schedule, which is pure overhead once the sizes are known.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SceneCapacityHint {
    pub shapes: usize,
    pub images: usize,
    pub texts: usize,
    pub shadow_draws: usize,
    pub draw_ops: usize,
    pub effect_layers: usize,
    pub backdrop_layers: usize,
}

/// How many dropped scenes' buffers each thread keeps for reuse. Scenes are
/// collected fresh every frame (root plus one per composited child layer);
/// for a heavy animated frame the draw vectors are megabytes, big enough
/// that dropping and reallocating them round-trips through mmap each frame.
const SCENE_BUFFER_POOL_LIMIT: usize = 4;

thread_local! {
    static SCENE_BUFFER_POOL: std::cell::RefCell<Vec<SceneBuffers>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// The emptied-but-still-allocated vectors of a dropped [`CompositorScene`].
struct SceneBuffers {
    shapes: Vec<DrawShape>,
    images: Vec<ImageDraw>,
    texts: Vec<TextDraw>,
    shadow_draws: Vec<ShadowDraw>,
    draw_ops: Vec<DrawOp>,
    effect_layers: Vec<EffectLayer>,
    backdrop_layers: Vec<BackdropLayer>,
}

impl Drop for CompositorScene {
    fn drop(&mut self) {
        SCENE_BUFFER_POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            if pool.len() >= SCENE_BUFFER_POOL_LIMIT {
                return;
            }
            self.clear();
            pool.push(SceneBuffers {
                shapes: std::mem::take(&mut self.shapes),
                images: std::mem::take(&mut self.images),
                texts: std::mem::take(&mut self.texts),
                shadow_draws: std::mem::take(&mut self.shadow_draws),
                draw_ops: std::mem::take(&mut self.draw_ops),
                effect_layers: std::mem::take(&mut self.effect_layers),
                backdrop_layers: std::mem::take(&mut self.backdrop_layers),
            });
        });
    }
}

impl CompositorScene {
    pub fn new() -> Self {
        Self::with_capacity(SceneCapacityHint::default())
    }

    pub fn with_capacity(hint: SceneCapacityHint) -> Self {
        if let Some(buffers) = SCENE_BUFFER_POOL.with(|pool| pool.borrow_mut().pop()) {
            return Self {
                shapes: buffers.shapes,
                images: buffers.images,
                texts: buffers.texts,
                shadow_draws: buffers.shadow_draws,
                draw_ops: buffers.draw_ops,
                effect_layers: buffers.effect_layers,
                backdrop_layers: buffers.backdrop_layers,
                retained_draws: Vec::new(),
                next_z: 0,
            };
        }
        Self {
            shapes: Vec::with_capacity(hint.shapes),
            images: Vec::with_capacity(hint.images),
            texts: Vec::with_capacity(hint.texts),
            shadow_draws: Vec::with_capacity(hint.shadow_draws),
            draw_ops: Vec::with_capacity(hint.draw_ops),
            effect_layers: Vec::with_capacity(hint.effect_layers),
            backdrop_layers: Vec::with_capacity(hint.backdrop_layers),
            retained_draws: Vec::new(),
            next_z: 0,
        }
    }

    pub fn capacity_hint(&self) -> SceneCapacityHint {
        SceneCapacityHint {
            shapes: self.shapes.len(),
            images: self.images.len(),
            texts: self.texts.len(),
            shadow_draws: self.shadow_draws.len(),
            draw_ops: self.draw_ops.len(),
            effect_layers: self.effect_layers.len(),
            backdrop_layers: self.backdrop_layers.len(),
        }
    }

    pub fn clear(&mut self) {
        self.shapes.clear();
        self.images.clear();
        self.texts.clear();
        self.shadow_draws.clear();
        self.draw_ops.clear();
        self.effect_layers.clear();
        self.backdrop_layers.clear();
        self.retained_draws.clear();
        self.next_z = 0;
    }

    /// Pushes one retained-batch draw at the next z position and returns it.
    pub fn push_retained_draw(&mut self, draw: RetainedDraw) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.retained_draws.push(draw);
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Retained(self.retained_draws.len() - 1),
        });
    }

    pub fn push_shape(
        &mut self,
        rect: Rect,
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        self.push_shape_with_geometry(
            rect,
            rect,
            rect_to_quad(rect),
            brush,
            shape,
            clip,
            blend_mode,
            false,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_shape_with_geometry(
        &mut self,
        rect: Rect,
        local_rect: Rect,
        quad: [[f32; 2]; 4],
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
        motion_context_animated: bool,
    ) {
        self.push_shape_with_stroke_and_arc(
            rect,
            local_rect,
            quad,
            brush,
            shape,
            None,
            None,
            clip,
            blend_mode,
            motion_context_animated,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_shape_with_stroke_and_arc(
        &mut self,
        rect: Rect,
        local_rect: Rect,
        quad: [[f32; 2]; 4],
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        stroke: Option<Stroke>,
        arc: Option<ArcGeometry>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
        motion_context_animated: bool,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.shapes.len();
        self.shapes.push(DrawShape {
            rect,
            local_rect,
            quad,
            snap_anchor: None,
            brush,
            shape,
            stroke,
            arc,
            z_index,
            clip,
            blend_mode,
            motion_context_animated,
        });
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Shape(index),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_image_with_geometry(
        &mut self,
        rect: Rect,
        local_rect: Rect,
        quad: [[f32; 2]; 4],
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
        clip: Option<Rect>,
        src_rect: Option<Rect>,
        blend_mode: BlendMode,
        motion_context_animated: bool,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.images.len();
        self.images.push(ImageDraw {
            rect,
            local_rect,
            quad,
            snap_anchor: None,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            sampling,
            z_index,
            clip,
            blend_mode,
            src_rect,
            motion_context_animated,
        });
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Image(index),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_text(
        &mut self,
        node_id: NodeId,
        rect: Rect,
        text: Rc<cranpose_ui::text::AnnotatedString>,
        color: Color,
        text_style: TextStyle,
        font_size: f32,
        scale: f32,
        layout_options: TextLayoutOptions,
        clip: Option<Rect>,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.texts.len();
        self.texts.push(TextDraw {
            node_id,
            rect,
            snap_anchor: None,
            translated_content_context: false,
            text,
            color,
            text_style,
            font_size,
            scale,
            layout_options,
            z_index,
            clip,
        });
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Text(index),
        });
    }

    pub fn push_shadow_draw(&mut self, mut draw: ShadowDraw) {
        let z_index = self.next_z;
        self.next_z += 1;
        let index = self.shadow_draws.len();
        draw.z_index = z_index;
        self.shadow_draws.push(draw);
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Shadow(index),
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_effect_layer_with_requirements(
        &mut self,
        rect: Rect,
        clip: Option<Rect>,
        effect: Option<RenderEffect>,
        blend_mode: BlendMode,
        composite_alpha: f32,
        z_start: usize,
        z_end: usize,
        requirements: SurfaceRequirementSet,
    ) {
        self.effect_layers.push(EffectLayer {
            rect,
            clip,
            snap_anchor: None,
            effect,
            blend_mode,
            composite_alpha,
            z_start,
            z_end,
            requirements,
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub fn push_effect_layer(
        &mut self,
        rect: Rect,
        clip: Option<Rect>,
        effect: Option<RenderEffect>,
        blend_mode: BlendMode,
        composite_alpha: f32,
        z_start: usize,
        z_end: usize,
    ) {
        let requirements = effect
            .as_ref()
            .map(|_| {
                SurfaceRequirementSet::default()
                    .with(crate::surface_requirements::SurfaceRequirement::RenderEffect)
            })
            .unwrap_or_default();
        self.push_effect_layer_with_requirements(
            rect,
            clip,
            effect,
            blend_mode,
            composite_alpha,
            z_start,
            z_end,
            requirements,
        );
    }
}

impl Default for CompositorScene {
    fn default() -> Self {
        Self::new()
    }
}

use crate::rect_to_quad;
