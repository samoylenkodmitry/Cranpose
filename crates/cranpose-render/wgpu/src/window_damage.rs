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
pub(crate) use cleared_images::{ClearedImages, keeps_copy, whole_when_lighter};
#[path = "window_damage/damage_rects.rs"]
mod damage_rects;
pub(crate) use damage_rects::{DamageBuilder, DamageRects, MAX_DAMAGE_RECTS};
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
/// The packets back whose damages the renderer keeps.
const HISTORY: usize = 8;

/// What a frame changes against the frame before.
// The part comes first: a damage of no pixel is then all zero bytes, which
// a write fills in place of a copy from a constant.
#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a frame's damage stays with its owners, which lend it; a box would allocate every frame"
)]
pub(crate) enum Damage {
    /// Only the window pixels of these rects; none when no pixel changes.
    Part(DamageRects),
    /// Recent frames changed most of the window, so this one skipped working
    /// out what it changes.
    Skipped,
    /// The whole window: the frame before is not known, or the frame changes
    /// most of it.
    Full,
}

impl Damage {
    // No pixel.
    pub(crate) const NONE: Self = Self::Part(DamageRects::EMPTY);

    // Whether the damage holds no pixel.
    pub(crate) fn is_none(&self) -> bool {
        matches!(self, Self::Part(rects) if rects.is_empty())
    }

    // Takes in the pixels `other` changes too, in a window of `viewport`.
    pub(crate) fn add(&mut self, other: &Self, viewport: (u32, u32)) {
        match (&mut *self, other) {
            (Self::Part(rects), Self::Part(more)) => {
                rects.add_all(more);
                self.settle(viewport);
            }
            (Self::Full, _) | (_, Self::Full) => *self = Self::Full,
            (Self::Skipped, _) | (_, Self::Skipped) => *self = Self::Skipped,
        }
    }

    // Cuts a part to a window of `viewport`, or takes the whole window for
    // a part past [`FULL_SHARE`] of it.
    pub(crate) fn settle(&mut self, viewport: (u32, u32)) {
        let Self::Part(rects) = self else {
            return;
        };
        rects.keep_within(window_rect(viewport));
        if self.takes_most_of(viewport) {
            *self = Self::Full;
        }
    }

    // Whether the damage takes more than [`FULL_SHARE`] of a window of
    // `viewport`: a part past it, or a damage that is no part.
    pub(crate) fn takes_most_of(&self, viewport: (u32, u32)) -> bool {
        let Self::Part(rects) = self else {
            return true;
        };
        let window = window_rect(viewport);
        rects.pixels() > FULL_SHARE * window.width * window.height
    }

    // Takes the value of `other`; a part copies only the rects it holds.
    pub(crate) fn assign(&mut self, other: &Self) {
        match other {
            Self::Skipped => *self = Self::Skipped,
            Self::Full => *self = Self::Full,
            Self::Part(rects) => self.assign_part(rects),
        }
    }

    // Takes the part `rects` make, copying only the rects they hold.
    pub(crate) fn assign_part(&mut self, rects: &DamageRects) {
        match self {
            Self::Part(held) => held.assign(rects),
            Self::Skipped | Self::Full => *self = Self::Part(*rects),
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
    // How many times pipelines a draw may stand in for landed: a stand-in
    // drawn at the same count draws the same pixels.
    pub(crate) landings: u64,
}

impl Held {
    // The frame held with `stand_ins` drawn with stand-ins besides the parts
    // it holds already, which a frame drawn over it after a landing draws
    // again: the whole window when it may show one anywhere.
    pub(crate) fn with_stand_ins(mut self, stand_ins: Damage) -> Self {
        self.stand_ins.add(&stand_ins, self.viewport);
        self
    }

    // The frame held with its stand-ins drawing `pixels`.
    pub(crate) fn with_stand_in_pixels(self, pixels: StandInPixels) -> Self {
        Self {
            stand_in_pixels: pixels,
            ..self
        }
    }

    // What a window whose one target holds this frame holds: with the parts
    // a frame after a landing draws again.
    pub(crate) fn window(self) -> HeldWindow {
        HeldWindow {
            oldest: self.frame,
            stand_ins: match self.stand_in_pixels {
                StandInPixels::Final => Damage::NONE,
                StandInPixels::Interim => self.stand_ins,
            },
            landings: Some(self.landings),
        }
    }
}

// What the parts of a held frame drawn in place of their pipelines show.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StandInPixels {
    // The pipelines' own pixels: a general or universal pipeline draws the
    // bytes of the specialization it stands in for, and a landing leaves
    // them as they are.
    Final,
    // Other pixels, a placeholder's, fallback triangles' or an instant
    // draw's, or kept layers that may hold them: a frame after a landing
    // draws them again.
    Interim,
}

impl StandInPixels {
    // Interim when either part may show a stand-in.
    pub(crate) fn with(self, other: Self) -> Self {
        match self {
            Self::Final => other,
            Self::Interim => Self::Interim,
        }
    }
}

// How many stand-ins the frames drew so far, reads of kept layers that may
// hold one among them: a layer holds a stand-in when the mark moved while
// it was drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StandInMark(pub(crate) u64);

impl StandInMark {
    // The pixels drawn between this mark and `now`.
    pub(crate) fn pixels_until(self, now: Self) -> StandInPixels {
        if now == self {
            StandInPixels::Final
        } else {
            StandInPixels::Interim
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
            landings: self.landings,
            stand_ins: Damage::NONE,
            stand_in_pixels: StandInPixels::Interim,
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

    // Takes the value of `other`; a part copies only the rects it holds.
    pub(crate) fn assign(&mut self, other: &Self) {
        self.since = other.since;
        self.damage.assign(&other.damage);
    }
}

/// The frame a target's pixels hold, drawn whole of the scene.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Held {
    frame: u64,
    setting: u64,
    viewport: (u32, u32),
    // The landings counted when the frame was drawn.
    landings: u64,
    // The parts drawn with stand-ins while pipelines compiled, or over kept
    // layers that may hold them; the whole window for a frame drawn whole.
    stand_ins: Damage,
    stand_in_pixels: StandInPixels,
}

/// What a frame changes in a target, and whether a frame between, which
/// knew no frame before it, drew whole: such a frame is the one later frames
/// compare with, not a change of most of the window.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Since {
    pub(crate) damage: Damage,
    pub(crate) baseline: bool,
    // Why the frame draws whole for the target's sake, whatever it changes.
    pub(crate) own_sake: Option<OwnSake>,
    // The parts of the target's frame drawn with stand-ins that the frame
    // leaves as they are: no pipeline landed since that frame.
    pub(crate) stand_ins: Damage,
}

// Why a frame draws the whole window for its target's sake.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OwnSake {
    // The target holds no frame.
    NoFrame,
    // The frame the target holds may show a stand-in anywhere.
    StandIns,
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

