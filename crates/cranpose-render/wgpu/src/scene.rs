//! Scene structures for GPU rendering

use crate::surface_requirements::SurfaceRequirementSet;
use cranpose_core::NodeId;
pub use cranpose_render_common::graph_scene::{ClickAction, HitRegion, Scene};
use cranpose_ui::{TextLayoutOptions, TextStyle};
use cranpose_ui_graphics::{
    BlendMode, Brush, Color, ColorFilter, ImageBitmap, Point, Rect, RenderEffect,
    RoundedCornerShape,
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
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
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
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    /// Source sub-region in image-pixel coordinates. `None` means full image.
    pub src_rect: Option<Rect>,
    pub motion_context_animated: bool,
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
            snap_anchor: None,
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
        motion_context_animated: bool,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.images.push(ImageDraw {
            rect,
            local_rect,
            quad,
            snap_anchor: None,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            z_index,
            clip,
            blend_mode,
            src_rect,
            motion_context_animated,
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
    }

    pub fn push_shadow_draw(&mut self, mut draw: ShadowDraw) {
        let z_index = self.next_z;
        self.next_z += 1;
        draw.z_index = z_index;
        self.shadow_draws.push(draw);
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
