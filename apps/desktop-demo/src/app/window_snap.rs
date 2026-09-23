use std::collections::HashMap;

use cranpose::WindowState;
use cranpose_core::MutableState;
use cranpose_ui::{Point, Size};

/// How near an edge has to come before the window lines up with it.
pub const SNAP_REACH: f32 = 12.0;
/// How far apart two edges may sit and still count as touching.
pub const ATTACH_EPSILON: f32 = 3.0;

/// Where a window is and how big it is, which is all the geometry needs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pane {
    pub origin: Point,
    pub size: Size,
}

impl Pane {
    fn right(self) -> f32 {
        self.origin.x + self.size.width
    }

    fn bottom(self) -> f32 {
        self.origin.y + self.size.height
    }
}

/// Where a window dragged to `moving` should sit once it has lined up with
/// whichever neighbour edge it came near.
pub fn lined_up_with_neighbours(moving: Pane, neighbours: &[Pane]) -> Point {
    neighbours.iter().fold(moving.origin, |at, other| Point {
        x: alongside(
            moving.origin.y,
            moving.bottom(),
            other.origin.y,
            other.bottom(),
        )
        .then(|| nearest_edge(at.x, moving.size.width, other.origin.x, other.right()))
        .flatten()
        .unwrap_or(at.x),
        y: alongside(
            moving.origin.x,
            moving.right(),
            other.origin.x,
            other.right(),
        )
        .then(|| nearest_edge(at.y, moving.size.height, other.origin.y, other.bottom()))
        .flatten()
        .unwrap_or(at.y),
    })
}

/// Whether two windows are touching along an edge, which is what makes one
/// travel with the other.
pub fn touching(a: Pane, b: Pane) -> bool {
    let sides = meet(a.right(), b.origin.x) || meet(b.right(), a.origin.x);
    let ends = meet(a.bottom(), b.origin.y) || meet(b.bottom(), a.origin.y);
    sides && overlaps(a.origin.y, a.bottom(), b.origin.y, b.bottom())
        || ends && overlaps(a.origin.x, a.right(), b.origin.x, b.right())
}

/// Every window that travels with `moved`, directly or through another.
pub fn carried_by(moved: u64, panes: &HashMap<u64, Pane>) -> Vec<u64> {
    let mut carried = vec![moved];
    let mut reach = panes.get(&moved).copied().into_iter().collect::<Vec<_>>();
    while let Some(pane) = reach.pop() {
        for (key, other) in panes {
            if !carried.contains(key) && touching(pane, *other) {
                carried.push(*key);
                reach.push(*other);
            }
        }
    }
    carried.remove(0);
    carried
}

fn nearest_edge(start: f32, length: f32, other_start: f32, other_end: f32) -> Option<f32> {
    [
        other_end,
        other_start - length,
        other_start,
        other_end - length,
    ]
    .into_iter()
    .filter(|edge| (edge - start).abs() <= SNAP_REACH)
    .min_by(|a, b| (a - start).abs().total_cmp(&(b - start).abs()))
}

fn overlaps(start: f32, end: f32, other_start: f32, other_end: f32) -> bool {
    start < other_end - ATTACH_EPSILON && other_start < end - ATTACH_EPSILON
}

/// Whether two spans are beside each other on the other axis, counting one
/// that stops just short: a window carried under another has no overlap with
/// it at all, and still lines its sides up.
fn alongside(start: f32, end: f32, other_start: f32, other_end: f32) -> bool {
    start < other_end + SNAP_REACH && other_start < end + SNAP_REACH
}

fn meet(a: f32, b: f32) -> bool {
    (a - b).abs() <= ATTACH_EPSILON
}

/// Where each window came to rest the last time it was let go, which is what
/// tells the next move which of its neighbours were lined up with it.
///
/// Remember one of these per set of windows that snap together, with
/// `rememberMutableStateOf(HashMap::new)`.
pub type RestingPlaces = MutableState<HashMap<u64, Point>>;

/// Forgets a window that is no longer on screen.
pub fn forget(resting: RestingPlaces, key: u64) {
    if resting.get_non_reactive().contains_key(&key) {
        resting.update(|resting| {
            resting.remove(&key);
        });
    }
}

/// Lines a moved window up with its neighbours and carries the ones attached
/// to it along.
///
/// Call it from a window's `on_moved` with the states of every window that can
/// snap. Cranpose reports only the moves the application did not ask for, so
/// what arrives here is the person dragging.
pub fn window_moved(resting: RestingPlaces, moved: u64, to: Point, windows: &[(u64, WindowState)]) {
    let panes = panes_with(moved, to, windows);
    let Some(moving) = panes.get(&moved).copied() else {
        return;
    };
    let was = resting.get_non_reactive().get(&moved).copied();
    let carried = was.map_or_else(Vec::new, |was| {
        let mut before = panes.clone();
        before.insert(
            moved,
            Pane {
                origin: was,
                size: moving.size,
            },
        );
        carried_by(moved, &before)
    });
    let loose: Vec<Pane> = panes
        .iter()
        .filter(|(key, _)| **key != moved && !carried.contains(key))
        .map(|(_, pane)| *pane)
        .collect();
    let lined_up = lined_up_with_neighbours(moving, &loose);
    let delta = was.map_or(Point::new(0.0, 0.0), |was| {
        Point::new(lined_up.x - was.x, lined_up.y - was.y)
    });

    for (key, state) in windows {
        if *key == moved {
            state.set_position(Some(lined_up));
        } else if carried.contains(key) {
            state.translate(delta.x, delta.y);
        }
    }
    resting.update(|resting| resting.extend(resting_places(moved, lined_up, windows)));
}

fn panes_with(moved: u64, to: Point, windows: &[(u64, WindowState)]) -> HashMap<u64, Pane> {
    windows
        .iter()
        .filter_map(|(key, state)| {
            let origin = if *key == moved {
                Some(to)
            } else {
                state.position_non_reactive()
            };
            Some((
                *key,
                Pane {
                    origin: origin?,
                    size: state.frame_size_non_reactive(),
                },
            ))
        })
        .collect()
}

fn resting_places(
    moved: u64,
    lined_up: Point,
    windows: &[(u64, WindowState)],
) -> Vec<(u64, Point)> {
    windows
        .iter()
        .filter_map(|(key, state)| {
            let origin = if *key == moved {
                Some(lined_up)
            } else {
                state.position_non_reactive()
            };
            Some((*key, origin?))
        })
        .collect()
}

#[cfg(test)]
#[path = "tests/window_snap_tests.rs"]
mod tests;
