//! Window pixels as a few rects that share no pixel.

use super::DeviceRect;

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

/// Window pixels as up to [`MAX_DAMAGE_RECTS`] rects of whole pixels that
/// share no pixel. Two are equal when they hold the same rects.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DamageRects {
    rects: [DeviceRect; MAX_DAMAGE_RECTS],
    len: usize,
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

    // Keeps of each rect the part inside `window`.
    pub(super) fn keep_within(&mut self, window: DeviceRect) {
        let mut kept = 0;
        for at in 0..self.len {
            if let Some(part) = self.rects[at].intersect(window) {
                self.rects[kept] = part;
                kept += 1;
            }
        }
        self.len = kept;
    }

    // Takes the rects of `other` in place of its own.
    pub(crate) fn assign(&mut self, other: &Self) {
        self.rects[..other.len].copy_from_slice(other.as_slice());
        self.len = other.len;
    }

    // Holds no rect.
    pub(crate) fn clear(&mut self) {
        self.len = 0;
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
