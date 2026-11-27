use std::rc::Rc;

use smallvec::SmallVec;

use super::BatchMember;
use crate::{gpu_stats::FrameStats, idle_pool::IdlePool, offscreen::OffscreenTarget};

const ATLAS_SIZE_STEP: u32 = 16;

#[derive(Clone, Copy)]
pub(super) struct AtlasPlacement {
    pub(super) atlas: usize,
    pub(super) x: u32,
    pub(super) y: u32,
}

pub(super) struct Shelf {
    y: u32,
    height: u32,
    x: u32,
}

#[derive(Default)]
pub(super) struct Atlas {
    width: u32,
    height: u32,
    shelves: Vec<Shelf>,
}

impl Atlas {
    pub(super) fn padded_size(&self, limit: u32) -> (u32, u32) {
        (
            padded_dimension(self.width, limit),
            padded_dimension(self.height, limit),
        )
    }
}

// The most atlas textures kept between frames.
const MAX_KEPT_ATLASES: usize = 4;

// The side of the members' square, in shelf widths, past which a shelf
// width is chosen again: members that would pack more than four times taller
// than wide.
const SHELF_WIDTH_REACH: u32 = 2;

// The textures of the atlases the layer surfaces a frame draws again go
// into. A render pass clears and stores its whole attachment, so an atlas
// takes a texture of its own padded size: one texture sized to the busiest
// recent frame drew the gauntlet's tier 10 frames at the Mate 20 X's size
// into 2.37 times their members' pixels. The textures of the sizes recent
// frames used stay, so frames whose members come and go take no new ones.
// The GL memory of a process stays at its peak, so the kept textures hold
// at most `kept_weight` of the largest of them. A size no frame used for
// `crate::idle_pool::IDLE_FRAMES` leaves. Members pack into one width while
// it holds them, so frames that differ in members differ in height.
pub(crate) struct SurfaceAtlases {
    format: wgpu::TextureFormat,
    shelf_width: u32,
    idle: IdlePool<Rc<OffscreenTarget>>,
    in_use: SmallVec<[Rc<OffscreenTarget>; MAX_KEPT_ATLASES]>,
}

impl SurfaceAtlases {
    pub(crate) fn new(format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            shelf_width: 0,
            idle: IdlePool::default(),
            in_use: SmallVec::new(),
        }
    }

    // The width the members of `group` pack into: the width earlier frames
    // packed into, chosen again once a member is wider or the side of the
    // members' square passes `SHELF_WIDTH_REACH` times it.
    pub(super) fn shelf_width(&mut self, group: &[BatchMember], limit: u32) -> u32 {
        let square = square_width(group, limit);
        let widest = group
            .iter()
            .map(|member| member.plan.width)
            .max()
            .unwrap_or(0);
        if self.shelf_width < widest || square > SHELF_WIDTH_REACH * self.shelf_width {
            self.shelf_width = padded_dimension(square.max(widest), limit);
        }
        self.shelf_width
    }

    // A texture for `atlas` of its padded size, no side past `limit`: a kept
    // one of that size, else the smallest kept one that holds it when the
    // kept textures leave no room for one more, else a new one. An atlas of
    // more than one shelf takes the whole shelf width, so a member one pixel
    // narrower that fits one more into a shelf changes no size.
    pub(super) fn acquire(
        &mut self,
        device: &wgpu::Device,
        atlas: &Atlas,
        limit: u32,
        stats: &FrameStats,
    ) -> Rc<OffscreenTarget> {
        let (mut width, height) = atlas.padded_size(limit);
        if atlas.shelves.len() > 1 {
            width = width.max(self.shelf_width);
        }
        let kept = self
            .idle
            .take(|kept| (kept.width, kept.height) == (width, height))
            .or_else(|| self.take_holder((width, height)));
        let is_new = kept.is_none();
        let texture = kept.unwrap_or_else(|| {
            Rc::new(OffscreenTarget::new_labeled(
                device,
                self.format,
                width,
                height,
                "Layer Surface Atlas",
            ))
        });
        stats.record_offscreen_acquire(texture.width, texture.height, self.format, is_new);
        self.in_use.push(Rc::clone(&texture));
        texture
    }

    fn take_holder(&mut self, (width, height): (u32, u32)) -> Option<Rc<OffscreenTarget>> {
        let areas = || {
            self.idle
                .iter()
                .chain(&self.in_use)
                .map(|texture| texture_area(texture))
        };
        let held: u64 = areas().sum();
        let largest = areas().max().unwrap_or(0);
        let own = u64::from(width) * u64::from(height);
        let room = self.idle.len() + self.in_use.len() < MAX_KEPT_ATLASES
            && held + own <= kept_weight(largest.max(own));
        if room {
            return None;
        }
        let holder = self
            .idle
            .iter()
            .filter(|kept| kept.width >= width && kept.height >= height)
            .map(|kept| (kept.width, kept.height))
            .min_by_key(|&size| u64::from(size.0) * u64::from(size.1))?;
        self.idle.take(|kept| (kept.width, kept.height) == holder)
    }

    // Takes back the textures this frame drew into, once its commands are
    // submitted, keeping the most recently used within the count and the
    // weight; a texture something else still holds stays with it.
    pub(crate) fn end_frame(&mut self) {
        let largest = self
            .idle
            .iter()
            .chain(&self.in_use)
            .map(|texture| texture_area(texture))
            .max()
            .unwrap_or(0);
        for texture in self.in_use.drain(..) {
            if Rc::strong_count(&texture) == 1 {
                self.idle
                    .put(texture, MAX_KEPT_ATLASES, kept_weight(largest), |kept| {
                        texture_area(kept)
                    });
            }
        }
        self.idle.end_frame();
    }

    pub(crate) fn len(&self) -> usize {
        self.idle.len()
    }

    pub(crate) fn bytes(&self) -> u64 {
        let bytes_per_pixel = crate::frame_graph::texture_format_bytes_per_pixel(self.format);
        self.idle
            .iter()
            .map(|texture| texture_area(texture) * bytes_per_pixel)
            .sum()
    }
}

