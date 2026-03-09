//! Scene structures for GPU rendering

use cranpose_core::NodeId;
pub use cranpose_render_common::graph_scene::{ClickAction, HitRegion, Scene};
use cranpose_ui::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, ImageBitmap, Rect, RenderEffect, RoundedCornerShape,
};
use std::rc::Rc;

#[derive(Clone)]
pub(crate) struct DrawShape {
    pub rect: Rect,
    pub local_rect: Rect,
    pub quad: [[f32; 2]; 4],
    pub brush: Brush,
    pub shape: Option<RoundedCornerShape>,
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
}

#[derive(Clone)]
pub(crate) struct TextDraw {
    pub node_id: NodeId,
    pub rect: Rect,
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
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    /// Source sub-region in image-pixel coordinates. `None` means full image.
    pub src_rect: Option<Rect>,
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

/// A subtree that should be rendered offscreen and processed by a RenderEffect.
#[derive(Clone)]
pub(crate) struct EffectLayer {
    pub rect: Rect,
    pub clip: Option<Rect>,
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
}

/// A backdrop effect applied to already-rendered content behind a node.
#[derive(Clone)]
pub(crate) struct BackdropLayer {
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
    pub effect_layers: Vec<EffectLayer>,
    pub backdrop_layers: Vec<BackdropLayer>,
    pub next_z: usize,
}

impl CompositorScene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            images: Vec::new(),
            texts: Vec::new(),
            shadow_draws: Vec::new(),
            effect_layers: Vec::new(),
            backdrop_layers: Vec::new(),
            next_z: 0,
        }
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
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.shapes.push(DrawShape {
            rect,
            local_rect,
            quad,
            brush,
            shape,
            z_index,
            clip,
            blend_mode,
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
        clip: Option<Rect>,
        src_rect: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.images.push(ImageDraw {
            rect,
            local_rect,
            quad,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            z_index,
            clip,
            blend_mode,
            src_rect,
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
        self.texts.push(TextDraw {
            node_id,
            rect,
            text,
            color,
            text_style,
            font_size,
            scale,
            layout_options,
            z_index,
            clip,
        });
    }

    pub fn push_shadow_draw(&mut self, mut draw: ShadowDraw) {
        let z_index = self.next_z;
        self.next_z += 1;
        draw.z_index = z_index;
        self.shadow_draws.push(draw);
    }
}

impl Default for CompositorScene {
    fn default() -> Self {
        Self::new()
    }
}

fn rect_to_quad(rect: Rect) -> [[f32; 2]; 4] {
    [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ]
}
