//! The window pixels a frame changes against the frame its composition
//! target kept. Every item the frame draws, an op, a child's composite, a
//! backdrop or an effect range, is summarized in drawing order as a hash of
//! what it draws and the window pixels it may touch; the summaries of the
//! two frames, compared item by item, bound the pixels that can differ.

use std::hash::{Hash, Hasher};

use cranpose_render_common::{
    geometry::blur_reach,
    graph::{CachePolicy, ProjectiveTransform},
};
use cranpose_ui_graphics::{FxHasher, Rect, RenderEffect, RenderHash};

use super::{
    DeviceRect, MAX_RESOLVE_DEPTH, clipped, composites_nothing, expand_rect, union_rect,
    z_ordered_ops,
};
use crate::{
    collect::{ChildLayer, LayerScene},
    draw_pass::op_draw_bounds,
    frame_packet::FramePacket,
    geometry::snap_delta_for_anchor,
    render::shadow_draw_bounds,
    scene::{
        BackdropLayer, ClipRounding, CompositorScene, DrawOp, DrawOpKind, EffectLayer, ImageDraw,
        LayerRoundedClip, Placement, RRectShadowDraw, RunDraw, ShadowDraw, SnapAnchor, TextDraw,
    },
};

#[path = "window_damage/swapchain_history.rs"]
mod swapchain_history;
pub(crate) use swapchain_history::SwapchainHistory;

/// The share of the window past which a frame draws all of it: a larger
/// part costs about what the whole does.
const FULL_SHARE: f32 = 0.5;
/// The most frames in a row a target whose frames keep changing most of
/// the window skips summarizing: about four seconds at 60 Hz, so a page
/// that never holds still pays two summaries in 258 frames.
const MAX_SKIPPED: u32 = 256;
/// The frames to skip at or past which a target lets its summaries go until
/// it summarizes again: after four frames in a row changed most of the
/// window.
const RELEASE_SKIPS: u32 = 64;
/// The windows of pixels a frame's items must cover before a presentable
/// image that comes cleared takes the frame from a kept copy: the copy reads
/// one window and writes one, so a lighter frame costs less drawn whole.
const COPY_COVERAGE: f32 = 2.0;
/// The frames in a row that change a small part of the window before the
/// composition target starts keeping the frame for images that come cleared.
const COPY_AFTER: u32 = 3;
/// The frames images that come cleared take before they first summarize,
/// about a second at 60 Hz: a page starting up keeps changing, and the copy
/// these summaries can start is worth little that early.
const COPY_HOLD_OFF: u32 = 64;

/// How a target summarizes its frames.
#[derive(Clone, Copy)]
pub(crate) struct Summaries {
    /// The windows of pixels below which the items of a frame that changes
    /// a part of the window cover so little that the whole costs less.
    windows: f32,
    /// The frames skipped after the first frame that changes most of the
    /// window, as a power of two: each such frame in a row quadruples them,
    /// up to [`MAX_SKIPPED`].
    first_skips_log2: u32,
}

impl Summaries {
    /// A target that keeps its pixels: each part it draws saves work at
    /// once, so it summarizes again soon after a busy frame.
    pub(crate) const KEPT: Self = Self {
        windows: 0.0,
        first_skips_log2: 0,
    };
    /// Images that come cleared: a copy pays only on a heavy page that holds
    /// mostly still, so they summarize seldom after a busy frame.
    pub(crate) const COPIED: Self = Self {
        windows: COPY_COVERAGE,
        first_skips_log2: COPY_HOLD_OFF.trailing_zeros(),
    };
}
/// The most rects a frame's damage keeps apart: changes far apart, such as
/// a spinner at the top and a caret at the bottom, each draw their own part
/// of the window, not a part spanning both.
pub(crate) const MAX_DAMAGE_RECTS: usize = 4;
/// The item between the root's items and the overlay's.
const OVERLAY_KEY: u64 = 0x6f76_6572_6c61_7973;

const BACKDROP: u8 = 0;
const EFFECT: u8 = 1;
const CHILD: u8 = 2;

/// What a frame changes against the frame its target kept.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Damage {
    /// Recent frames changed most of the window, so this one skipped
    /// summarizing.
    Skipped,
    /// The whole window: the target kept no frame to compare with, or the
    /// frame changes most of it.
    Full,
    /// Only the window pixels of these rects; none when no pixel changes.
    Part(DamageRects),
}

