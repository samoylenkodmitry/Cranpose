//! The window pixels a frame changes against the frame a target holds. A
//! packet carries the pixels its frame changes against the packet before
//! it; the renderer keeps the damages of the packets it took last, and a
//! target that holds an older frame draws their union since.

use super::DeviceRect;
use crate::{
    frame_packet::FramePacket,
    geometry::snap_delta_for_anchor,
    gpu_stats::{FrameDamage, WindowRects},
    scene::SnapAnchor,
};

#[path = "window_damage/captures.rs"]
mod captures;
pub(crate) use captures::{Capture, collect as collect_captures, widen};
#[path = "window_damage/cleared_images.rs"]
mod cleared_images;
pub(crate) use cleared_images::ClearedImages;
#[path = "window_damage/swapchain_history.rs"]
mod swapchain_history;
pub(crate) use swapchain_history::SwapchainHistory;

/// The share of the window past which a frame draws all of it: a larger
/// part costs about what the whole does.
pub(crate) const FULL_SHARE: f32 = 0.5;
/// The windows of pixels a frame's draws must cover before a presentable
/// image that comes cleared takes the frame from a kept copy: the copy reads
/// one window and writes one, so a lighter frame costs less drawn whole.
const COPY_COVERAGE: f32 = 2.0;
/// The frames in a row that change a small part of the window before the
/// composition target starts keeping the frame for images that come cleared.
const COPY_AFTER: u32 = 3;
/// The frames images that come cleared take before a kept copy may start,
/// about a second at 60 Hz: a page starting up keeps changing, and a copy
/// is worth little that early.
pub(crate) const COPY_HOLD_OFF: u32 = 64;
/// The packets back whose damages the renderer keeps.
const HISTORY: usize = 8;
/// The most rects a frame's damage keeps apart: changes far apart, such as
/// a spinner at the top and a caret at the bottom, each draw their own part
/// of the window, not a part spanning both.
pub(crate) const MAX_DAMAGE_RECTS: usize = 4;

/// What a frame changes against the frame before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Damage {
    /// Recent frames changed most of the window, so this one skipped working
    /// out what it changes.
    Skipped,
    /// The whole window: the frame before is not known, or the frame changes
    /// most of it.
    Full,
    /// Only the window pixels of these rects; none when no pixel changes.
    Part(DamageRects),
}

impl Damage {
    // The pixels either damage changes, in a window of `viewport`.
    pub(crate) fn union(self, other: Self, viewport: (u32, u32)) -> Self {
        match (self, other) {
            (Self::Part(mut rects), Self::Part(more)) => {
                for rect in more.as_slice() {
                    rects.add(*rect);
                }
                settle(rects, viewport)
            }
            (Self::Full, _) | (_, Self::Full) => Self::Full,
            (Self::Skipped, _) | (_, Self::Skipped) => Self::Skipped,
        }
    }

    /// What of the page a frame with this damage draws.
    pub(crate) fn repaint(self) -> Repaint {
        match self {
            Self::Skipped | Self::Full => Repaint::Whole,
            Self::Part(rects) if rects.is_empty() => Repaint::Nothing,
            Self::Part(rects) => Repaint::Window(rects),
        }
    }

    // The damage as a frame's statistics report it.
    pub(crate) fn stats(self) -> FrameDamage {
        match self {
            Self::Skipped => FrameDamage::Skipped,
            Self::Full => FrameDamage::Full,
            Self::Part(rects) => {
                let mut window = WindowRects::default();
                for rect in rects.as_slice() {
                    window
                        .push([rect.x, rect.y, rect.width, rect.height].map(|value| value as u32));
                }
                FrameDamage::Part(window)
            }
        }
    }
}

/// What of the page a frame draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Repaint {
    Whole,
    /// Only these rects inside the page; the rest keeps the frame before's
    /// pixels.
    Window(DamageRects),
    /// Nothing: the page holds the frame already.
    Nothing,
}

impl Repaint {
    // Whether every pixel the frame draws lies in one of the rects of
    // `region`.
    pub(crate) fn within(self, region: &DamageRects) -> bool {
        match self {
            Self::Whole => false,
            Self::Window(rects) => region.covers(&rects),
            Self::Nothing => true,
        }
    }

    /// The pixels the frame draws of a page of `viewport`.
    pub(crate) fn pixels(self, (width, height): (u32, u32)) -> u64 {
        match self {
            Self::Whole => u64::from(width) * u64::from(height),
            Self::Window(rects) => rects.pixels() as u64,
            Self::Nothing => 0,
        }
    }
}

/// Window pixels as up to [`MAX_DAMAGE_RECTS`] rects of whole pixels that
/// share no pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct DamageRects {
    rects: [DeviceRect; MAX_DAMAGE_RECTS],
    len: usize,
}

impl Default for DamageRects {
    fn default() -> Self {
        let none = DeviceRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        };
        Self {
            rects: [none; MAX_DAMAGE_RECTS],
            len: 0,
        }
    }
}

