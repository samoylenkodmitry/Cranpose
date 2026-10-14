use std::{rc::Rc, sync::Arc};

pub use cranpose_render_common::graph_scene::{HitRegion, Scene};
use cranpose_render_common::{layer_shadow::ShadowRRect, primitive_emit::ShapeDrawParams};
use cranpose_ui::TextStyle;
use cranpose_ui_graphics::{
    ArcGeometry, BlendMode, Brush, Color, ColorFilter, ImageBitmap, ImageSampling, LineGeometry,
    Point, Rect, RoundedCornerShape, Stroke, Trapezoid,
};

use crate::draw::RenderItem;

#[derive(Clone, Debug)]
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
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
}

/// A point moved by `dx`, `dy`.
fn shifted(point: Point, dx: f32, dy: f32) -> Point {
    Point::new(point.x + dx, point.y + dy)
}

impl DrawShape {
    /// The shape moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.translate(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            arc: self.arc.map(|arc| ArcGeometry {
                center: shifted(arc.center, dx, dy),
                ..arc
            }),
            line: self.line.map(|line| {
                line.placed(shifted(line.start, dx, dy), shifted(line.end, dx, dy), 1.0)
            }),
            trapezoid: self.trapezoid.map(|trapezoid| trapezoid.translate(dx, dy)),
            clip: None,
            ..self.clone()
        }
    }

    /// The shape `params` resolved.
    pub(crate) fn of_params(params: ShapeDrawParams) -> Self {
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
            clip: params.clip,
            blend_mode: params.blend_mode,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct TextDraw {
    pub rect: Rect,
    pub snap_anchor: Option<Point>,
    pub text: Rc<cranpose_ui::text::AnnotatedString>,
    pub color: Color,
    pub text_style: Arc<TextStyle>,
    pub font_size: f32,
    pub scale: f32,
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
    pub clip: Option<Rect>,
    pub blend_mode: BlendMode,
    pub src_rect: Option<Rect>,
}

/// One pass of a layer's elevation shadow: Skia's round rect shadow in
/// device pixels, its color and the clip it is cut to.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ShadowDraw {
    pub shadow: ShadowRRect,
    pub snap_anchor: Option<Point>,
    pub color: Color,
    pub clip: Option<Rect>,
}

impl TextDraw {
    /// The text moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.translate(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            clip: None,
            ..self.clone()
        }
    }
}

impl ImageDraw {
    /// The image moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            rect: self.rect.translate(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            clip: None,
            ..self.clone()
        }
    }
}

impl ShadowDraw {
    /// The shadow moved by `dx`, `dy`, with no clip.
    pub(crate) fn translated(&self, dx: f32, dy: f32) -> Self {
        Self {
            shadow: self.shadow.translated(dx, dy),
            snap_anchor: self.snap_anchor.map(|anchor| shifted(anchor, dx, dy)),
            clip: None,
            ..*self
        }
    }
}

pub(crate) struct RasterScene {
    pub shapes: Vec<DrawShape>,
    pub images: Vec<ImageDraw>,
    pub texts: Vec<TextDraw>,
    pub shadows: Vec<ShadowDraw>,
    /// Every item in the order it was pushed, which is the order it draws in.
    pub order: Vec<RenderItem>,
}

impl RasterScene {
    pub fn new() -> Self {
        Self {
            shapes: Vec::new(),
            images: Vec::new(),
            texts: Vec::new(),
            shadows: Vec::new(),
            order: Vec::new(),
        }
    }

    /// Empties the scene and keeps its storage.
    pub(crate) fn clear(&mut self) {
        self.shapes.clear();
        self.images.clear();
        self.texts.clear();
        self.shadows.clear();
        self.order.clear();
    }

    /// Queues `shape` to draw over everything queued before.
    pub(crate) fn push_draw_shape(&mut self, shape: DrawShape) {
        self.order.push(RenderItem::Shape(self.shapes.len()));
        self.shapes.push(shape);
    }

    pub fn push_shape(
        &mut self,
        rect: Rect,
        brush: Brush,
        shape: Option<RoundedCornerShape>,
        clip: Option<Rect>,
        blend_mode: BlendMode,
    ) {
        self.push_draw_shape(DrawShape {
            rect,
            snap_anchor: None,
            snap_to_pixel_grid: false,
            brush,
            shape,
            stroke: None,
            arc: None,
            line: None,
            trapezoid: None,
            clip,
            blend_mode,
        });
    }

    pub fn push_shadow(&mut self, shadow: ShadowRRect, color: Color, clip: Option<Rect>) {
        self.order.push(RenderItem::Shadow(self.shadows.len()));
        self.shadows.push(ShadowDraw {
            shadow,
            snap_anchor: None,
            color,
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
        self.push_draw_shape(DrawShape {
            rect,
            snap_anchor: None,
            snap_to_pixel_grid: true,
            brush,
            shape,
            stroke: None,
            arc: None,
            line: None,
            trapezoid: None,
            clip,
            blend_mode,
        });
    }

    /// Queues a shape an emitted primitive resolved: a rect, rounded rect,
    /// stroke, arc band, line segment or slice of a path fill.
    pub fn push_shape_params(&mut self, params: ShapeDrawParams) {
        self.push_draw_shape(DrawShape::of_params(params));
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
        self.order.push(RenderItem::Image(self.images.len()));
        self.images.push(ImageDraw {
            rect,
            snap_anchor: None,
            image,
            alpha: alpha.clamp(0.0, 1.0),
            color_filter,
            sampling,
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
        text_style: Arc<TextStyle>,
        font_size: f32,
        scale: f32,
        clip: Option<Rect>,
    ) {
        self.order.push(RenderItem::Text(self.texts.len()));
        self.texts.push(TextDraw {
            rect,
            snap_anchor: None,
            text,
            color,
            text_style,
            font_size,
            scale,
            clip,
        });
    }
}

impl Default for RasterScene {
    fn default() -> Self {
        Self::new()
    }
}
