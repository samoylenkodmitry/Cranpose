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

fn crop_source_rect(src_size: Size, dst_size: Size, alignment: Alignment) -> Rect {
    if src_size.width <= 0.0
        || src_size.height <= 0.0
        || dst_size.width <= 0.0
        || dst_size.height <= 0.0
    {
        return Rect::from_size(Size::ZERO);
    }

    let src_aspect = src_size.width / src_size.height;
    let dst_aspect = dst_size.width / dst_size.height;

    if (src_aspect - dst_aspect).abs() <= f32::EPSILON {
        return Rect::from_origin_size(crate::modifier::Point::ZERO, src_size);
    }

    if src_aspect > dst_aspect {
        // Source is wider than destination: crop width.
        let crop_width = src_size.height * dst_aspect;
        let x = alignment
            .horizontal
            .align(src_size.width, crop_width)
            .clamp(0.0, (src_size.width - crop_width).max(0.0));
        Rect {
            x,
            y: 0.0,
            width: crop_width,
            height: src_size.height,
        }
    } else {
        // Source is taller than destination: crop height.
        let crop_height = src_size.width / dst_aspect;
        let y = alignment
            .vertical
            .align(src_size.height, crop_height)
            .clamp(0.0, (src_size.height - crop_height).max(0.0));
        Rect {
            x: 0.0,
            y,
            width: src_size.width,
            height: crop_height,
        }
    }
}

fn intersect_rect(a: Rect, b: Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.width).min(b.x + b.width);
    let bottom = (a.y + a.height).min(b.y + b.height);
    let width = right - left;
    let height = bottom - top;
    if width <= 0.0 || height <= 0.0 {
        None
    } else {
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
    }
}

fn map_destination_clip_to_source(
    src_rect: Rect,
    dst_rect: Rect,
    clipped_dst_rect: Rect,
) -> Option<Rect> {
    if src_rect.width <= 0.0
        || src_rect.height <= 0.0
        || dst_rect.width <= 0.0
        || dst_rect.height <= 0.0
        || clipped_dst_rect.width <= 0.0
        || clipped_dst_rect.height <= 0.0
    {
        return None;
    }

    let scale_x = src_rect.width / dst_rect.width;
    let scale_y = src_rect.height / dst_rect.height;

    let src_min_x = src_rect.x;
    let src_min_y = src_rect.y;
    let src_max_x = src_rect.x + src_rect.width;
    let src_max_y = src_rect.y + src_rect.height;

    let raw_left = src_rect.x + (clipped_dst_rect.x - dst_rect.x) * scale_x;
    let raw_top = src_rect.y + (clipped_dst_rect.y - dst_rect.y) * scale_y;
    let raw_right =
        src_rect.x + ((clipped_dst_rect.x + clipped_dst_rect.width) - dst_rect.x) * scale_x;
    let raw_bottom =
        src_rect.y + ((clipped_dst_rect.y + clipped_dst_rect.height) - dst_rect.y) * scale_y;

    let left = raw_left.clamp(src_min_x, src_max_x);
    let top = raw_top.clamp(src_min_y, src_max_y);
    let right = raw_right.clamp(src_min_x, src_max_x);
    let bottom = raw_bottom.clamp(src_min_y, src_max_y);
    let width = right - left;
    let height = bottom - top;

    if width <= 0.0 || height <= 0.0 {
        None
    } else {
        Some(Rect {
            x: left,
            y: top,
            width,
            height,
        })
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

    let image_modifier =
        modifier
            .then(semantics_modifier)
            .draw_behind(move |scope: &mut dyn DrawScope| {
                if draw_alpha <= 0.0 {
                    return;
                }
                let container_size = scope.size();
                if container_size.width <= 0.0 || container_size.height <= 0.0 {
                    return;
                }
                if content_scale == ContentScale::Crop {
                    // For Crop, sample the centered/biased source sub-rect directly.
                    let src_rect = crop_source_rect(intrinsic_dp, container_size, alignment);
                    if src_rect.width <= 0.0 || src_rect.height <= 0.0 {
                        return;
                    }
                    scope.draw_image_src(
                        draw_painter.bitmap().clone(),
                        src_rect,
                        Rect::from_size(container_size),
                        draw_alpha,
                        color_filter,
                    );
                } else {
                    let dst_rect =
                        destination_rect(intrinsic_dp, container_size, alignment, content_scale);
                    if dst_rect.width <= 0.0 || dst_rect.height <= 0.0 {
                        return;
                    }
                    let container_rect = Rect::from_size(container_size);
                    let Some(clipped_dst_rect) = intersect_rect(dst_rect, container_rect) else {
                        return;
                    };
                    let full_src_rect = Rect::from_size(intrinsic_dp);
                    let Some(clipped_src_rect) =
                        map_destination_clip_to_source(full_src_rect, dst_rect, clipped_dst_rect)
                    else {
                        return;
                    };
                    scope.draw_image_src(
                        draw_painter.bitmap().clone(),
                        clipped_src_rect,
                        clipped_dst_rect,
                        draw_alpha,
                        color_filter,
                    );
                }
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

    #[test]
    fn crop_source_rect_is_centered_for_wide_source() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(100.0, 100.0);
        let rect = crop_source_rect(src, dst, Alignment::CENTER);
        assert_eq!(
            rect,
            Rect {
                x: 50.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            }
        );
    }

    #[test]
    fn crop_source_rect_honors_start_alignment() {
        let src = Size::new(200.0, 100.0);
        let dst = Size::new(100.0, 100.0);
        let rect = crop_source_rect(src, dst, Alignment::TOP_START);
        assert_eq!(
            rect,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            }
        );
    }

    fn approx_eq(left: f32, right: f32) {
        assert!((left - right).abs() < 1e-4, "left={left}, right={right}");
    }

    #[test]
    fn intersect_rect_returns_none_when_disjoint() {
        let a = Rect {
            x: 0.0,
            y: 0.0,
            width: 10.0,
            height: 10.0,
        };
        let b = Rect {
            x: 20.0,
            y: 20.0,
            width: 10.0,
            height: 10.0,
        };
        assert_eq!(intersect_rect(a, b), None);
    }

    #[test]
    fn map_destination_clip_to_source_scales_proportionally() {
        let src = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let dst = Rect {
            x: -50.0,
            y: 0.0,
            width: 200.0,
            height: 100.0,
        };
        let clipped_dst = Rect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let mapped = map_destination_clip_to_source(src, dst, clipped_dst).expect("mapped");
        approx_eq(mapped.x, 25.0);
        approx_eq(mapped.y, 0.0);
        approx_eq(mapped.width, 50.0);
        approx_eq(mapped.height, 100.0);
    }

    #[test]
    fn map_destination_clip_to_source_returns_full_source_without_clipping() {
        let src = Rect {
            x: 0.0,
            y: 0.0,
            width: 120.0,
            height: 80.0,
        };
        let dst = Rect {
            x: 10.0,
            y: 5.0,
            width: 60.0,
            height: 40.0,
        };
        let mapped = map_destination_clip_to_source(src, dst, dst).expect("mapped");
        approx_eq(mapped.x, src.x);
        approx_eq(mapped.y, src.y);
        approx_eq(mapped.width, src.width);
        approx_eq(mapped.height, src.height);
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