// The side of the square the members of `group` would fill.
pub(super) fn square_width(group: &[BatchMember], limit: u32) -> u32 {
    let area: u64 = group
        .iter()
        .map(|member| u64::from(member.plan.width) * u64::from(member.plan.height))
        .sum();
    u32::try_from(area.isqrt()).unwrap_or(limit)
}

fn texture_area(texture: &OffscreenTarget) -> u64 {
    u64::from(texture.width) * u64::from(texture.height)
}

// The pixels the kept atlas textures hold together at most, when the
// largest of them holds `largest`: one and a half times as many.
fn kept_weight(largest: u64) -> u64 {
    largest + largest / 2
}

pub(super) fn padded_dimension(value: u32, limit: u32) -> u32 {
    let step = (value.max(ATLAS_SIZE_STEP).next_power_of_two() / 8).max(ATLAS_SIZE_STEP);
    value.max(1).div_ceil(step).saturating_mul(step).min(limit)
}

/// Shelf packing of regions edge to edge into as few atlases as the
/// dimension limit allows. Nothing separates neighbours: every reader of a
/// region holds its samples to the region's own texel centers, so a
/// neighbour's texels are never read and an atlas needs no clearing.
pub(super) struct AtlasPacker {
    limit: u32,
    shelf_width: u32,
    pub(super) atlases: Vec<Atlas>,
}

impl AtlasPacker {
    pub(super) fn new(limit: u32) -> Self {
        Self {
            limit,
            shelf_width: limit,
            atlases: Vec::new(),
        }
    }

    /// A packer whose shelves end at `width`, or at the limit when that is
    /// narrower, so its atlases grow down rather than out to the limit.
    pub(super) fn with_shelf_width(self, width: u32) -> Self {
        Self {
            shelf_width: width.clamp(1, self.limit),
            ..self
        }
    }

    pub(super) fn place(&mut self, width: u32, height: u32) -> Option<AtlasPlacement> {
        if width > self.limit || height > self.limit {
            return None;
        }
        for (atlas_index, atlas) in self.atlases.iter_mut().enumerate() {
            for shelf in &mut atlas.shelves {
                if shelf.height >= height && shelf.x + width <= self.shelf_width.max(width) {
                    let placement = AtlasPlacement {
                        atlas: atlas_index,
                        x: shelf.x,
                        y: shelf.y,
                    };
                    shelf.x += width;
                    atlas.width = atlas.width.max(shelf.x);
                    return Some(placement);
                }
            }
            if atlas.height + height <= self.limit {
                let placement = AtlasPlacement {
                    atlas: atlas_index,
                    x: 0,
                    y: atlas.height,
                };
                atlas.shelves.push(Shelf {
                    y: atlas.height,
                    height,
                    x: width,
                });
                atlas.height += height;
                atlas.width = atlas.width.max(width);
                return Some(placement);
            }
        }
        self.atlases.push(Atlas {
            width,
            height,
            shelves: vec![Shelf {
                y: 0,
                height,
                x: width,
            }],
        });
        Some(AtlasPlacement {
            atlas: self.atlases.len() - 1,
            x: 0,
            y: 0,
        })
    }
}

#[cfg(test)]
#[path = "tests/frame_atlas_padding_tests.rs"]
mod atlas_padding_tests;
