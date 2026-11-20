//! Window pixels as a few rects that share no pixel.

use super::DeviceRect;

// The most rects a frame's damage keeps apart: changes far apart, such as
// a clock at the top, counters over a dashboard and a caret at the bottom,
// each draw their own part of the window, not a part spanning them. The
// patchwork at tier 17 holds up to 21 rects while its frame adds them; the
// ticker's 72 changing rows hold 32, and a bound of 16 takes the
// patchwork's pixels up by 39%.
pub(crate) const MAX_DAMAGE_RECTS: usize = 32;
const _: () = assert!(MAX_DAMAGE_RECTS < 256, "a u8 counts the rects");
// The pixels two rects of a frame's damage may add to theirs to draw as
// one. One more rect costs its scissor, the batches that break at it and a
// test of each of its pass's draws. Measured on an Apple M3 with the
// patchwork at tier 17: 33 to 43 microseconds a rect and 2.8 to 4.1 ms a
// million pixels. A merge that takes the damage past FULL_SHARE draws the
// whole window, so rects merge only where they almost touch. At 4096
// pixels some patchwork frames drew whole and the steady frame rate fell
// from 60 to 58.7. At 16384 every frame drew whole and it fell to 50.
const RECT_COST: f32 = 1024.0;

/// Window pixels as up to [`MAX_DAMAGE_RECTS`] rects of whole pixels that
/// share no pixel. Two are equal when they hold the same rects.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DamageRects {
    rects: [DeviceRect; MAX_DAMAGE_RECTS],
    len: u8,
}

impl Default for DamageRects {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl PartialEq for DamageRects {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl DamageRects {
    pub(super) const EMPTY: Self = Self {
        rects: [DeviceRect {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
        }; MAX_DAMAGE_RECTS],
        len: 0,
    };

    pub(crate) fn as_slice(&self) -> &[DeviceRect] {
        &self.rects[..usize::from(self.len)]
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

    // Adds `rect`, grown out to whole pixels, as `Merge::add` merges it.
    pub(crate) fn add(&mut self, rect: DeviceRect) {
        Merge {
            held: self,
            partners: &mut None,
        }
        .add(rect);
    }

    // Adds every rect of `rects`. A list that holds none takes each as it
    // is: they share no pixel already.
    pub(crate) fn add_all(&mut self, rects: &Self) {
        if self.is_empty() {
            for rect in rects.as_slice() {
                self.push(*rect);
            }
            return;
        }
        let mut run = self.run();
        for rect in rects.as_slice() {
            run.add(*rect);
        }
    }

    // Starts a run of adds into the rects.
    pub(crate) fn run(&mut self) -> DamageRun<'_> {
        DamageRun {
            held: self,
            partners: None,
        }
    }

    // Takes out the rect at `at`; the last rect moves into its place.
    fn swap_remove(&mut self, at: u8) -> DeviceRect {
        let at = usize::from(at);
        let rect = self.rects[at];
        self.len -= 1;
        self.rects[at] = self.rects[usize::from(self.len)];
        rect
    }

    fn push(&mut self, rect: DeviceRect) {
        self.rects[usize::from(self.len)] = rect;
        self.len += 1;
    }

    // Keeps of each rect the part inside `window`.
    pub(super) fn keep_within(&mut self, window: DeviceRect) {
        let mut kept = 0;
        for at in 0..usize::from(self.len) {
            if let Some(part) = self.rects[at].intersect(window) {
                self.rects[usize::from(kept)] = part;
                kept += 1;
            }
        }
        self.len = kept;
    }

    // Takes the rects of `other` in place of its own.
    pub(crate) fn assign(&mut self, other: &Self) {
        self.rects[..usize::from(other.len)].copy_from_slice(other.as_slice());
        self.len = other.len;
    }

    // Holds no rect.
    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }
}

// A frame's damage that takes rects one at a time: it keeps between its
// adds what their merges need, so a full damage finds its cheapest pair
// without testing every pair again.
#[derive(Default)]
pub(crate) struct DamageBuilder {
    rects: DamageRects,
    partners: Option<Partners>,
}

impl DamageBuilder {
    pub(crate) fn as_rects(&self) -> &DamageRects {
        &self.rects
    }