impl DamageRects {
    pub(crate) fn as_slice(&self) -> &[DeviceRect] {
        &self.rects[..self.len]
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The pixels of every rect.
    pub(crate) fn pixels(&self) -> f32 {
        self.as_slice()
            .iter()
            .map(|rect| rect.width * rect.height)
            .sum()
    }

    /// The rect holding every rect; `None` when there is none.
    pub(crate) fn bounds(&self) -> Option<DeviceRect> {
        self.as_slice().iter().copied().reduce(DeviceRect::union)
    }

    /// Whether `rect` shares a pixel with a rect.
    pub(crate) fn meets(&self, rect: DeviceRect) -> bool {
        self.as_slice()
            .iter()
            .any(|part| part.intersect(rect).is_some())
    }

    /// Whether one rect holds all of `rect`.
    pub(crate) fn holds(&self, rect: DeviceRect) -> bool {
        self.as_slice().iter().any(|part| holds(*part, rect))
    }

    // Whether the rects together hold every pixel of `rects`: the rects
    // share no pixel, so their parts of a rect add up to all of it only
    // when they cover it.
    pub(crate) fn covers(&self, rects: &Self) -> bool {
        rects.as_slice().iter().all(|rect| {
            let covered: f32 = self
                .as_slice()
                .iter()
                .filter_map(|part| part.intersect(*rect))
                .map(|part| part.width * part.height)
                .sum();
            covered >= rect.width * rect.height
        })
    }

    /// Adds `rect`, grown out to whole pixels. It merges with every rect it
    /// shares a pixel with; past [`MAX_DAMAGE_RECTS`], the two rects whose
    /// bounds add the fewest pixels merge.
    pub(crate) fn add(&mut self, rect: DeviceRect) {
        let mut rect = rect.snap_out();
        loop {
            while let Some(at) = self
                .as_slice()
                .iter()
                .position(|part| part.intersect(rect).is_some())
            {
                rect = rect.union(self.remove(at));
            }
            if self.len < MAX_DAMAGE_RECTS {
                self.rects[self.len] = rect;
                self.len += 1;
                return;
            }
            let (first, second) = self.cheapest_pair(rect);
            if second == self.len {
                rect = rect.union(self.remove(first));
            } else {
                let merged = self.rects[first].union(self.rects[second]);
                self.remove(second);
                self.remove(first);
                self.rects[self.len] = rect;
                self.len += 1;
                rect = merged;
            }
        }
    }

    /// The two of the rects and `rect`, at index `len`, whose bounds add the
    /// fewest pixels to theirs.
    fn cheapest_pair(&self, rect: DeviceRect) -> (usize, usize) {
        let candidate = |index: usize| {
            if index < self.len {
                self.rects[index]
            } else {
                rect
            }
        };
        let area = |rect: DeviceRect| rect.width * rect.height;
        let mut best = (0, 1, f32::INFINITY);
        for first in 0..=self.len {
            for second in first + 1..=self.len {
                let (a, b) = (candidate(first), candidate(second));
                let added = area(a.union(b)) - area(a) - area(b);
                if added < best.2 {
                    best = (first, second, added);
                }
            }
        }
        (best.0, best.1)
    }

    fn remove(&mut self, at: usize) -> DeviceRect {
        let rect = self.rects[at];
        self.rects.copy_within(at + 1..self.len, at);
        self.len -= 1;
        rect
    }

    /// The rects cut to `window`.
    pub(crate) fn within(&self, window: DeviceRect) -> Self {
        let mut within = Self::default();
        for part in self.as_slice() {
            if let Some(part) = part.intersect(window) {
                within.rects[within.len] = part;
                within.len += 1;
            }
        }
        within
    }
}

/// A swapchain image a frame draws into, as the surface handed it out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PresentedImage {
    pub(crate) index: u32,
    /// Whether it holds what it last presented.
    pub(crate) kept: bool,
}

/// What the renderer knows of the pixels a presentable image holds when a
/// frame draws into it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ImageContent {
    /// A texture the caller hands in, drawn whole.
    Untracked,
    /// A swapchain image its surface tells apart from the others.
    Numbered(PresentedImage),
    /// An image of a surface that hands every image out cleared, as a
    /// WebGPU canvas does.
    Cleared,
}

impl ImageContent {
    /// What `frame` holds: a surface that does not tell its images apart
    /// hands them out cleared.
    pub(crate) fn of(frame: &wgpu::SurfaceTexture) -> Self {
        frame.image().map_or(Self::Cleared, |image| {
            Self::Numbered(PresentedImage {
                index: image.index,
                kept: image.kept,
            })
        })
    }
}

/// What a frame draws into the window.
pub(crate) struct FrameWindow<'a> {
    pub(crate) packet: &'a FramePacket,
    pub(crate) viewport: (u32, u32),
    /// What every pixel depends on besides the scene, hashed: the clear
    /// colour, the glyph atlas and the debug switches.
    pub(crate) setting: u64,
}

impl Held {
    // The packet of the frame held.
    pub(crate) fn frame(self) -> u64 {
        self.frame
    }
}

