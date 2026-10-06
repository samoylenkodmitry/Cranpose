use std::rc::Rc;

pub use cranpose_render_common::graph_scene::{HitRegion, Scene};
use cranpose_render_common::{layer_shadow::ShadowRRect, primitive_emit::ShapeDrawParams};
use cranpose_ui::TextStyle;
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, ImageBitmap, ImageSampling, LineGeometry,
    Point, Rect, RoundedCornerShape, Stroke, Trapezoid,
};

#[derive(Clone)]
pub(crate) struct DrawShape {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub snap_to_pixel_grid: bool,
    pub brush: Brush,
    pub shape: Option<RoundedCornerShape>,
    pub stroke: Option<Stroke>,
    pub arc: Option<ArcGeometry>,
    pub line: Option<LineGeometry>,
    pub trapezoid: Option<Trapezoid>,
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
}

impl DrawShape {
    /// The shape `params` resolved, drawn `z_index`-th.
    pub(crate) fn of_params(params: ShapeDrawParams, z_index: usize) -> Self {
        Self {
            rect: params.rect,
            snap_anchor: None,
            snap_to_pixel_grid: false,
            brush: params.brush.into_brush(),
            shape: params.shape,
            stroke: params.stroke,
            arc: params.arc,
            line: params.line,
            trapezoid: params.trapezoid,
            z_index,
            clip: params.clip,
            blend_mode: params.blend_mode,
        }
    }
}

#[derive(Clone)]
pub(crate) struct TextDraw {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub text: Rc<cranpose_ui::text::AnnotatedString>,
    pub color: Color,
    pub text_style: TextStyle,
    pub font_size: f32,
    pub scale: f32,
    pub z_index: usize,
    pub clip: Option<Rect>,
}

#[derive(Clone)]
pub(crate) struct ImageDraw {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub image: ImageBitmap,
    pub alpha: f32,
    pub color_filter: Option<ColorFilter>,
    pub sampling: ImageSampling,
    pub z_index: usize,
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub src_rect: Option<Rect>,
}

/// One pass of a layer's elevation shadow: Skia's round rect shadow in
/// device pixels, its color and the clip it is cut to.
#[derive(Clone, Copy)]
pub(crate) struct ShadowDraw {
    pub shadow: ShadowRRect,
    pub snap_anchor: Option<Point>,
    pub color: Color,
    pub z_index: usize,
    pub clip: Option<Rect>,
}

pub(crate) struct RasterScene {
    pub shapes: Vec<DrawShape>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadows: Vec<ShadowDraw>,
    pub next_z: usize,
}

impl RasterScene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            images: Vec::new(),
            texts: Vec::new(),
            shadows: Vec::new(),
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
        let z_index = self.next_z;
        self.next_z += 1;
        self.shapes.push(DrawShape {
            rect,
            snap_anchor: None,
            snap_to_pixel_grid: false,
            brush,
            shape,
            stroke: None,
            arc: None,
            line: None,
            trapezoid: None,
            z_index,
            clip,
            blend_mode,
        });
    }

    pub fn push_shadow(&mut self, shadow: ShadowRRect, color: Color, clip: Option<Rect>) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.shadows.push(ShadowDraw {
            shadow,
            snap_anchor: None,
            color,
            z_index,
            clip,
        });
    }

    pub fn push_pixel_snapped_shape(
        &mut self,
        rect: Rect,
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.shapes.push(DrawShape {
            rect,
            snap_anchor: None,
            snap_to_pixel_grid: true,
            brush,
            shape,
            stroke: None,
            arc: None,
            line: None,
            trapezoid: None,
            z_index,
            clip,
            blend_mode,
        });
    }

    /// Queues a shape an emitted primitive resolved: a rect, rounded rect,
    /// stroke, arc band, line segment or slice of a path fill.
    pub fn push_shape_params(&mut self, params: ShapeDrawParams) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.shapes.push(DrawShape::of_params(params, z_index));
    }

    #[expect(clippy::too_many_arguments)]
    pub fn push_image_with_geometry(
        &mut self,
        rect: Rect,
        _local_rect: Rect,
        _quad: [[f32; 2]; 4],
        image: ImageBitmap,
        alpha: f32,
        color_filter: Option<ColorFilter>,
        sampling: ImageSampling,
        clip: Option<Rect>,
        src_rect: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.images.push(ImageDraw {
            rect,
            snap_anchor: None,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            sampling,
            z_index,
            clip,
            blend_mode,
            src_rect,
        });
    }

    #[expect(clippy::too_many_arguments)]
    pub fn push_text(
        &mut self,
        rect: Rect,
        text: Rc<cranpose_ui::text::AnnotatedString>,
        color: Color,
        text_style: TextStyle,
        font_size: f32,
        scale: f32,
        clip: Option<Rect>,
    ) {
        let z_index = self.next_z;
        self.next_z += 1;
        self.texts.push(TextDraw {
            rect,
            snap_anchor: None,
            text,
            color,
            text_style,
            font_size,
            scale,
            z_index,
            clip,
        });
    }
}

impl Default for RasterScene {
    fn default() -> Self {
        Self::new()
    }
}
