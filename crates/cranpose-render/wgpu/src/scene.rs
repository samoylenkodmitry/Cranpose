//! Scene structures for GPU rendering

use std::rc::Rc;

use cranpose_core::NodeId;
pub use cranpose_render_common::graph_scene::{ClickAction, HitRegion, Scene};
use cranpose_render_common::style_shared::ResolvedBrush;
use cranpose_ui::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, ImageBitmap, ImageSampling, Point, Rect,
    RenderEffect, RoundedCornerShape, Stroke,
};

use crate::surface_requirements::SurfaceRequirementSet;

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

#[derive(Clone, Copy)]
#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct ColorPatch {
    pub slot: u32,
    pub shape_index: u32,
    pub color: [f32; 4],
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) struct PendingFeedCapture {
    pub key: (cranpose_render_common::graph::DrawCommandId, u32),
    pub shape_start: usize,
    pub shape_count: usize,
    pub fingerprint: u64,
    pub capture_clip: Option<Rect>,
    pub frame: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SceneBrush {
    Solid(Color),
    Gradient(u32),
}

pub(crate) fn intern_brush_into(table: &mut Vec<Brush>, brush: ResolvedBrush) -> SceneBrush {
    match brush {
        ResolvedBrush::Solid(color) => SceneBrush::Solid(color),
        ResolvedBrush::Other(brush) => {
            let index = table.len() as u32;
            table.push(brush);
            SceneBrush::Gradient(index)
        }
    }
}

impl SceneBrush {
    pub fn resolve<'a>(&self, brushes: &'a [Brush]) -> std::borrow::Cow<'a, Brush> {
        match *self {
            SceneBrush::Solid(color) => std::borrow::Cow::Owned(Brush::Solid(color)),
            SceneBrush::Gradient(index) => std::borrow::Cow::Borrowed(&brushes[index as usize]),
        }
    }

    pub fn render_hash(&self, brushes: &[Brush]) -> u64 {
        use cranpose_ui_graphics::RenderHash as _;
        match *self {
            SceneBrush::Solid(color) => Brush::Solid(color).render_hash(),
            SceneBrush::Gradient(index) => brushes[index as usize].render_hash(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DrawShape {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub snap_anchor: Option<SnapAnchor>,
    pub brush: SceneBrush,
    pub shape: Option<RoundedCornerShape>,
    pub stroke: Option<Stroke>,
    pub arc: Option<ArcGeometry>,
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub motion_context_animated: bool,
}

impl DrawShape {
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
    pub text: std::sync::Arc<cranpose_ui::text::RenderString>,
    pub color: Color,
    pub text_style: TextStyle,
    pub font_size: f32,
    pub scale: f32,
    pub layout_options: TextLayoutOptions,
    pub z_index: usize,
    pub clip: Option<Rect>,
}

const RENDER_STRING_MEMO_CAPACITY: usize = 2048;

thread_local! {
    static RENDER_STRING_MEMO: std::cell::RefCell<
        cranpose_core::collections::map::HashMap<usize, RenderStringMemoEntry>,
    > = std::cell::RefCell::new(cranpose_core::collections::map::HashMap::default());
}

struct RenderStringMemoEntry {
    text: std::rc::Weak<cranpose_ui::text::AnnotatedString>,
    render: std::sync::Arc<cranpose_ui::text::RenderString>,
}

pub(crate) fn render_string_for(
    text: &Rc<cranpose_ui::text::AnnotatedString>,
) -> std::sync::Arc<cranpose_ui::text::RenderString> {
    RENDER_STRING_MEMO.with(|memo| {
        let mut memo = memo.borrow_mut();
        let key = Rc::as_ptr(text) as usize;
        if let Some(entry) = memo.get(&key)
            && entry.text.strong_count() > 0
            && entry.text.as_ptr() == Rc::as_ptr(text)
        {
            return std::sync::Arc::clone(&entry.render);
        }

        let render = std::sync::Arc::new(text.render_string());
        if memo.len() >= RENDER_STRING_MEMO_CAPACITY {
            memo.retain(|_, entry| entry.text.strong_count() > 0);
            if memo.len() >= RENDER_STRING_MEMO_CAPACITY {
                memo.clear();
            }
        }
        memo.insert(
            key,
            RenderStringMemoEntry {
                text: Rc::downgrade(text),
                render: std::sync::Arc::clone(&render),
            },
        );
        render
    })
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
    pub src_rect: Option<Rect>,
    pub motion_context_animated: bool,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SimilarityTransform {
    pub(crate) center: [f32; 2],
    pub(crate) rot: [f32; 2],
    pub(crate) scale: f32,
    paint_select: f32,
    _pad: [f32; 2],
}

impl SimilarityTransform {
    pub(crate) const IDENTITY: Self = Self {
        center: [0.0, 0.0],
        rot: [1.0, 0.0],
        scale: 1.0,
        paint_select: 0.0,
        _pad: [0.0; 2],
    };

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn new(center: [f32; 2], angle: f32, scale: f32) -> Self {
        Self {
            center,
            rot: [angle.cos(), angle.sin()],
            scale,
            paint_select: 0.0,
            _pad: [0.0; 2],
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn with_retained_paint(mut self) -> Self {
        self.paint_select = 1.0;
        self
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct RetainedDraw {
    pub slot: u32,
    pub transform: SimilarityTransform,
    pub bounds: Rect,
    pub first_shape: u32,
    pub shape_count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DrawOpKind {
    Shape(usize),
    Image(usize),
    Text(usize),
    Shadow(usize),
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    Retained(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DrawOp {
    pub z_index: usize,
    pub kind: DrawOpKind,
}

#[derive(Clone)]
pub(crate) struct ShadowDraw {
    pub shapes: Vec<(DrawShape, BlendMode)>,
    pub post_blur_cutouts: Vec<(DrawShape, BlendMode)>,
    pub brushes: Vec<Brush>,
    pub texts: Vec<TextDraw>,
    pub blur_radius: f32,
    pub clip: Option<Rect>,
    pub occluder: Option<Rect>,
    pub z_index: usize,
}

#[derive(Clone)]
pub(crate) struct EffectLayer {
    pub rect: Rect,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<SnapAnchor>,
    pub effect: Option<RenderEffect>,
    pub blend_mode: BlendMode,
    pub composite_alpha: f32,
    pub z_start: usize,
    pub z_end: usize,
    pub requirements: SurfaceRequirementSet,
}

#[derive(Clone)]
pub(crate) struct BackdropLayer {
    pub node_id: Option<NodeId>,
    pub rect: Rect,
    pub clip: Option<Rect>,
    pub snap_anchor: Option<SnapAnchor>,
    pub effect: RenderEffect,
    pub z_index: usize,
}

pub(crate) struct CompositorScene {
    pub shapes: Vec<DrawShape>,
    pub brushes: Vec<Brush>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadow_draws: Vec<ShadowDraw>,
    pub draw_ops: Vec<DrawOp>,
    pub effect_layers: Vec<EffectLayer>,
    pub backdrop_layers: Vec<BackdropLayer>,
    pub retained_draws: Vec<RetainedDraw>,
    pub next_z: usize,
}

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

const SCENE_BUFFER_POOL_LIMIT: usize = 4;

thread_local! {
    static SCENE_BUFFER_POOL: std::cell::RefCell<Vec<SceneBuffers>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct SceneBuffers {
    shapes: Vec<DrawShape>,
    brushes: Vec<Brush>,
    images: Vec<ImageDraw>,
    texts: Vec<TextDraw>,
    shadow_draws: Vec<ShadowDraw>,
    draw_ops: Vec<DrawOp>,
    effect_layers: Vec<EffectLayer>,
    backdrop_layers: Vec<BackdropLayer>,
}

impl Drop for CompositorScene {
    fn drop(&mut self) {
        let _ = SCENE_BUFFER_POOL.try_with(|pool| {
            let Ok(mut pool) = pool.try_borrow_mut() else {
                return;
            };
            if pool.len() >= SCENE_BUFFER_POOL_LIMIT {
                return;
            }
            self.clear();
            pool.push(SceneBuffers {
                shapes: std::mem::take(&mut self.shapes),
                brushes: std::mem::take(&mut self.brushes),
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
                brushes: buffers.brushes,
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
            brushes: Vec::new(),
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
        self.brushes.clear();
        self.images.clear();
        self.texts.clear();
        self.shadow_draws.clear();
        self.draw_ops.clear();
        self.effect_layers.clear();
        self.backdrop_layers.clear();
        self.retained_draws.clear();
        self.next_z = 0;
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn push_retained_draw(&mut self, draw: RetainedDraw) {
        if std::env::var_os("CRANPOSE_RETAINED_DIAG").is_some() {
            eprintln!(
                "[retained] slot={} first={} count={} center=({:.6},{:.6}) rot=({:.9},{:.9}) scale={:.9} bounds=({:.4},{:.4},{:.4},{:.4})",
                draw.slot,
                draw.first_shape,
                draw.shape_count,
                draw.transform.center[0],
                draw.transform.center[1],
                draw.transform.rot[0],
                draw.transform.rot[1],
                draw.transform.scale,
                draw.bounds.x,
                draw.bounds.y,
                draw.bounds.width,
                draw.bounds.height,
            );
        }
        let z_index = self.next_z;
        self.next_z += 1;
        self.retained_draws.push(draw);
        self.draw_ops.push(DrawOp {
            z_index,
            kind: DrawOpKind::Retained(self.retained_draws.len() - 1),
        });
    }

    pub fn intern_brush(&mut self, brush: ResolvedBrush) -> SceneBrush {
        intern_brush_into(&mut self.brushes, brush)
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
            ResolvedBrush::from_brush(brush),
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
        brush: ResolvedBrush,
        shape: Option<RoundedCornerShape>,
        stroke: Option<Stroke>,
        arc: Option<ArcGeometry>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
        motion_context_animated: bool,
    ) {
        let brush = self.intern_brush(brush);
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
            text: render_string_for(&text),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_drop_skips_pooling_when_the_pool_is_held() {
        let scene = CompositorScene::new();
        SCENE_BUFFER_POOL.with(|pool| {
            let _held = pool.borrow_mut();
            let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(scene)));
            assert!(
                dropped.is_ok(),
                "dropping a scene under a held pool borrow must not panic"
            );
        });
    }
}