impl FrameWindow<'_> {
    /// The frame as a target holds it once drawn.
    pub(crate) fn held(&self) -> Held {
        Held {
            frame: self.packet.frame_id,
            setting: self.setting,
            viewport: self.viewport,
        }
    }
}

/// What a packet's frame changes against the packet before it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PacketDamage {
    /// The packet the damage is against; `None` when the frame changes
    /// against no packet a target may hold.
    pub(crate) since: Option<u64>,
    pub(crate) damage: Damage,
}

impl PacketDamage {
    /// A frame that changes the whole window.
    pub(crate) const WHOLE: Self = Self {
        since: None,
        damage: Damage::Full,
    };
}

/// The frame a target's pixels hold, drawn whole of the scene with no
/// stand-in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Held {
    frame: u64,
    setting: u64,
    viewport: (u32, u32),
}

/// What a frame changes in a target, and whether a frame between, which
/// knew no frame before it, drew whole: such a frame is the one later frames
/// compare with, not a change of most of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Since {
    pub(crate) damage: Damage,
    pub(crate) baseline: bool,
    // Whether the target held no frame, so the frame draws whole for the
    // target's sake, whatever it changes.
    pub(crate) unheld: bool,
}

/// The damages of the packets the renderer took last, by frame.
#[derive(Default)]
pub(crate) struct FrameDamages {
    entries: [Option<(u64, PacketDamage)>; HISTORY],
}

impl FrameDamages {
    // The entry that holds the damage of `frame`, or of a frame `HISTORY`
    // frames apart.
    fn slot(frame: u64) -> usize {
        (frame % HISTORY as u64) as usize
    }

    fn noted(&self, frame: u64) -> Option<PacketDamage> {
        self.entries[Self::slot(frame)]
            .filter(|(noted, _)| *noted == frame)
            .map(|(_, damage)| damage)
    }

    /// Keeps the damage of the packet of `frame`.
    pub(crate) fn note(&mut self, frame: u64, damage: PacketDamage) {
        self.entries[Self::slot(frame)] = Some((frame, damage));
    }

    /// What `frame` changes in a target that holds `held`: the union of the
    /// damages of the packets since the held one; the whole window when the
    /// target holds none, holds one of another setting, or one of the
    /// damages since is not known.
    pub(crate) fn since(&self, held: Option<Held>, frame: &FrameWindow<'_>) -> Since {
        let changed = |damage| Since {
            damage,
            baseline: false,
            unheld: false,
        };
        if self
            .noted(frame.packet.frame_id)
            .is_some_and(|packet| packet.damage == Damage::Skipped)
        {
            return changed(Damage::Skipped);
        }
        let Some(held) = held else {
            return Since {
                unheld: true,
                ..changed(Damage::Full)
            };
        };
        if (held.setting, held.viewport) != (frame.setting, frame.viewport) {
            return changed(Damage::Full);
        }
        match self.union_back(frame.packet.frame_id, held.frame) {
            Ok(union) => changed(Damage::Part(union)),
            Err(since) => since,
        }
    }

    // The union of the damages of the packets from `at` back to the one
    // after `held`; what a target that holds `held` takes from them when
    // one of them is not a part of the window.
    pub(crate) fn union_back(&self, at: u64, held: u64) -> Result<DamageRects, Since> {
        let unknown = |baseline| Since {
            damage: Damage::Full,
            baseline,
            unheld: false,
        };
        // A frame that changed nothing against the one before is that one.
        let mut held_frame = held;
        for _ in 0..HISTORY {
            match self.noted(held_frame) {
                Some(PacketDamage {
                    since: Some(since),
                    damage: Damage::Part(changed),
                }) if changed.is_empty() => held_frame = since,
                _ => break,
            }
        }
        let mut union = DamageRects::default();
        let mut at = at;
        for _ in 0..=HISTORY {
            if at == held_frame || at == held {
                return Ok(union);
            }
            let Some(packet) = self.noted(at) else {
                return Err(unknown(false));
            };
            let Some(since) = packet.since else {
                return Err(unknown(true));
            };
            let Damage::Part(part) = packet.damage else {
                return Err(unknown(false));
            };
            for rect in part.as_slice() {
                union.add(*rect);
            }
            at = since;
        }
        Err(unknown(false))
    }
}

pub(crate) fn snap_delta(anchor: Option<SnapAnchor>, scale: f32) -> cranpose_ui_graphics::Point {
    anchor
        .map(|anchor| snap_delta_for_anchor(anchor, scale))
        .unwrap_or_default()
}

pub(crate) fn window_rect((width, height): (u32, u32)) -> DeviceRect {
    DeviceRect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    }
}

/// The damage `rects` make in a window of `viewport`: the rects cut to it,
/// or the whole window past [`FULL_SHARE`] of it.
pub(crate) fn settle(rects: DamageRects, viewport: (u32, u32)) -> Damage {
    let window = window_rect(viewport);
    let rects = rects.within(window);
    if rects.pixels() > FULL_SHARE * window.width * window.height {
        Damage::Full
    } else {
        Damage::Part(rects)
    }
}

fn holds(outer: DeviceRect, inner: DeviceRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}
