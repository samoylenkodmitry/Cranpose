//! The masks vector paths fill, rasterized once and kept by their path,
//! placement and brush.

use crate::{Brush, ImageBitmap, Point, Rect};

const VECTOR_PATH_MASK_CACHE_ENTRIES: usize = 96;
const VECTOR_PATH_MASK_CACHE_BYTES: usize = 8 * 1024 * 1024;

pub(crate) struct VectorPathMaskCache {
    entries: Vec<(u64, ImageBitmap)>,
    bytes: usize,
}

impl VectorPathMaskCache {
    pub(crate) const fn new() -> Self {
        Self {
            entries: Vec::new(),
            bytes: 0,
        }
    }

    fn get(&mut self, key: u64) -> Option<ImageBitmap> {
        let index = self.entries.iter().position(|(seen, _)| *seen == key)?;
        let entry = self.entries.remove(index);
        let image = entry.1.clone();
        self.entries.push(entry);
        Some(image)
    }

    fn put(&mut self, key: u64, image: ImageBitmap) {
        let bytes = image.pixels().len();
        if bytes > VECTOR_PATH_MASK_CACHE_BYTES {
            return;
        }
        self.bytes += bytes;
        self.entries.push((key, image));
        while self.entries.len() > VECTOR_PATH_MASK_CACHE_ENTRIES
            || self.bytes > VECTOR_PATH_MASK_CACHE_BYTES
        {
            let (_, dropped) = self.entries.remove(0);
            self.bytes = self.bytes.saturating_sub(dropped.pixels().len());
        }
    }
}

/// The key of `path`'s raster: a gradient's colors depend on the rect it
/// resolves against, a solid color's do not.
/// Which side of a path's edge a coverage mask covers: its fill, or the
/// rest of the mask's rect, as a clip's mask erases it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathSide {
    Inside,
    Outside,
}

pub(crate) fn vector_path_mask_key(
    path: &crate::VectorPath,
    origin: Point,
    mask_size: (usize, usize),
    brush: &Brush,
    brush_rect: Rect,
    side: PathSide,
) -> u64 {
    use std::hash::Hasher;
    let mut hasher = crate::fx_hash::FxHasher::default();
    hasher.write_u8(path.fill_rule() as u8);
    hasher.write_u8(side as u8);
    hasher.write_u32(origin.x.to_bits());
    hasher.write_u32(origin.y.to_bits());
    hasher.write_usize(mask_size.0);
    hasher.write_usize(mask_size.1);
    crate::render_hash::hash_brush(brush, &mut hasher);
    if !matches!(brush, Brush::Solid(_)) {
        for value in [
            brush_rect.x,
            brush_rect.y,
            brush_rect.width,
            brush_rect.height,
        ] {
            hasher.write_u32(value.to_bits());
        }
    }
    for subpath in path.subpaths() {
        hasher.write_usize(subpath.len());
        for point in subpath {
            hasher.write_u32(point.x.to_bits());
            hasher.write_u32(point.y.to_bits());
        }
    }
    hasher.finish()
}

/// How many raster pixels a vector path's fill gets per logical pixel along
/// each axis.
pub(crate) const VECTOR_PATH_SUPERSAMPLE: f32 = 2.0;

/// `path`'s fill, or with [`PathSide::Outside`] the rest of the mask, over
/// `size` raster pixels from `origin`: one alpha byte of a solid color per
/// pixel, or for a gradient the color it paints at each pixel's center with
/// the pixel's coverage.
pub(crate) fn rasterize_vector_path(
    path: &crate::VectorPath,
    brush: &Brush,
    brush_rect: Rect,
    origin: Point,
    (width, height): (usize, usize),
    side: PathSide,
) -> Option<ImageBitmap> {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
    let covered = |opacity: f32| {
        let mut mask = path.coverage_mask(width, height, origin, VECTOR_PATH_SUPERSAMPLE, 1.0);
        for value in &mut mask {
            let share = match side {
                PathSide::Inside => *value,
                PathSide::Outside => u8::MAX - *value,
            };
            *value = (f32::from(share) * opacity + 0.5) as u8;
        }
        mask
    };
    if let Brush::Solid(color) = brush {
        let mask = match side {
            PathSide::Inside => path.coverage_mask(
                width,
                height,
                origin,
                VECTOR_PATH_SUPERSAMPLE,
                color.3.clamp(0.0, 1.0),
            ),
            PathSide::Outside => covered(color.3.clamp(0.0, 1.0)),
        };
        let rgb = [byte(color.0), byte(color.1), byte(color.2)];
        return ImageBitmap::from_alpha8(width as u32, height as u32, rgb, mask).ok();
    }
    let coverage = covered(1.0);
    let mut pixels = Vec::with_capacity(coverage.len() * 4);
    for (row, line) in coverage.chunks_exact(width).enumerate() {
        let y = origin.y + (row as f32 + 0.5) / VECTOR_PATH_SUPERSAMPLE;
        for (column, &covered) in line.iter().enumerate() {
            if covered == 0 {
                pixels.extend_from_slice(&[0; 4]);
                continue;
            }
            let x = origin.x + (column as f32 + 0.5) / VECTOR_PATH_SUPERSAMPLE;
            let color = brush.color_at(brush_rect, Point::new(x, y));
            let alpha = (color.3.clamp(0.0, 1.0) * f32::from(covered) + 0.5) as u8;
            pixels.extend_from_slice(&[byte(color.0), byte(color.1), byte(color.2), alpha]);
        }
    }
    ImageBitmap::from_rgba8(width as u32, height as u32, pixels).ok()
}

pub(crate) fn vector_path_mask_cache_get(key: u64) -> Option<ImageBitmap> {
    crate::thread_state::with(|state| state.vector_path_masks.borrow_mut().get(key))
}

pub(crate) fn vector_path_mask_cache_put(key: u64, image: ImageBitmap) {
    crate::thread_state::with(|state| state.vector_path_masks.borrow_mut().put(key, image));
}

#[cfg(test)]
#[path = "tests/vector_path_mask_tests.rs"]
mod tests;