impl Damage {
    /// What of the page a frame with this damage draws.
    pub(crate) fn repaint(self) -> Repaint {
        match self {
            Self::Skipped | Self::Full => Repaint::Whole,
            Self::Part(rects) if rects.is_empty() => Repaint::Nothing,
            Self::Part(rects) => Repaint::Window(rects),
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
    fn holds(&self, rect: DeviceRect) -> bool {
        self.as_slice().iter().any(|part| holds(*part, rect))
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
    fn within(&self, window: DeviceRect) -> Self {
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

/// An item of the window: what it draws, and the window pixels it may
/// touch.
#[derive(Clone, Copy, PartialEq)]
struct Item {
    key: u64,
    bounds: Option<DeviceRect>,
}

/// A backdrop's reads and writes in window pixels: a changed pixel it
/// reads changes every pixel it writes.
#[derive(Clone, Copy)]
struct Capture {
    reads: DeviceRect,
    writes: DeviceRect,
}

/// A frame's items in drawing order, and its backdrops.
#[derive(Default)]
pub(crate) struct WindowSummary {
    items: Vec<Item>,
    captures: Vec<Capture>,
}

impl WindowSummary {
    /// The window pixels the items cover, a pixel counted once for each
    /// item over it; the first item, the frame's setting, covers none.
    fn coverage(&self, viewport: (u32, u32)) -> f32 {
        let window = window_rect(viewport);
        self.items
            .iter()
            .skip(1)
            .filter_map(|item| item.bounds?.intersect(window))
            .map(|bounds| bounds.width * bounds.height)
            .sum()
    }
}

/// The summary of the frame a composition target's pixels show; valid only
/// while they show that frame, drawn without a stand-in.
#[derive(Default)]
pub(crate) struct KeptWindow {
    summary: WindowSummary,
    valid: bool,
}

impl KeptWindow {
    /// Stops comparing with the kept frame: its pixels no longer show
    /// what a frame of the same items draws.
    pub(crate) fn forget(&mut self) {
        self.valid = false;
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

/// The frames drawn into presentable images that come cleared. While a page
/// whose items cover the window more than twice changes a small part of it,
/// the composition target keeps the frame, draws only what changes, and is
/// copied whole into each image; the first frame that changes most of the
/// window draws straight into its image again and lets the target go. The
/// images summarize first after [`COPY_HOLD_OFF`] frames.
#[derive(Default)]
pub(crate) struct ClearedImages {
    /// The summary of the frame drawn straight into an image last.
    pub(crate) kept: KeptWindow,
    /// The frames in a row drawn straight into an image that changed a
    /// small part of the window.
    small_frames: u32,
    /// Whether these images took a frame yet.
    started: bool,
    /// Whether the composition target keeps the frame for these images.
    pub(crate) copied: bool,
}

impl ClearedImages {
    /// What `frame`, drawn straight into an image, changes against the frame
    /// before, summarized by `summaries`; and whether the composition target
    /// is to keep a copy of it.
    pub(crate) fn damage(
        &mut self,
        summaries: &mut WindowDamage,
        frame: &FrameWindow<'_>,
    ) -> (Damage, bool) {
        if !self.started {
            self.started = true;
            summaries.hold_off(COPY_HOLD_OFF);
        }
        let damage = summaries.begin_as(frame, &mut self.kept, Summaries::COPIED);
        if !matches!(damage, Damage::Part(_)) {
            self.small_frames = 0;
            return (damage, false);
        }
        self.small_frames += 1;
        let keeps = self.small_frames >= COPY_AFTER;
        if keeps {
            self.small_frames = 0;
        }
        (damage, keeps)
    }

    /// Hands the summary of the frame just copied into the composition
    /// target to the target's `kept`. A frame that drew a stand-in leaves no
    /// summary, so the next frame draws whole and the target goes.
    pub(crate) fn hand_over(&mut self, target: &mut KeptWindow) {
        std::mem::swap(&mut self.kept, target);
        self.kept.forget();
        self.copied = true;
    }

    /// Takes back the summary of the frame the composition target kept,
    /// which goes: frames draw straight into their images again.
    pub(crate) fn take_back(&mut self, target: &mut KeptWindow) {
        std::mem::swap(&mut self.kept, target);
        self.copied = false;
        self.small_frames = 0;
    }
}

/// What a frame draws into the window.
pub(crate) struct FrameWindow<'a> {
    pub(crate) packet: &'a FramePacket,
    pub(crate) viewport: (u32, u32),
    /// What every pixel depends on besides the items, hashed: the clear
    /// colour, the glyph atlas and the debug switches.
    pub(crate) setting: u64,
}

/// Summarizes the frames drawn into a composition target and compares each
/// with the frame the target kept. While frames keep changing most of the
/// window, or keep leaving it no frame to compare with (a stand-in drawn, a
/// screenshot between), it skips 1, 4, 16, 64 and then [`MAX_SKIPPED`]
/// frames between summaries.
#[derive(Default)]
pub(crate) struct WindowDamage {
    next: WindowSummary,
    /// The items of the last summary, which the next one takes room for.
    last_items: usize,
    summarized: bool,
    skipped: u32,
    /// Whether the frame after the skipped ones summarizes as the frame the
    /// next one compares with, which a skipped frame leaves none of.
    baseline: bool,
    full_frames: u32,
    events: Vec<(usize, u8, usize)>,
    open: Vec<OpenEffect>,
}

impl WindowDamage {
    /// Summarizes `frame` and compares it with `kept`.
    pub(crate) fn begin(&mut self, frame: &FrameWindow<'_>, kept: &mut KeptWindow) -> Damage {
        if frame.packet.held_still {
            self.end_run();
        }
        self.begin_as(frame, kept, Summaries::KEPT)
    }

    // Ends the run of frames that changed most of the window: the app held
    // still before this frame, so the frames on either side of the pause are
    // not in a row, and this one summarizes as the frame after skipped ones
    // does. Skips counted in frames would otherwise outlast many presses of a
    // TV remote, each a few frames and a pause.
    fn end_run(&mut self) {
        if self.skipped > 0 {
            self.skipped = 0;
            self.baseline = true;
        }
        self.full_frames = 0;
    }

    /// Skips summarizing for the next `frames` frames; the frame after them
    /// summarizes as the frame the next one compares with.
    pub(crate) fn hold_off(&mut self, frames: u32) {
        self.skipped = self.skipped.max(frames);
    }

    /// Summarizes `frame` and compares it with `kept`, as [`Self::begin`]
    /// does, by `summaries`: a part of a frame whose items cover fewer
    /// windows of pixels than they say counts as the whole window, which
    /// costs that frame less.
    pub(crate) fn begin_as(
        &mut self,
        frame: &FrameWindow<'_>,
        kept: &mut KeptWindow,
        summaries: Summaries,
    ) -> Damage {
        self.summarized = false;
        if self.skipped > 0 {
            self.skipped -= 1;
            self.baseline = self.skipped == 0;
            return Damage::Skipped;
        }
        self.summarized = self.summarize(frame);
        let baseline = std::mem::take(&mut self.baseline);
        let damage = if self.summarized && kept.valid {
            match compare(&kept.summary, &self.next, frame.viewport) {
                Damage::Part(_)
                    if summaries.windows > 0.0 && self.covers_less(frame, summaries) =>
                {
                    Damage::Full
                }
                damage => damage,
            }
        } else if self.summarized && baseline {
            return Damage::Full;
        } else {
            Damage::Full
        };
        if damage == Damage::Full {
            self.full_frames = self.full_frames.saturating_add(1);
            let doublings = 2 * (self.full_frames - 1) + summaries.first_skips_log2;
            self.skipped = 1u32 << doublings.min(MAX_SKIPPED.trailing_zeros());
            if self.skipped >= RELEASE_SKIPS {
                self.release(kept);
            }
        } else {
            self.full_frames = 0;
        }
        damage
    }

    /// Whether the items of the frame just summarized cover fewer windows of
    /// pixels than `summaries` asks for.
    fn covers_less(&self, frame: &FrameWindow<'_>, summaries: Summaries) -> bool {
        let (width, height) = frame.viewport;
        self.next.coverage(frame.viewport) < summaries.windows * width as f32 * height as f32
    }

    /// What the frame [`Self::begin`] summarized, with a [`Damage::Part`],
    /// draws when the pixels `changed` differ from what its target holds.
    pub(crate) fn repaint(&self, changed: DamageRects, viewport: (u32, u32)) -> Repaint {
        if changed.is_empty() {
            return Repaint::Nothing;
        }
        widen(&self.next, changed, viewport).repaint()
    }

    /// Lets the summaries go, `kept`'s among them: a page that keeps changing
    /// most of the window holds none while it skips. The frame keeps none.
    fn release(&mut self, kept: &mut KeptWindow) {
        kept.summary = WindowSummary::default();
        self.next = WindowSummary::default();
        self.events = Vec::new();
        self.open = Vec::new();
        self.summarized = false;
    }

    /// Ends the frame [`Self::begin`] summarized: `kept` now holds its
    /// summary when the frame was `drawn` whole, with no stand-in.
    pub(crate) fn end(&mut self, kept: &mut KeptWindow, drawn: bool) {
        kept.valid = drawn && self.summarized;
        if kept.valid {
            std::mem::swap(&mut kept.summary, &mut self.next);
        }
        self.summarized = false;
    }

    /// Summarizes `frame` into `next`; false when it holds what the
    /// summary cannot describe.
    fn summarize(&mut self, frame: &FrameWindow<'_>) -> bool {
        self.next.items.clear();
        self.next.items.reserve(self.last_items);
        self.next.captures.clear();
        let scale = frame.packet.root_scale;
        if !scale.is_finite() || scale <= 0.0 {
            return false;
        }
        let window = window_rect(frame.viewport);
        let mut setting = FxHasher::default();
        frame.viewport.hash(&mut setting);
        setting.write_u32(scale.to_bits());
        setting.write_u64(frame.setting);
        self.next.items.push(Item {
            key: setting.finish(),
            bounds: Some(window),
        });
        let place = Place {
            to_window: ProjectiveTransform::uniform_scale(scale),
            clip: window,
            scale,
            depth: 0,
        };
        let mut walk = Walk {
            summary: &mut self.next,
            events: &mut self.events,
            open: &mut self.open,
            complete: true,
        };
        walk.layer(&frame.packet.root, &place, None);
        if let Some(overlay) = frame.packet.overlay.as_deref() {
            walk.summary.items.push(Item {
                key: OVERLAY_KEY,
                bounds: None,
            });
            walk.layer(overlay, &place, None);
        }
        let complete = walk.complete;
        self.last_items = self.next.items.len();
        complete
    }
}

fn window_rect((width, height): (u32, u32)) -> DeviceRect {
    DeviceRect {
        x: 0.0,
        y: 0.0,
        width: width as f32,
        height: height as f32,
    }
}

/// The pixels `next` changes against `kept`, and the pixels drawing them
/// again needs: the bounds of every item that differs, then all that every
/// backdrop touching them reads and writes. A changed pixel a backdrop reads
/// changes all it writes, and a backdrop drawn again reads the page beneath
/// it again, which must be drawn again then, not kept.
fn compare(kept: &WindowSummary, next: &WindowSummary, viewport: (u32, u32)) -> Damage {
    let window = window_rect(viewport);
    let most = FULL_SHARE * window.width * window.height;
    match changed_bounds(&kept.items, &next.items, most) {
        Some(changed) if changed.is_empty() => Damage::Part(changed),
        Some(changed) => widen(next, changed, viewport),
        None => Damage::Full,
    }
}

/// The pixels a frame `next` summarizes draws when the pixels `damage`
/// differ from what its target holds: all that every backdrop touching them
/// reads and writes too, in one rect with them, cut to the window.
fn widen(next: &WindowSummary, mut damage: DamageRects, viewport: (u32, u32)) -> Damage {
    loop {
        let mut grew = false;
        for capture in &next.captures {
            let touched = damage.meets(capture.reads) || damage.meets(capture.writes);
            let both = capture.reads.union(capture.writes);
            if touched && !damage.holds(both) {
                damage.add(both);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let window = window_rect(viewport);
    let damage = damage.within(window);
    if damage.is_empty() {
        return Damage::Part(damage);
    }
    if damage.pixels() > FULL_SHARE * window.width * window.height
        || !leaves_out_an_item(next, &damage, window)
    {
        Damage::Full
    } else {
        Damage::Part(damage)
    }
}

/// Whether drawing only `damage` leaves out an item that draws, beside
/// those that take the whole window. Without one, the part draws about all
/// the whole does, and a page cleared with its background's colour as a
/// whole frame starts fills its part by a draw.
fn leaves_out_an_item(next: &WindowSummary, damage: &DamageRects, window: DeviceRect) -> bool {
    next.items.iter().any(|item| {
        item.bounds
            .is_some_and(|bounds| !holds(bounds, window) && !damage.meets(bounds))
    })
}

/// The bounds of the items that differ, as rects: pair by pair when the
/// counts match, else everything between the common prefix and suffix.
/// `None` once they cover more than `most` pixels.
fn changed_bounds(kept: &[Item], next: &[Item], most: f32) -> Option<DamageRects> {
    let mut damage = DamageRects::default();
    let mut add = |item: &Item| {
        if let Some(bounds) = item.bounds {
            damage.add(bounds);
        }
        damage.pixels() <= most
    };
    if kept.len() == next.len() {
        for (before, after) in kept.iter().zip(next) {
            if before != after && !(add(before) && add(after)) {
                return None;
            }
        }
        return Some(damage);
    }
    let prefix = kept
        .iter()
        .zip(next)
        .take_while(|(before, after)| before == after)
        .count();
    let suffix = kept[prefix..]
        .iter()
        .rev()
        .zip(next[prefix..].iter().rev())
        .take_while(|(before, after)| before == after)
        .count();
    let changed = kept[prefix..kept.len() - suffix]
        .iter()
        .chain(&next[prefix..next.len() - suffix]);
    for item in changed {
        if !add(item) {
            return None;
        }
    }
    Some(damage)
}

fn holds(outer: DeviceRect, inner: DeviceRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}

/// Where a layer's content lands in the window.
#[derive(Clone, Copy)]
struct Place {
    /// From the layer's logical space to window pixels.
    to_window: ProjectiveTransform,
    /// The window pixels the layer's clips leave.
    clip: DeviceRect,
    /// The device pixels per logical unit the layer is drawn at.
    scale: f32,
    depth: usize,
}

impl Place {
    /// The window pixels `rect` of the layer's space may touch: a pixel of
    /// the layer's raster and a window pixel past it all around, for the
    /// filtering and snapping between them.
    fn reach(&self, rect: Rect) -> DeviceRect {
        let bounds = self
            .to_window
            .bounds_for_rect(expand_rect(rect, 1.0 / self.scale));
        DeviceRect {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        }
        .expand(1.0)
    }

    /// [`Self::reach`] within the layer's clips.
    fn window(&self, rect: Rect) -> Option<DeviceRect> {
        self.reach(rect).intersect(self.clip)
    }
}

/// An effect range whose items the walk is in: what they draw folds into
/// its key, and a change to any of them changes all the effect writes.
struct OpenEffect {
    z_end: usize,
    fold: FxHasher,
    writes: Option<DeviceRect>,
    captures: usize,
}

struct Walk<'w> {
    summary: &'w mut WindowSummary,
    /// The z-ordered children, backdrops and effects of the layers being
    /// walked, each layer's after its parent's.
    events: &'w mut Vec<(usize, u8, usize)>,
    open: &'w mut Vec<OpenEffect>,
    complete: bool,
}

impl Walk<'_> {
    /// Summarizes `layer`'s items in drawing order into the summary, or
    /// into `fold` when what encloses the layer spreads a change of any of
    /// them over all it draws; returns the bounds of what the layer draws,
    /// in its own space.
    fn layer(
        &mut self,
        layer: &LayerScene,
        place: &Place,
        mut fold: Option<&mut FxHasher>,
    ) -> Option<Rect> {
        if place.depth >= MAX_RESOLVE_DEPTH {
            self.complete = false;
            return None;
        }
        let scene = &layer.scene;
        let events = self.events.len();
        let children = layer.children.iter().enumerate();
        self.events
            .extend(children.map(|(index, child)| (child.z_index, CHILD, index)));
        let backdrops = scene.backdrop_layers.iter().enumerate();
        self.events
            .extend(backdrops.map(|(index, backdrop)| (backdrop.z_index, BACKDROP, index)));
        let effects = scene.effect_layers.iter().enumerate();
        self.events
            .extend(effects.map(|(index, effect)| (effect.z_start, EFFECT, index)));
        let events_end = self.events.len();
        self.events[events..].sort_unstable();
        let open = self.open.len();
        let ops = z_ordered_ops(&scene.draw_ops);
        let mut bounds = None;
        let mut next_op = 0;
        for event in events..events_end {
            let (z, kind, index) = self.events[event];
            while let Some(op) = ops.get(next_op).filter(|op| op.z_index < z) {
                let drawn = self.op(scene, op, place, fold.as_deref_mut(), open);
                bounds = union_rect(bounds, drawn);
                next_op += 1;
            }
            self.close_effects(z, open);
            let drawn = match kind {
                BACKDROP => self.backdrop(
                    &scene.backdrop_layers[index],
                    place,
                    fold.as_deref_mut(),
                    open,
                ),
                EFFECT => self.effect(&scene.effect_layers[index], place, fold.as_deref_mut()),
                _ => self.child(&layer.children[index], place, fold.as_deref_mut(), open),
            };
            bounds = union_rect(bounds, drawn);
        }
        for op in &ops[next_op..] {
            let drawn = self.op(scene, op, place, fold.as_deref_mut(), open);
            bounds = union_rect(bounds, drawn);
        }
        self.close_effects(usize::MAX, open);
        self.events.truncate(events);
        bounds
    }

    /// Puts an item into the summary and into the effect ranges open around
    /// it, or into the fold of what encloses its layer.
    fn emit(
        &mut self,
        key: u64,
        bounds: impl FnOnce() -> Option<DeviceRect>,
        fold: Option<&mut FxHasher>,
        open: usize,
    ) {
        if let Some(fold) = fold {
            fold.write_u64(key);
            return;
        }
        for effect in &mut self.open[open..] {
            effect.fold.write_u64(key);
        }
        self.summary.items.push(Item {
            key,
            bounds: bounds(),
        });
    }

    /// Puts the effect ranges ending at or before `z` into the summary, as
    /// the items the frame composites them as.
    #[inline]
    fn close_effects(&mut self, z: usize, open: usize) {
        if self.open.len() > open {
            self.close_open_effects(z, open);
        }
    }

    #[inline(never)]
    fn close_open_effects(&mut self, z: usize, open: usize) {
        while let Some(at) = self.open[open..]
            .iter()
            .rposition(|effect| effect.z_end <= z)
        {
            let effect = self.open.remove(open + at);
            if let Some(writes) = effect.writes {
                spread(&mut self.summary.captures[effect.captures..], writes);
            }
            self.emit(effect.fold.finish(), || effect.writes, None, open);
        }
    }

    fn op(
        &mut self,
        scene: &CompositorScene,
        op: &DrawOp,
        place: &Place,
        fold: Option<&mut FxHasher>,
        open: usize,
    ) -> Option<Rect> {
        self.close_effects(op.z_index, open);
        let bounds = op_bounds(scene, op, place.scale);
        let key = op_key(scene, op);
        self.emit(
            key,
            || bounds.and_then(|bounds| place.window(bounds)),
            fold,
            open,
        );
        bounds
    }

    fn backdrop(
        &mut self,
        backdrop: &BackdropLayer,
        place: &Place,
        fold: Option<&mut FxHasher>,
        open: usize,
    ) -> Option<Rect> {
        let key = backdrop_key(backdrop);
        let visible = (backdrop.alpha != 0.0)
            .then(|| {
                let snap = snap_delta(backdrop.snap_anchor, place.scale);
                let rect = backdrop.rect.translate(snap.x, snap.y);
                let reach = expand_rect(rect, backdrop.effect.output_padding());
                let clip = backdrop.clip.map(|clip| clip.translate(snap.x, snap.y));
                clipped(reach, clip)
            })
            .flatten();
        if let Some(visible) = visible {
            let reads = expand_rect(visible, backdrop.effect.input_padding());
            self.summary.captures.push(Capture {
                reads: place.reach(reads),
                writes: place.reach(visible),
            });
        }
        self.emit(
            key,
            || visible.and_then(|visible| place.window(visible)),
            fold,
            open,
        );
        visible
    }

    /// Opens an effect range, or folds the effect into what encloses it.
    fn effect(
        &mut self,
        effect: &EffectLayer,
        place: &Place,
        fold: Option<&mut FxHasher>,
    ) -> Option<Rect> {
        let key = effect_key(effect);
        let snap = snap_delta(effect.snap_anchor, place.scale);
        let padding = effect
            .effect
            .as_ref()
            .map_or(0.0, RenderEffect::output_padding);
        let written = expand_rect(effect.rect.translate(snap.x, snap.y), padding);
        let visible = clipped(
            written,
            effect.clip.map(|clip| clip.translate(snap.x, snap.y)),
        );
        match fold {
            Some(fold) => fold.write_u64(key),
            None => {
                let mut fold = FxHasher::default();
                fold.write_u64(key);
                self.open.push(OpenEffect {
                    z_end: effect.z_end,
                    fold,
                    writes: visible.and_then(|visible| place.window(visible)),
                    captures: self.summary.captures.len(),
                });
            }
        }
        visible
    }

    /// Summarizes a child as its composite, then its content's items; a
    /// child whose effect, or the effect range it lies in, spreads a change
    /// inside over all it writes, as one item that folds its content in.
    fn child(
        &mut self,
        child: &ChildLayer,
        place: &Place,
        fold: Option<&mut FxHasher>,
        open: usize,
    ) -> Option<Rect> {
        if child.mesh.is_some() {
            self.complete = false;
            return None;
        }
        let mut key = FxHasher::default();
        hash_child(child, &mut key);
        let clip = child.clip.map_or(Some(place.clip), |clip| {
            place.reach(clip).intersect(place.clip)
        });
        let inner = clip.map(|clip| Place {
            to_window: child.transform.then(place.to_window),
            clip,
            scale: place.scale * child.surface_scale,
            depth: place.depth + 1,
        });
        let folds = fold.is_some() || child.effect.is_some() || self.open.len() > open;
        let captures = self.summary.captures.len();
        let mut header = None;
        let content = match inner.filter(|_| !composites_nothing(child)) {
            Some(inner) if folds => {
                let mut content_fold = FxHasher::default();
                let content = self.layer(&child.content, &inner, Some(&mut content_fold));
                key.write_u64(content_fold.finish());
                content
            }
            Some(inner) => {
                header = Some(self.summary.items.len());
                self.summary.items.push(Item {
                    key: 0,
                    bounds: None,
                });
                self.layer(&child.content, &inner, None)
            }
            None => None,
        };
        let surface = child_surface(child, content);
        let backdrop = child
            .backdrop
            .as_ref()
            .filter(|_| child.backdrop_alpha != 0.0);
        let output = backdrop.map_or(0.0, RenderEffect::output_padding);
        let composite = inner.and_then(|inner| {
            if let Some(backdrop) = backdrop {
                let padding = backdrop.input_padding() + output;
                self.summary.captures.push(Capture {
                    reads: inner.reach(expand_rect(child.local_bounds, padding)),
                    writes: inner.reach(expand_rect(child.local_bounds, output)),
                });
            }
            inner.window(expand_rect(surface, output))
        });
        if folds && let Some(composite) = composite {
            spread(&mut self.summary.captures[captures..], composite);
        }
        let key = key.finish();
        match header {
            Some(header) => {
                self.summary.items[header] = Item {
                    key,
                    bounds: composite,
                }
            }
            None => self.emit(key, || composite, fold, open),
        }
        clipped(child.transform.bounds_for_rect(surface), child.clip)
    }
}

/// Widens what each capture writes by `writes`: an effect its output passes
/// through spreads a change of it there.
fn spread(captures: &mut [Capture], writes: DeviceRect) {
    for capture in captures {
        capture.writes = capture.writes.union(writes);
    }
}

/// The rect of the child's own space its surface covers: its box, and what
/// its content draws past it unless a rounded clip cuts that, grown by what
/// its effect reads and writes past its input.
fn child_surface(child: &ChildLayer, content: Option<Rect>) -> Rect {
    let bounds = if child.rounded_clip.is_some() {
        child.local_bounds
    } else {
        union_rect(Some(child.local_bounds), content).unwrap_or(child.local_bounds)
    };
    let padding = child.effect.as_ref().map_or(0.0, |effect| {
        effect.input_padding() + effect.output_padding()
    });
    expand_rect(bounds, padding)
}

/// The rect of the scene's space `op` may draw in.
fn op_bounds(scene: &CompositorScene, op: &DrawOp, scale: f32) -> Option<Rect> {
    if let DrawOpKind::Shadow(index) = op.kind {
        let shadow = &scene.shadow_draws[index];
        if shadow.requires_surface() {
            let reach = blur_reach(shadow.blur_radius, scale);
            return shadow_draw_bounds(shadow)
                .and_then(|bounds| clipped(expand_rect(bounds, reach), shadow.clip));
        }
    }
    op_draw_bounds(scene, op, scale)
}

fn snap_delta(anchor: Option<SnapAnchor>, scale: f32) -> cranpose_ui_graphics::Point {
    anchor
        .map(|anchor| snap_delta_for_anchor(anchor, scale))
        .unwrap_or_default()
}

fn op_key(scene: &CompositorScene, op: &DrawOp) -> u64 {
    let mut state = FxHasher::default();
    match op.kind {
        DrawOpKind::Run(index) => {
            state.write_u8(0);
            hash_run(&scene.runs[index], &mut state);
        }
        DrawOpKind::Image(index) => {
            state.write_u8(1);
            hash_image(&scene.images[index], &mut state);
        }
        DrawOpKind::Text(index) => {
            state.write_u8(2);
            hash_text(&scene.texts[index], &mut state);
        }
        DrawOpKind::Shadow(index) => {
            state.write_u8(3);
            hash_shadow(&scene.shadow_draws[index], &mut state);
        }
        DrawOpKind::RRectShadow(index) => {
            state.write_u8(4);
            hash_rrect_shadow(&scene.rrect_shadows[index], &mut state);
        }
    }
    state.finish()
}

fn hash_values<H: Hasher>(values: impl IntoIterator<Item = f32>, state: &mut H) {
    for value in values {
        state.write_u32(value.to_bits());
    }
}

fn hash_rect<H: Hasher>(rect: Rect, state: &mut H) {
    hash_values([rect.x, rect.y, rect.width, rect.height], state);
}

fn hash_option_rect<H: Hasher>(rect: Option<Rect>, state: &mut H) {
    state.write_u8(u8::from(rect.is_some()));
    if let Some(rect) = rect {
        hash_rect(rect, state);
    }
}

fn hash_anchor<H: Hasher>(anchor: Option<SnapAnchor>, state: &mut H) {
    state.write_u8(u8::from(anchor.is_some()));
    if let Some(anchor) = anchor {
        hash_values(
            [anchor.origin.x, anchor.origin.y, anchor.device_pixel_step],
            state,
        );
    }
}

fn hash_rounding<H: Hasher>(rounding: Option<ClipRounding>, state: &mut H) {
    state.write_u8(u8::from(rounding.is_some()));
    if let Some(rounding) = rounding {
        hash_rect(rounding.rect, state);
        hash_values([rounding.radius], state);
    }
}

fn hash_rounded_clip<H: Hasher>(clip: Option<LayerRoundedClip>, state: &mut H) {
    state.write_u8(u8::from(clip.is_some()));
    if let Some(clip) = clip {
        hash_rect(clip.rect, state);
        hash_values(clip.radii, state);
    }
}

fn hash_option_render_hash<H: Hasher, T: RenderHash>(value: Option<&T>, state: &mut H) {
    state.write_u64(value.map_or(0, RenderHash::render_hash));
}

fn hash_placement<H: Hasher>(placement: &Placement, state: &mut H) {
    hash_values(
        [placement.offset.x, placement.offset.y, placement.alpha],
        state,
    );
    hash_anchor(placement.snap_anchor, state);
    hash_option_rect(placement.clip, state);
    hash_rounding(placement.rounding, state);
    hash_option_render_hash(placement.color_filter.as_deref(), state);
}

fn hash_run<H: Hasher>(run: &RunDraw, state: &mut H) {
    state.write_u64(run.tables().fingerprint());
    state.write_u32(run.segments.start);
    state.write_u32(run.segments.end);
    hash_placement(&run.placement, state);
}

fn hash_text<H: Hasher>(text: &TextDraw, state: &mut H) {
    state.write_u64(text.text.render_hash());
    state.write_u64(text.color.render_hash());
    state.write_u64(text.style_hash);
    hash_values([text.font_size, text.scale], state);
    text.layout_options.hash(state);
    hash_rect(text.rect, state);
    hash_option_rect(text.clip, state);
    hash_anchor(text.snap_anchor, state);
}

fn hash_image<H: Hasher>(image: &ImageDraw, state: &mut H) {
    hash_rect(image.rect, state);
    hash_values(image.quad.into_iter().flatten(), state);
    hash_anchor(image.snap_anchor, state);
    state.write_u64(image.image.render_hash());
    hash_values([image.alpha], state);
    hash_option_render_hash(image.color_filter.as_ref(), state);
    image.sampling.hash(state);
    hash_option_rect(image.clip, state);
    hash_rounding(image.rounding, state);
    image.blend_mode.hash(state);
    hash_option_rect(image.src_rect, state);
    image.motion_context_animated.hash(state);
}

fn hash_shadow<H: Hasher>(shadow: &ShadowDraw, state: &mut H) {
    for run in [&shadow.shapes, &shadow.post_blur_cutouts] {
        state.write_u8(u8::from(run.is_some()));
        if let Some(run) = run {
            hash_run(run, state);
        }
    }
    state.write_usize(shadow.texts.len());
    for text in &shadow.texts {
        hash_text(text, state);
    }
    hash_values([shadow.blur_radius], state);
    hash_option_rect(shadow.clip, state);
    hash_rounded_clip(shadow.rounded_clip, state);
}

fn hash_rrect_shadow<H: Hasher>(draw: &RRectShadowDraw, state: &mut H) {
    let shadow = &draw.shadow;
    hash_rect(shadow.bounds, state);
    hash_values(shadow.radii, state);
    hash_values([shadow.umbra_inset, shadow.distance_correction], state);
    state.write_u8(u8::from(shadow.hole.is_some()));
    if let Some(hole) = shadow.hole {
        hash_values([hole.inset, hole.radius], state);
    }
    state.write_u64(draw.color.render_hash());
    hash_option_rect(draw.clip, state);
    hash_anchor(draw.snap_anchor, state);
}

fn backdrop_key(backdrop: &BackdropLayer) -> u64 {
    let mut state = FxHasher::default();
    hash_values([backdrop.alpha], &mut state);
    hash_rect(backdrop.rect, &mut state);
    hash_option_rect(backdrop.clip, &mut state);
    hash_option_rect(backdrop.reach, &mut state);
    hash_rounded_clip(backdrop.rounded_clip, &mut state);
    hash_anchor(backdrop.snap_anchor, &mut state);
    state.write_u64(backdrop.effect.render_hash());
    state.finish()
}

fn effect_key(effect: &EffectLayer) -> u64 {
    let mut state = FxHasher::default();
    hash_rect(effect.rect, &mut state);
    hash_option_rect(effect.clip, &mut state);
    hash_anchor(effect.snap_anchor, &mut state);
    hash_option_render_hash(effect.effect.as_ref(), &mut state);
    effect.blend_mode.hash(&mut state);
    hash_values([effect.composite_alpha], &mut state);
    state.finish()
}

/// Hashes how the child composites: everything but its content.
fn hash_child<H: Hasher>(child: &ChildLayer, state: &mut H) {
    hash_rect(child.local_bounds, state);
    hash_values(child.transform.matrix().into_iter().flatten(), state);
    hash_option_rect(child.clip, state);
    hash_rounded_clip(child.rounded_clip, state);
    hash_values(
        [child.alpha, child.backdrop_alpha, child.surface_scale],
        state,
    );
    child.blend_mode.hash(state);
    hash_option_render_hash(child.effect.as_ref(), state);
    hash_option_render_hash(child.backdrop.as_ref(), state);
    hash_anchor(child.snap_anchor, state);
    state.write_u8(u8::from(child.cache_policy == CachePolicy::Auto));
    state.write_u8(u8::from(child.in_place));
}