    fn noted(&self, frame: u64) -> Option<&PacketDamage> {
        self.entries[Self::slot(frame)]
            .as_ref()
            .filter(|(noted, _)| *noted == frame)
            .map(|(_, damage)| damage)
    }

    /// Keeps the damage of the packet of `frame`.
    pub(crate) fn note(&mut self, frame: u64, damage: &PacketDamage) {
        match &mut self.entries[Self::slot(frame)] {
            Some((noted, held)) => {
                *noted = frame;
                held.assign(damage);
            }
            empty => *empty = Some((frame, *damage)),
        }
    }

    /// What `frame` changes in a target that holds `held`: the damages since
    /// the held one and, after a landing, its parts drawn with stand-ins; the
    /// whole window when the target holds none, one of another setting, one
    /// that may show a stand-in anywhere after a landing, or no known damage.
    pub(crate) fn since(&self, held: Option<Held>, frame: &FrameWindow<'_>) -> Since {
        let changed = |damage| Since {
            damage,
            baseline: false,
            own_sake: None,
            stand_ins: Damage::NONE,
        };
        if self
            .noted(frame.packet.frame_id)
            .is_some_and(|packet| packet.damage == Damage::Skipped)
        {
            return changed(Damage::Skipped);
        }
        // A target that holds no frame, or one that may show a stand-in
        // anywhere, takes the frame whole for its own sake.
        let own_sake = |own_sake| Since {
            own_sake: Some(own_sake),
            ..changed(Damage::Full)
        };
        let Some(held) = held else {
            return own_sake(OwnSake::NoFrame);
        };
        // A stand-in drawn again before a landing draws the same pixels: the
        // frame keeps the held frame's stand-ins until one.
        let (redrawn, kept) = match held.stand_ins {
            _ if held.landings == frame.landings => (DamageRects::default(), held.stand_ins),
            _ if held.stand_in_pixels == StandInPixels::Final => {
                (DamageRects::default(), Damage::NONE)
            }
            Damage::Part(stand_ins) => (stand_ins, Damage::NONE),
            _ => return own_sake(OwnSake::StandIns),
        };
        if (held.setting, held.viewport) != (frame.setting, frame.viewport) {
            return changed(Damage::Full);
        }
        let mut union = DamageRects::default();
        match self.union_back(frame.packet.frame_id, held.frame, &mut union) {
            Ok(()) => {
                union.add_all(&redrawn);
                Since {
                    stand_ins: kept,
                    ..changed(Damage::Part(union))
                }
            }
            Err(no_part) => Since {
                baseline: no_part == NoPart::Baseline,
                ..changed(Damage::Full)
            },
        }
    }

    // Writes into `union` the union of the damages of the packets from `at`
    // back to the one after `held`; why they make no part of the window when
    // one of them is not a part.
    pub(crate) fn union_back(
        &self,
        at: u64,
        held: u64,
        union: &mut DamageRects,
    ) -> Result<(), NoPart> {
        // A frame that changed nothing against the one before is that one.
        let mut held_frame = held;
        for _ in 0..HISTORY {
            match self.noted(held_frame) {
                Some(&PacketDamage {
                    since: Some(since),
                    damage: Damage::Part(ref changed),
                }) if changed.is_empty() => held_frame = since,
                _ => break,
            }
        }
        union.clear();
        let mut at = at;
        for _ in 0..=HISTORY {
            if at == held_frame || at == held {
                return Ok(());
            }
            let Some(packet) = self.noted(at) else {
                return Err(NoPart::Unknown);
            };
            let Some(since) = packet.since else {
                return Err(NoPart::Baseline);
            };
            let Damage::Part(part) = &packet.damage else {
                return Err(NoPart::Unknown);
            };
            union.add_all(part);
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
    anchor.map_or_default(|anchor| snap_delta_for_anchor(anchor, scale))
}

pub(crate) fn window_rect((width, height): (u32, u32)) -> DeviceRect {
    DeviceRect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    }
}
