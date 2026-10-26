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

/// How many times the area the busiest recent frame packed into its atlas
/// the atlas may hold before it shrinks to that need.
pub(super) const SURFACE_ATLAS_SLACK: u64 = 4;

/// The size of the one atlas the layer surfaces a frame draws again go
/// into. Members pack into its width; it grows to fit a frame that needs
/// more and shrinks once the busiest frame of the last
/// [`crate::idle_pool::IDLE_FRAMES`] needs less than a
/// [`SURFACE_ATLAS_SLACK`]th of it. Sized to each frame's members instead,
/// the gauntlet's 5 and 10 cards a frame at tier 8 took four sizes in turn,
/// and the WebGL page held a texture and a depth buffer for each: 20 MiB
/// for 0.5 MP of surfaces.
///
/// A new size takes one padding step more in height where the GPU memory a
/// process holds stays at its peak, so the next busier frame fits: its
/// members pack into the atlas's width and add shelves below. A step on
/// each side drew the Mate 20 X gauntlet's tier 12 frames, which need at
/// most 1280 by 1352 pixels, into a 1536-pixel square. Metal takes a
/// dropped texture's memory back, so there a new size is the size asked
/// for.
pub(crate) struct SurfaceAtlasSizes {
    current: Option<(u32, u32)>,
    need_width: crate::idle_pool::RecentPeak,
    need_height: crate::idle_pool::RecentPeak,
    step_ahead: bool,
}

impl SurfaceAtlasSizes {
    pub(crate) fn for_backend(backend: wgpu::Backend) -> Self {
        Self {
            current: None,
            need_width: crate::idle_pool::RecentPeak::default(),
            need_height: crate::idle_pool::RecentPeak::default(),
            step_ahead: backend != wgpu::Backend::Metal,
        }
    }

    /// The width members pack into: the atlas's own once it holds the
    /// widest of them, else about the square root of their `area`.
    pub(super) fn shelf_width(&self, area: u64, widest: u32, limit: u32) -> u32 {
        match self.current {
            Some((width, _)) if width >= widest => width,
            _ => u32::try_from(area.isqrt()).unwrap_or(limit),
        }
    }

    /// The size the atlas whose members pack into `size`, a padded size,
    /// takes this frame; no side passes `limit`.
    pub(super) fn settle(&mut self, size: (u32, u32), limit: u32) -> (u32, u32) {
        let area = |(width, height): (u32, u32)| u64::from(width) * u64::from(height);
        self.need_width.reach(u64::from(size.0));
        self.need_height.reach(u64::from(size.1));
        let need = (
            u32::try_from(self.need_width.value()).unwrap_or(limit),
            u32::try_from(self.need_height.value()).unwrap_or(limit),
        );
        let settled = match self.current {
            Some(current) if current.0 >= size.0 && current.1 >= size.1 => {
                if area(current) > area(self.grown(need, limit)).saturating_mul(SURFACE_ATLAS_SLACK)
                {
                    self.grown(need, limit)
                } else {
                    current
                }
            }
            Some(current) => {
                let grown = self.grown(size, limit);
                (current.0.max(grown.0), current.1.max(grown.1))
            }
            None => self.grown(size, limit),
        };
        self.current = Some(settled);
        settled
    }

    fn grown(&self, size: (u32, u32), limit: u32) -> (u32, u32) {
        if self.step_ahead {
            (size.0, padded_dimension(size.1.saturating_add(1), limit))
        } else {
            size
        }
    }

    /// Ends a frame, letting the recent need follow the frames since the
    /// busiest one once that is [`crate::idle_pool::IDLE_FRAMES`] old.
    pub(crate) fn end_frame(&mut self) {
        self.need_width.end_frame();
        self.need_height.end_frame();
    }
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
