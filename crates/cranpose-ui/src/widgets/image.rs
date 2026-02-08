//! Image composable and painter primitives.

#![allow(non_snake_case)]
#![allow(clippy::too_many_arguments)] // API matches Jetpack Compose Image signature.

use crate::composable;
use crate::layout::core::{Alignment, Measurable};
use crate::modifier::{Modifier, Rect, Size};
use crate::widgets::Layout;
use cranpose_core::NodeId;
use cranpose_ui_graphics::{ColorFilter, DrawScope, ImageBitmap};
use cranpose_ui_layout::{Constraints, MeasurePolicy, MeasureResult};

pub const DEFAULT_ALPHA: f32 = 1.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContentScale {
    Fit,
    Crop,
    FillBounds,
    FillWidth,
    FillHeight,
    Inside,
    None,
}

impl ContentScale {
    pub fn scaled_size(self, src_size: Size, dst_size: Size) -> Size {
        if src_size.width <= 0.0
            || src_size.height <= 0.0
            || dst_size.width <= 0.0
            || dst_size.height <= 0.0
        {
            return Size::ZERO;
        }

        let scale_x = dst_size.width / src_size.width;
        let scale_y = dst_size.height / src_size.height;

        let (factor_x, factor_y) = match self {
            Self::Fit => {
                let factor = scale_x.min(scale_y);
                (factor, factor)
            }
            Self::Crop => {
                let factor = scale_x.max(scale_y);
                (factor, factor)
            }
            Self::FillBounds => (scale_x, scale_y),
            Self::FillWidth => (scale_x, scale_x),
            Self::FillHeight => (scale_y, scale_y),
            Self::Inside => {
                if src_size.width <= dst_size.width && src_size.height <= dst_size.height {
                    (1.0, 1.0)
                } else {
                    let factor = scale_x.min(scale_y);
                    (factor, factor)
                }
            }
            Self::None => (1.0, 1.0),
        };

        Size {
            width: src_size.width * factor_x,
            height: src_size.height * factor_y,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Painter {
    bitmap: ImageBitmap,
}

impl Painter {
    pub fn from_bitmap(bitmap: ImageBitmap) -> Self {
        Self { bitmap }
    }

    pub fn intrinsic_size(&self) -> Size {
        self.bitmap.intrinsic_size()
    }

    pub fn bitmap(&self) -> &ImageBitmap {
        &self.bitmap
    }
}

impl From<ImageBitmap> for Painter {
    fn from(value: ImageBitmap) -> Self {
        Self::from_bitmap(value)
    }
}

pub fn BitmapPainter(bitmap: ImageBitmap) -> Painter {
    Painter::from_bitmap(bitmap)
}

/// Measure policy for Image that preserves aspect ratio when constraints
/// force the image smaller than its intrinsic size.
///
/// Unlike [`LeafMeasurePolicy`] which clamps width and height independently,
/// this scales both dimensions by the same factor so the image is never
/// distorted by layout constraints.
#[derive(Clone, Debug, PartialEq)]
struct ImageMeasurePolicy {
    intrinsic_size: Size,
}

impl MeasurePolicy for ImageMeasurePolicy {
    fn measure(
        &self,
        _measurables: &[Box<dyn Measurable>],
        constraints: Constraints,
    ) -> MeasureResult {
        let iw = self.intrinsic_size.width;
        let ih = self.intrinsic_size.height;

        if iw <= 0.0 || ih <= 0.0 {
            let (w, h) = constraints.constrain(0.0, 0.0);
            return MeasureResult::new(
                Size {
                    width: w,
                    height: h,
                },
                vec![],
            );
        }

        // Clamp each axis to its constraint range.
        let cw = iw.clamp(constraints.min_width, constraints.max_width);
        let ch = ih.clamp(constraints.min_height, constraints.max_height);

        // If either axis had to shrink, scale both by the smaller factor
        // so the aspect ratio is preserved.
        let scale_x = cw / iw;
        let scale_y = ch / ih;

        let (width, height) = if scale_x < 1.0 || scale_y < 1.0 {
            let factor = scale_x.min(scale_y);
            let w = (iw * factor).clamp(constraints.min_width, constraints.max_width);
            let h = (ih * factor).clamp(constraints.min_height, constraints.max_height);
            (w, h)
        } else {
            (cw, ch)
        };

        MeasureResult::new(Size { width, height }, vec![])
    }

    fn min_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn max_intrinsic_width(&self, _measurables: &[Box<dyn Measurable>], _height: f32) -> f32 {
        self.intrinsic_size.width
    }

    fn min_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.intrinsic_size.height
    }

    fn max_intrinsic_height(&self, _measurables: &[Box<dyn Measurable>], _width: f32) -> f32 {
        self.intrinsic_size.height
    }
}

fn destination_rect(
    src_size: Size,
    dst_size: Size,
    alignment: Alignment,
    content_scale: ContentScale,
) -> Rect {
    let draw_size = content_scale.scaled_size(src_size, dst_size);
    let offset_x = alignment.horizontal.align(dst_size.width, draw_size.width);
    let offset_y = alignment.vertical.align(dst_size.height, draw_size.height);
    Rect {
        x: offset_x,
        y: offset_y,
        width: draw_size.width,
        height: draw_size.height,
    }
}

#[composable]
pub fn Image<P>(
    painter: P,
    content_description: Option<String>,
    modifier: Modifier,
    alignment: Alignment,
    content_scale: ContentScale,
    alpha: f32,
    color_filter: Option<ColorFilter>,
) -> NodeId
where
    P: Into<Painter> + Clone + PartialEq + 'static,
{
    let painter = painter.into();
    // Treat bitmap pixels as dp (1 image-pixel = 1 dp).  This keeps images
    // at the same visual size on every screen density.  On hi-dpi screens the
    // bitmap is upscaled by the renderer, which is the correct behaviour for
    // web-loaded and most application images.
    let intrinsic_dp = painter.intrinsic_size();
    let draw_alpha = alpha.clamp(0.0, 1.0);
    let draw_painter = painter.clone();

    let semantics_modifier = if let Some(description) = content_description {
        Modifier::empty().semantics(move |config| {
            config.content_description = Some(description.clone());
        })
    } else {
        Modifier::empty()
    };

    let image_modifier = modifier
        .then(semantics_modifier)
        .clip_to_bounds()
        .draw_behind(move |scope: &mut dyn DrawScope| {
            if draw_alpha <= 0.0 {
                return;
            }
            let container_size = scope.size();
            let rect = destination_rect(intrinsic_dp, container_size, alignment, content_scale);
            if rect.width <= 0.0 || rect.height <= 0.0 {
                return;
            }
            scope.draw_image_at(
                rect,
                draw_painter.bitmap().clone(),
                draw_alpha,
                color_filter,
            );
        });

    Layout(
        image_modifier,
        ImageMeasurePolicy {
            intrinsic_size: intrinsic_dp,
        },
        || {},
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::core::Alignment;

    fn sample_bitmap() -> ImageBitmap {
        ImageBitmap::from_rgba8(4, 2, vec![255; 4 * 2 * 4]).expect("bitmap")
    }

    #[test]
    fn painter_reports_intrinsic_size_and_bitmap() {
        let bitmap = sample_bitmap();
        let painter = BitmapPainter(bitmap.clone());
        assert_eq!(painter.intrinsic_size(), Size::new(4.0, 2.0));
        assert_eq!(painter.bitmap(), &bitmap);
    }

    #[test]
    fn fit_keeps_aspect_ratio() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(300.0, 300.0);
        let result = ContentScale::Fit.scaled_size(src, dst);
        assert_eq!(result, Size::new(300.0, 150.0));
    }

    #[test]
    fn crop_fills_bounds() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(300.0, 300.0);
        let result = ContentScale::Crop.scaled_size(src, dst);
        assert_eq!(result, Size::new(600.0, 300.0));
    }

    #[test]
    fn destination_rect_aligns_center() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(300.0, 300.0);
        let rect = destination_rect(src, dst, Alignment::CENTER, ContentScale::Fit);
        assert_eq!(
            rect,
            Rect {
                x: 0.0,
                y: 75.0,
                width: 300.0,
                height: 150.0,
            }
        );
    }