    pub(crate) fn clear(&mut self) {
        self.rects.clear();
        self.start_run();
    }

    // Starts a run of adds over the rects held: the partners go, so the
    // run's merges follow the rects alone.
    pub(crate) fn start_run(&mut self) {
        self.partners = None;
    }

    // Adds `rect`, grown out to whole pixels, as `Merge::add` merges it.
    pub(crate) fn add(&mut self, rect: DeviceRect) {
        Merge {
            held: &mut self.rects,
            partners: &mut self.partners,
        }
        .add(rect);
    }
}

// A run of adds into a list of rects: it keeps between its adds what their
// merges need, as `DamageBuilder` does, while the run lasts.
pub(crate) struct DamageRun<'a> {
    held: &'a mut DamageRects,
    partners: Option<Partners>,
}

impl DamageRun<'_> {
    // Adds `rect`, grown out to whole pixels, as `Merge::add` merges it.
    pub(crate) fn add(&mut self, rect: DeviceRect) {
        Merge {
            held: self.held,
            partners: &mut self.partners,
        }
        .add(rect);
    }
}

// A held rect's cheapest partner: the partner's index and the pixels the
// bounds of the two add to theirs.
#[derive(Clone, Copy)]
struct Partner {
    at: u8,
    added: f32,
}

// Each held rect's cheapest partner; `None` for a rect whose partner left,
// which searches again when a pair must merge.
#[derive(Clone, Copy)]
struct Partners([Option<Partner>; MAX_DAMAGE_RECTS]);

impl Partners {
    // Finds every held rect's cheapest partner, each pair tested once, for
    // partners that hold none.
    fn pair(&mut self, held: &DamageRects) {
        let rects = held.as_slice();
        for (first, a) in (0u8..).zip(rects) {
            for (second, b) in (0u8..).zip(rects).skip(usize::from(first) + 1) {
                let added = added_pixels(*a, *b);
                self.offer(first, second, added);
                self.offer(second, first, added);
            }
        }
    }

    // Takes `partner` as the partner of the rect at `at` when the two add
    // fewer pixels than the rect and its partner do.
    fn offer(&mut self, at: u8, partner: u8, added: f32) {
        let slot = &mut self.0[usize::from(at)];
        if slot.is_none_or(|held| added < held.added) {
            *slot = Some(Partner { at: partner, added });
        }
    }
}

// A run of adds into `held`. An add tests each held rect once a merge step,
// and once the rects reach their bound, it searches again only for the rects
// whose partner left.
struct Merge<'a> {
    held: &'a mut DamageRects,
    // Kept from the first add that finds the rects full.
    partners: &'a mut Option<Partners>,
}

