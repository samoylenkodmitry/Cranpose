//! The window pixels a frame changes against the frame a target holds. A
//! packet carries the pixels its frame changes against the packet before
//! it; the renderer keeps the damages of the packets it took last, and a
//! target that holds an older frame draws their union since.

use super::DeviceRect;
use crate::{
    frame_packet::{FramePacket, HeldWindow},
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

// The share of the window past which a frame draws all of it. A part
// costs more a pixel than the whole: its walk tests each record against
// the parts, its draws break into batches at them, and an image that comes
// cleared takes a copy of the kept frame. Measured on an Apple M3: the
// patchwork's frames at tier 17 that change 29 to 49% of the window draw
// in 8.5 ms in parts and in 12.7 ms whole. The gauntlet's frames at tier
// 16 that change 87 to 93% cost 1.1 to 1.7 ms more CPU and 1.5 ms more
// wait in a part than whole.
pub(crate) const FULL_SHARE: f32 = 0.5;
/// The windows of pixels a frame's draws must cover before a presentable
/// image that comes cleared takes the frame from a kept copy: the copy reads
/// one window and writes one, so a lighter frame costs less drawn whole.
const COPY_COVERAGE: f32 = 2.0;
/// The frames in a row that change a small part of the window before the
/// composition target starts keeping the frame for images that come cleared.
const COPY_AFTER: u32 = 3;
/// The packets back whose damages the renderer keeps.
const HISTORY: usize = 8;
// The most rects a frame's damage keeps apart: changes far apart, such as
// a clock at the top, counters over a dashboard and a caret at the bottom,
// each draw their own part of the window, not a part spanning them.
pub(crate) const MAX_DAMAGE_RECTS: usize = 32;
// The pixels two rects of a frame's damage may add to theirs to draw as
// one. One more rect costs its scissor, the batches that break at it and a
// test of each of its pass's draws. Measured on an Apple M3 with the
// patchwork at tier 17: 33 to 43 microseconds a rect and 2.8 to 4.1 ms a
// million pixels. A merge that takes the damage past FULL_SHARE draws the
// whole window, so rects merge only where they almost touch. At 4096
// pixels some patchwork frames drew whole and the steady frame rate fell
// from 60 to 58.7. At 16384 every frame drew whole and it fell to 50.
const RECT_COST: f32 = 1024.0;

/// What a frame changes against the frame before.
#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a frame's damage moves a few times a frame; a box would allocate every frame"
)]
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
#[expect(
    clippy::large_enum_variant,
    reason = "a frame's repaint moves a few times a frame; a box would allocate every frame"
)]
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

    // Adds `rect`, grown out to whole pixels. It merges with every rect it
    // shares a pixel with, and with the rect whose bounds with it add the
    // fewest pixels while they cost less than one more rect; past
    // MAX_DAMAGE_RECTS, the two rects whose bounds add the fewest pixels
    // merge.
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
            if let Some((at, _)) = self.nearest(rect).filter(|&(_, added)| added < RECT_COST) {
                rect = rect.union(self.remove(at));
                continue;
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

    // The rect whose bounds with `rect` add the fewest pixels to theirs, and
    // those pixels.
    fn nearest(&self, rect: DeviceRect) -> Option<(usize, f32)> {
        self.as_slice()
            .iter()
            .map(|part| added_pixels(*part, rect))
            .enumerate()
            .min_by(|a, b| a.1.total_cmp(&b.1))
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
        let mut best = (0, 1, f32::INFINITY);
        for first in 0..=self.len {
            for second in first + 1..=self.len {
                let added = added_pixels(candidate(first), candidate(second));
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

    // Adds every rect of `rects`.
    pub(crate) fn add_all(&mut self, rects: &Self) {
        for rect in rects.as_slice() {
            self.add(*rect);
        }
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
    // How many images the surface hands out.
    pub(crate) images: u32,
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
                images: image.images,
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
    // The frame held with `stand_ins` drawn with stand-ins, which the next
    // frame drawn over it draws again: the whole window when it may show
    // one anywhere.
    pub(crate) fn with_stand_ins(self, stand_ins: Damage) -> Self {
        Self { stand_ins, ..self }
    }

    // What a window whose one target holds this frame holds.
    pub(crate) fn window(self) -> HeldWindow {
        HeldWindow {
            oldest: self.frame,
            stand_ins: self.stand_ins,
        }
    }
}

impl FrameWindow<'_> {
    /// The frame as a target holds it once drawn.
    pub(crate) fn held(&self) -> Held {
        Held {
            frame: self.packet.frame_id,
            setting: self.setting,
            viewport: self.viewport,
            stand_ins: Damage::Part(DamageRects::default()),
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

/// The frame a target's pixels hold, drawn whole of the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Held {
    frame: u64,
    setting: u64,
    viewport: (u32, u32),
    // The parts drawn with stand-ins while pipelines compiled, or over kept
    // layers that may hold them; the whole window for a frame drawn whole.
    stand_ins: Damage,
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

    /// What `frame` changes in a target that holds `held`: the damages of the
    /// packets since the held one and its parts drawn with stand-ins; the
    /// whole window when the target holds none, one of another setting or one
    /// that may show a stand-in anywhere, or a damage since is not known.
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
        // A target that holds no frame, or one that may show a stand-in
        // anywhere, takes the frame whole for its own sake.
        let Some((held, Damage::Part(stand_ins))) = held.map(|held| (held, held.stand_ins)) else {
            return Since {
                unheld: true,
                ..changed(Damage::Full)
            };
        };
        if (held.setting, held.viewport) != (frame.setting, frame.viewport) {
            return changed(Damage::Full);
        }
        match self.union_back(frame.packet.frame_id, held.frame) {
            Ok(mut union) => {
                union.add_all(&stand_ins);
                changed(Damage::Part(union))
            }
            Err(no_part) => Since {
                baseline: no_part == NoPart::Baseline,
                ..changed(Damage::Full)
            },
        }
    }

    // The union of the damages of the packets from `at` back to the one
    // after `held`; why they make no part of the window when one of them is
    // not a part.
    pub(crate) fn union_back(&self, at: u64, held: u64) -> Result<DamageRects, NoPart> {
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
                return Err(NoPart::Unknown);
            };
            let Some(since) = packet.since else {
                return Err(NoPart::Baseline);
            };
            let Damage::Part(part) = packet.damage else {
                return Err(NoPart::Unknown);
            };
            for rect in part.as_slice() {
                union.add(*rect);
            }
            at = since;
        }
        Err(NoPart::Unknown)
    }
}

// Why the damages back to a held frame make no part of the window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NoPart {
    // A frame between knew no frame before it and drew whole: the frames
    // after it compare with it.
    Baseline,
    // A damage is not known, or it is not a part of the window.
    Unknown,
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

// The pixels the bounds of `a` and `b` add to theirs.
fn added_pixels(a: DeviceRect, b: DeviceRect) -> f32 {
    let area = |rect: DeviceRect| rect.width * rect.height;
    area(a.union(b)) - area(a) - area(b)
}

fn holds(outer: DeviceRect, inner: DeviceRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}