    // --- ImageMeasurePolicy tests ---

    fn measure_image(intrinsic: Size, constraints: Constraints) -> Size {
        let policy = ImageMeasurePolicy {
            intrinsic_size: intrinsic,
        };
        policy.measure(&[], constraints).size
    }

    #[test]
    fn image_measure_unconstrained() {
        let size = measure_image(
            Size::new(800.0, 600.0),
            Constraints::loose(f32::INFINITY, f32::INFINITY),
        );
        assert_eq!(size, Size::new(800.0, 600.0));
    }

    #[test]
    fn image_measure_width_constrained_preserves_aspect_ratio() {
        // 800×600 constrained to max_width=400 → should scale to 400×300
        let size = measure_image(
            Size::new(800.0, 600.0),
            Constraints::loose(400.0, f32::INFINITY),
        );
        assert_eq!(size, Size::new(400.0, 300.0));
    }

    #[test]
    fn image_measure_height_constrained_preserves_aspect_ratio() {
        // 800×600 constrained to max_height=300 → should scale to 400×300
        let size = measure_image(
            Size::new(800.0, 600.0),
            Constraints::loose(f32::INFINITY, 300.0),
        );
        assert_eq!(size, Size::new(400.0, 300.0));
    }

    #[test]
    fn image_measure_both_constrained_uses_smaller_factor() {
        // 800×600 constrained to 200×400 → width is the bottleneck (0.25)
        // scaled: 200×150
        let size = measure_image(Size::new(800.0, 600.0), Constraints::loose(200.0, 400.0));
        assert_eq!(size, Size::new(200.0, 150.0));
    }

    #[test]
    fn image_measure_fits_within_constraints() {
        // 200×100 in a 400×400 container → stays at intrinsic size
        let size = measure_image(Size::new(200.0, 100.0), Constraints::loose(400.0, 400.0));
        assert_eq!(size, Size::new(200.0, 100.0));
    }

    #[test]
    fn image_measure_zero_intrinsic() {
        let size = measure_image(Size::ZERO, Constraints::loose(400.0, 400.0));
        assert_eq!(size, Size::new(0.0, 0.0));
    }
}
