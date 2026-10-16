//! Surfaces: a layer the items' own geometry cannot follow, turned, mirrored
//! or asked to composite offscreen, is drawn into pixels of its own first
//! and lands in its parent as one image.

use cranpose_render_common::{graph::ProjectiveTransform, graph_scene::RenderDiagnostics};
use cranpose_ui_graphics::{ImageBitmap, Rect, whole};

use crate::{
    draw::{ClipBounds, Target, TextRaster, draw_item},
    scene::RasterScene,
};

/// The widest and tallest surface, in pixels; a larger layer is cut.
const MAX_SURFACE: f32 = 4096.0;
/// The transparent pixels around a surface's content, so its edges blend
/// out where it is sampled turned.
const MARGIN: f32 = 1.0;
const AXIS_TOLERANCE: f32 = 1.0e-5;

/// Whether `transform` only scales up or down and moves, which the items'
/// own geometry follows: no turn, mirror, skew or perspective.
pub(crate) fn scales_and_moves(transform: &ProjectiveTransform) -> bool {
    let [[a, b, _], [c, d, _], perspective] = transform.matrix();
    let near = |value: f32, target: f32| (value - target).abs() <= AXIS_TOLERANCE;
    near(b, 0.0)
        && near(c, 0.0)
        && a > 0.0
        && d > 0.0
        && near(perspective[0], 0.0)
        && near(perspective[1], 0.0)
        && near(perspective[2], 1.0)
}

/// Where a layer's surface lies: the map from the layer's space to the
/// surface's pixels, the surface's size, and where its pixels land.
pub(crate) struct SurfacePlacement {
    /// Maps the layer's space to the surface's pixels.
    pub to_surface: ProjectiveTransform,
    pub width: u32,
    pub height: u32,
    /// The image's rect: in device pixels, or in the surface's pixels for
    /// a turned surface, which `to_device` then places.
    pub rect: Rect,
    pub to_device: Option<ProjectiveTransform>,
}

impl SurfacePlacement {
    /// The surface of a layer of `bounds` that `transform` places in device
    /// pixels. A surface that only moves lies on the device's pixel grid,
    /// so its pixels land whole; a turned one keeps the device's scale.
    /// `None` for a layer that covers no pixel.
    pub(crate) fn of(bounds: Rect, transform: &ProjectiveTransform) -> Option<Self> {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }
        if scales_and_moves(transform) {
            let device = transform.bounds_for_rect(bounds);
            let x = whole::floor(device.x) - MARGIN;
            let y = whole::floor(device.y) - MARGIN;
            let width = surface_extent(whole::ceil(device.x + device.width) + MARGIN - x)?;
            let height = surface_extent(whole::ceil(device.y + device.height) + MARGIN - y)?;
            return Some(Self {
                to_surface: transform.then(ProjectiveTransform::translation(-x, -y)),
                width,
                height,
                rect: Rect {
                    x,
                    y,
                    width: width as f32,
                    height: height as f32,
                },
                to_device: None,
            });
        }
        let [[a, b, _], [c, d, _], _] = transform.matrix();
        let scale = (a * d - b * c).abs().sqrt();
        if !scale.is_normal() {
            return None;
        }
        let width = surface_extent(whole::ceil(bounds.width * scale) + 2.0 * MARGIN)?;
        let height = surface_extent(whole::ceil(bounds.height * scale) + 2.0 * MARGIN)?;
        let to_surface = ProjectiveTransform::translation(-bounds.x, -bounds.y)
            .then(ProjectiveTransform::uniform_scale(scale))
            .then(ProjectiveTransform::translation(MARGIN, MARGIN));
        let to_device = to_surface.inverse()?.then(*transform);
        Some(Self {
            to_surface,
            width,
            height,
            rect: Rect {
                x: 0.0,
                y: 0.0,
                width: width as f32,
                height: height as f32,
            },
            to_device: Some(to_device),
        })
    }
}

fn surface_extent(pixels: f32) -> Option<u32> {
    (pixels.is_finite() && pixels >= 1.0).then(|| pixels.min(MAX_SURFACE) as u32)
}

/// Draws every item of `items` into transparent pixels of `width` by
/// `height` and returns them as a bitmap with straight alpha.
pub(crate) fn rasterize_surface(
    items: &RasterScene,
    width: u32,
    height: u32,
    diagnostics: &RenderDiagnostics,
    text: &TextRaster,
) -> Option<ImageBitmap> {
    let mut pixels = vec![0u8; width as usize * height as usize * 4];
    let mut target = Target {
        pixels: &mut pixels,
        left: 0,
        top: 0,
        width,
        height,
    };
    let window = ClipBounds {
        min_x: 0,
        min_y: 0,
        max_x: width as i32,
        max_y: height as i32,
    };
    for &item in &items.order {
        if items.item_bounds(item, window).is_some() {
            draw_item(&mut target, items, item, diagnostics, text);
        }
    }
    // Drawn over transparent pixels, the colours came out premultiplied.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = u32::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    ImageBitmap::from_rgba8(width, height, pixels).ok()
}
