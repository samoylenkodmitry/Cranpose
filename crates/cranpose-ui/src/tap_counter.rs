use std::cell::Cell;

use cranpose_foundation::PointerUptime;
use cranpose_ui_graphics::Point;

use crate::text_selection::{MULTI_TAP_SLOP_PX, MULTI_TAP_TIMEOUT_MS};

#[derive(Clone, Copy)]
struct Tap {
    uptime: PointerUptime,
    position: Point,
    count: u8,
}

#[derive(Default)]
pub(crate) struct TapCounter {
    last: Cell<Option<Tap>>,
}

impl TapCounter {
    pub(crate) fn count_at(&self, uptime: PointerUptime, position: Point) -> u8 {
        self.last
            .get()
            .filter(|last| {
                uptime
                    .millis_since(last.uptime)
                    .is_some_and(|millis| millis <= MULTI_TAP_TIMEOUT_MS)
                    && within_slop(last.position, position)
            })
            .map_or(1, |last| last.count.saturating_add(1))
    }

    pub(crate) fn last_count(&self) -> u8 {
        self.last.get().map_or(0, |last| last.count)
    }

    // Whether `position` lands within the multi-tap slop of the last press,
    // however long ago it was.
    pub(crate) fn in_place(&self, position: Point) -> bool {
        self.last
            .get()
            .is_some_and(|last| within_slop(last.position, position))
    }

    pub(crate) fn record(&self, uptime: PointerUptime, position: Point, count: u8) {
        self.last.set(Some(Tap {
            uptime,
            position,
            count,
        }));
    }
}

fn within_slop(from: Point, to: Point) -> bool {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    dx * dx + dy * dy <= MULTI_TAP_SLOP_PX * MULTI_TAP_SLOP_PX
}
