use std::rc::Rc;

use smallvec::SmallVec;

use super::BatchMember;
use crate::{gpu_stats::FrameStats, offscreen::OffscreenTarget};

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

// The side of the members' square, in shelf widths, past which a shelf
// width is chosen again: members that would pack more than four times taller
// than wide.
const SHELF_WIDTH_REACH: u32 = 2;

// The frames in a row whose need must fit one padding step under an atlas
// texture before the texture shrinks to the largest of those needs. A
// shorter window grew the texture again at the next busy frame: the
// gauntlet's 1,500 frames at tier 10 made 9 textures with 120 frames and 5
// with 480.
const SHRINK_FRAMES: u32 = 480;

// The textures the layer surfaces a frame draws again go into, one for the
// first atlas of a frame and one more for each further atlas. A render pass
// clears and stores its whole attachment, so a texture follows its atlases'
// need, rounded up to a padding step: one sized to the busiest recent frame
// with a step to spare drew the gauntlet's tier 10 frames at the Mate 20 X's
// size into 2.37 times their members' pixels. A texture grows on the side a
// need passes it, and shrinks once the need has fitted one step under it for
// `SHRINK_FRAMES` frames. The GL memory of a process stays at its peak, so a
// texture it replaces is let go before the new one is made. Members pack
// into one width while it holds them, so frames that differ in members
// differ in height.
pub(crate) struct SurfaceAtlases {
    format: wgpu::TextureFormat,
    shelf_width: u32,
    slots: SmallVec<[AtlasSlot; 2]>,
    next_slot: usize,
    frame: u64,
}

#[derive(Default)]
struct AtlasSlot {
    texture: Option<Rc<OffscreenTarget>>,
    // The frames in a row whose need fitted one step under the texture, and
    // the largest of those needs.
    under: u32,
    under_need: (u32, u32),
    last_frame: u64,
}

impl AtlasSlot {
    // The size the texture takes for a frame that needs `need`.
    fn settle(&mut self, need: (u32, u32)) -> (u32, u32) {
        let Some(kept) = self
            .texture
            .as_ref()
            .map(|texture| (texture.width, texture.height))
        else {
            self.under = 0;
            return need;
        };
        if need.0 > kept.0 || need.1 > kept.1 {
            self.under = 0;
            return (kept.0.max(need.0), kept.1.max(need.1));
        }
        if need.0 > step_below(kept.0) && need.1 > step_below(kept.1) {
            self.under = 0;
            return kept;
        }
        self.under_need = if self.under == 0 {
            need
        } else {
            (self.under_need.0.max(need.0), self.under_need.1.max(need.1))
        };
        self.under += 1;
        if self.under < SHRINK_FRAMES {
            return kept;
        }
        self.under = 0;
        self.under_need
    }
}

impl SurfaceAtlases {
    pub(crate) fn new(format: wgpu::TextureFormat) -> Self {
        Self {
            format,
            shelf_width: 0,
            slots: SmallVec::new(),
            next_slot: 0,
            frame: 0,
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

    // The texture `atlas` draws into, no side past `limit`. Its need is its
    // padded size; an atlas of more than one shelf needs the whole shelf
    // width, so a member one pixel narrower that fits one more into a shelf
    // changes no size.
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
        if self.slots.len() <= self.next_slot {
            self.slots.push(AtlasSlot::default());
        }
        let slot = &mut self.slots[self.next_slot];
        self.next_slot += 1;
        slot.last_frame = self.frame;
        let (width, height) = slot.settle((width, height));
        if let Some(texture) = slot
            .texture
            .as_ref()
            .filter(|texture| (texture.width, texture.height) == (width, height))
        {
            stats.record_offscreen_acquire(width, height, self.format, false);
            return Rc::clone(texture);
        }
        slot.texture = None;
        let texture = Rc::new(OffscreenTarget::new_labeled(
            device,
            self.format,
            width,
            height,
            "Layer Surface Atlas",
        ));
        stats.record_offscreen_acquire(width, height, self.format, true);
        slot.texture = Some(Rc::clone(&texture));
        texture
    }

    // Ends a frame, letting go the texture of a slot no frame used for
    // `crate::idle_pool::IDLE_FRAMES`.
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        self.next_slot = 0;
        let oldest_kept = self.frame.saturating_sub(crate::idle_pool::IDLE_FRAMES);
        for slot in &mut self.slots {
            if slot.last_frame < oldest_kept {
                *slot = AtlasSlot::default();
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.textures().count()
    }

    pub(crate) fn bytes(&self) -> u64 {
        let bytes_per_pixel = crate::frame_graph::texture_format_bytes_per_pixel(self.format);
        self.textures()
            .map(|texture| u64::from(texture.width) * u64::from(texture.height) * bytes_per_pixel)
            .sum()
    }

    fn textures(&self) -> impl Iterator<Item = &OffscreenTarget> {
        self.slots.iter().filter_map(|slot| slot.texture.as_deref())
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

// The padded dimension one padding step under `value`, a padded dimension.
fn step_below(value: u32) -> u32 {
    value.saturating_sub(padding_step(value))
}

fn padding_step(value: u32) -> u32 {
    (value.max(ATLAS_SIZE_STEP).next_power_of_two() / 8).max(ATLAS_SIZE_STEP)
}

pub(super) fn padded_dimension(value: u32, limit: u32) -> u32 {
    let step = padding_step(value);
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
