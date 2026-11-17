// Step 4 of "How it works": a spot on the paragraph that follows the
// pointer over either copy, or wanders along the ends of the lines by
// itself, the word under it marked on both copies, and the pulse that runs
// along the cord between them.

use cranpose_core::{mutableStateOf, MutableState};
use cranpose_ui::{Point, Rect};

use super::{sheet_layout::SheetLayout, walk::ease};

// Seconds the pulse takes along the cord; it rests for a quarter of that.
const PULSE_SECONDS: f32 = 1.9;

// Where a pointer came over the paragraph, in sheet points, and the page's
// scroll then. A pointer that rests while the page scrolls on sends no event,
// though the paragraph moves away from under it.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct Pointer {
    pub at: Point,
    pub scroll: f32,
}

// The spot, the marked word and the pulse, which the paragraph draws and
// the cord follows, and the pointer they follow.
#[derive(Clone, Copy)]
pub(super) struct PointBack {
    pub pointer: MutableState<Option<Pointer>>,
    pub spot: MutableState<Option<Point>>,
    // The marked word's box, which slides from word to word.
    pub word: MutableState<Option<Rect>>,
    pub word_show: MutableState<f32>,
    // Where the pulse is along the cord, from 0 to 1.25; past 1 it rests.
    pub pulse: MutableState<f32>,
    // Seconds along the spot's own route.
    pub route_clock: MutableState<f32>,
}

impl PointBack {
    // At rest, with no pointer and the spot not yet placed.
    pub(super) fn new() -> Self {
        Self {
            pointer: mutableStateOf(None),
            spot: mutableStateOf(None),
            word: mutableStateOf(None),
            word_show: mutableStateOf(0.0),
            pulse: mutableStateOf(0.0),
            route_clock: mutableStateOf(0.5),
        }
    }

    // Where the pointer is over the paragraph while the page's scroll stays
    // at `scroll`, where it came.
    pub(super) fn pointer_at(self, scroll: f32) -> Option<Point> {
        self.pointer
            .get_non_reactive()
            .filter(|pointer| (pointer.scroll - scroll).abs() < 0.5)
            .map(|pointer| pointer.at)
    }

    // Moves everything on by `seconds`, over `layout`, with the page's
    // scroll at `scroll`.
    pub(super) fn advance(self, seconds: f32, layout: &SheetLayout, scroll: f32) {
        let pointer = self.pointer_at(scroll);
        let target = match pointer {
            Some(pointer) => Some(pointer),
            None => {
                let clock = self.route_clock.get_non_reactive() + seconds;
                self.route_clock.set(clock);
                route_point(layout, clock)
            }
        };
        self.pulse
            .set((self.pulse.get_non_reactive() + seconds / PULSE_SECONDS) % 1.25);
        let Some(target) = target else {
            return;
        };
        let follow = 1.0 - (-seconds * if pointer.is_some() { 34.0 } else { 9.0 }).exp();
        let spot = self.spot.get_non_reactive().unwrap_or(target);
        let spot = Point::new(
            spot.x + (target.x - spot.x) * follow,
            spot.y + (target.y - spot.y) * follow,
        );
        self.spot.set(Some(spot));
        let word = layout.word_near(spot.x, spot.y);
        let shown = self.word_show.get_non_reactive();
        let ease_toward =
            |from: f32, to: f32, rate: f32| from + (to - from) * (1.0 - (-seconds * rate).exp());
        match (word, self.word.get_non_reactive()) {
            (Some(word), Some(box_now)) if (word.y - box_now.y).abs() <= 3.0 => {
                let slide = |from: f32, to: f32| ease_toward(from, to, 22.0);
                self.word.set(Some(Rect {
                    x: slide(box_now.x, word.x),
                    y: slide(box_now.y, word.y),
                    width: slide(box_now.width, word.width),
                    height: slide(box_now.height, word.height),
                }));
                self.word_show.set(ease_toward(shown, 1.0, 14.0));
            }
            (Some(word), _) => {
                // On another line the box jumps there and fades in again.
                self.word.set(Some(word));
                self.word_show.set(0.0);
            }
            (None, _) => self.word_show.set(ease_toward(shown, 0.0, 14.0)),
        }
    }
}

// Where the spot wanders `time` seconds along its route: across the last
// word of each line, then on to the next line.
fn route_point(layout: &SheetLayout, time: f32) -> Option<Point> {
    let legs = layout.lines.iter().filter_map(|line| {
        let last = layout
            .words
            .iter()
            .rfind(|word| (word.y - line.y).abs() < line.height * 0.5)?;
        Some((
            1.3 + last.width / (150.0 * layout.u),
            last.x + last.width * 0.2,
            last.x + last.width * 0.8,
            last.y + last.height * 0.52,
        ))
    });
    let total: f32 = legs.clone().map(|(along, ..)| along + 0.8).sum();
    if total <= 0.0 {
        return None;
    }
    let mut left = time % total;
    let mut previous: Option<(f32, f32)> = None;
    let first = legs.clone().next();
    for (along, from, to, y) in legs {
        if let Some((end_x, end_y)) = previous {
            if left < 0.8 {
                let share = ease(0.0, 0.8, left);
                return Some(Point::new(
                    end_x + (from - end_x) * share,
                    end_y + (y - end_y) * share,
                ));
            }
            left -= 0.8;
        }
        if left <= along {
            return Some(Point::new(from + (to - from) * (left / along), y));
        }
        left -= along;
        previous = Some((to, y));
    }
    let ((end_x, end_y), (_, from, _, y)) = (previous?, first?);
    let share = ease(0.0, 0.8, left);
    Some(Point::new(
        end_x + (from - end_x) * share,
        end_y + (y - end_y) * share,
    ))
}