impl Merge<'_> {
    // Adds `rect`, grown out to whole pixels. A rect that one held rect
    // holds changes nothing. Else it merges with every rect it shares a
    // pixel with, and with the rect whose bounds with it add the fewest
    // pixels while they cost less than one more rect. Past
    // MAX_DAMAGE_RECTS, the pair that adds the fewest pixels merges: the
    // new rect and its nearest, or two held rects.
    fn add(&mut self, rect: DeviceRect) {
        let mut rect = rect.snap_out();
        if self.held.holds(rect) {
            return;
        }
        // The pixels `rect` adds to the bounds of each held rect, by index.
        let mut added = [0.0; MAX_DAMAGE_RECTS];
        loop {
            let mut nearest: Option<(u8, f32)> = None;
            let mut shared = None;
            for (at, part) in (0u8..).zip(self.held.as_slice()) {
                if shares_pixels(*part, rect) {
                    shared = Some(at);
                    break;
                }
                let more = added_pixels(*part, rect);
                added[usize::from(at)] = more;
                if nearest.is_none_or(|(_, least)| more < least) {
                    nearest = Some((at, more));
                }
            }
            if let Some(at) = shared {
                rect = rect.union(self.take(at, &mut added));
                continue;
            }
            let Some((near, near_added)) = nearest else {
                self.push(rect, &added);
                return;
            };
            if near_added < RECT_COST {
                rect = rect.union(self.take(near, &mut added));
                continue;
            }
            if usize::from(self.held.len) < MAX_DAMAGE_RECTS {
                self.push(rect, &added);
                return;
            }
            let (first, second, pair_added) = self.cheapest_pair();
            if near_added <= pair_added {
                rect = rect.union(self.take(near, &mut added));
                continue;
            }
            let merged =
                self.held.rects[usize::from(first)].union(self.held.rects[usize::from(second)]);
            self.take(first.max(second), &mut added);
            self.take(first.min(second), &mut added);
            self.push(rect, &added);
            rect = merged;
        }
    }

    // The two held rects whose bounds add the fewest pixels to theirs, and
    // those pixels.
    fn cheapest_pair(&mut self) -> (u8, u8, f32) {
        let held = &*self.held;
        let partners = match &mut *self.partners {
            Some(partners) => partners,
            none => {
                let partners = none.insert(Partners([None; MAX_DAMAGE_RECTS]));
                partners.pair(held);
                partners
            }
        };
        let mut cheapest = (0, 0, f32::INFINITY);
        for at in 0..held.len {
            if partners.0[usize::from(at)].is_none() {
                for other in (0..held.len).filter(|&other| other != at) {
                    let added =
                        added_pixels(held.rects[usize::from(at)], held.rects[usize::from(other)]);
                    partners.offer(at, other, added);
                }
            }
            if let Some(partner) = partners.0[usize::from(at)]
                && partner.added < cheapest.2
            {
                cheapest = (at, partner.at, partner.added);
            }
        }
        cheapest
    }

    // Takes out the held rect at `at`, which the last one replaces, with the
    // pixels `added` by it.
    fn take(&mut self, at: u8, added: &mut [f32; MAX_DAMAGE_RECTS]) -> DeviceRect {
        let last = self.held.len - 1;
        let rect = self.held.swap_remove(at);
        added[usize::from(at)] = added[usize::from(last)];
        if let Some(partners) = self.partners {
            partners.0[usize::from(at)] = partners.0[usize::from(last)];
            for slot in &mut partners.0[..usize::from(last)] {
                *slot = slot.and_then(|partner| match partner.at {
                    gone if gone == at => None,
                    moved if moved == last => Some(Partner { at, ..partner }),
                    _ => Some(partner),
                });
            }
        }
        rect
    }

    // Holds `rect`, whose bounds with each held rect add `added` pixels.
    fn push(&mut self, rect: DeviceRect, added: &[f32; MAX_DAMAGE_RECTS]) {
        let at = self.held.len;
        self.held.push(rect);
        if let Some(partners) = self.partners {
            partners.0[usize::from(at)] = None;
            for (other, &more) in (0u8..).zip(&added[..usize::from(at)]) {
                partners.offer(at, other, more);
                // A rect that searches again finds `rect` in its search.
                if partners.0[usize::from(other)].is_some() {
                    partners.offer(other, at, more);
                }
            }
        }
    }
}

// The pixels the bounds of `a` and `b` add to theirs.
// The edges compare as plain numbers: the rects of a damage are finite,
// and `f32::min` and `f32::max` cost more for their NaN rule.
fn added_pixels(a: DeviceRect, b: DeviceRect) -> f32 {
    let lower = |p: f32, q: f32| if p < q { p } else { q };
    let higher = |p: f32, q: f32| if p > q { p } else { q };
    let width = higher(a.x + a.width, b.x + b.width) - lower(a.x, b.x);
    let height = higher(a.y + a.height, b.y + b.height) - lower(a.y, b.y);
    width * height - a.width * a.height - b.width * b.height
}

// Whether `a` and `b` share a pixel: both hold one at least, as whole
// pixels grown out or kept within a window.
fn shares_pixels(a: DeviceRect, b: DeviceRect) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

fn holds(outer: DeviceRect, inner: DeviceRect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && inner.x + inner.width <= outer.x + outer.width
        && inner.y + inner.height <= outer.y + outer.height
}
