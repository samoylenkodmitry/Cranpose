use cranpose_core::NodeId;
use cranpose_ui_graphics::Rect;

use crate::{focus_order::FocusEntry, modifier::FocusDirection};

// One of the four directions an arrow key or a D-pad moves focus in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arrow {
    Left,
    Right,
    Up,
    Down,
}

impl Arrow {
    pub(crate) fn of(direction: FocusDirection) -> Option<Self> {
        match direction {
            FocusDirection::Left => Some(Self::Left),
            FocusDirection::Right => Some(Self::Right),
            FocusDirection::Up => Some(Self::Up),
            FocusDirection::Down => Some(Self::Down),
            FocusDirection::Next
            | FocusDirection::Previous
            | FocusDirection::Enter
            | FocusDirection::Exit => None,
        }
    }

    fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

#[derive(Clone, Copy)]
struct Edges {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Edges {
    fn of(rect: Rect) -> Self {
        Self {
            left: rect.x,
            top: rect.y,
            right: rect.x + rect.width,
            bottom: rect.y + rect.height,
        }
    }

    fn center_x(self) -> f32 {
        (self.left + self.right) * 0.5
    }

    fn center_y(self) -> f32 {
        (self.top + self.bottom) * 0.5
    }

    fn translated(self, dx: f32, dy: f32) -> Self {
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right + dx,
            bottom: self.bottom + dy,
        }
    }
}

// The point a search starts from when nothing holds focus, as Compose
// enters a focus group: the top left corner of `entries` for Right and
// Down, the bottom right corner for Left and Up.
pub(crate) fn entry_point(entries: &[FocusEntry], arrow: Arrow) -> Option<Rect> {
    let mut bounds = entries.iter().map(|entry| Edges::of(entry.rect));
    let first = bounds.next()?;
    let union = bounds.fold(first, |union, edges| Edges {
        left: union.left.min(edges.left),
        top: union.top.min(edges.top),
        right: union.right.max(edges.right),
        bottom: union.bottom.max(edges.bottom),
    });
    let (x, y) = match arrow {
        Arrow::Right | Arrow::Down => (union.left, union.top),
        Arrow::Left | Arrow::Up => (union.right, union.bottom),
    };
    Some(Rect {
        x,
        y,
        width: 0.0,
        height: 0.0,
    })
}

// The target an arrow moves focus to from `focused`, by the rules of
// Compose's two-dimensional focus search: a target must lie at least partly
// past the focused rect in that direction; one inside the focused rect's
// beam beats one outside it; then the one with the smallest
// `13 * major^2 + minor^2` wins, where major is the gap along the arrow and
// minor the distance between centers across it.
pub(crate) fn find_best_candidate<'a>(
    candidates: impl IntoIterator<Item = &'a FocusEntry>,
    focused: Rect,
    arrow: Arrow,
) -> Option<NodeId> {
    let focused = Edges::of(focused);
    let mut best = match arrow {
        Arrow::Left => focused.translated(focused.right - focused.left + 1.0, 0.0),
        Arrow::Right => focused.translated(-(focused.right - focused.left + 1.0), 0.0),
        Arrow::Up => focused.translated(0.0, focused.bottom - focused.top + 1.0),
        Arrow::Down => focused.translated(0.0, -(focused.bottom - focused.top + 1.0)),
    };
    let mut found = None;
    for candidate in candidates {
        let rect = Edges::of(candidate.rect);
        if is_better_candidate(rect, best, focused, arrow) {
            best = rect;
            found = Some(candidate.node_id);
        }
    }
    found
}

fn is_candidate(rect: Edges, focused: Edges, arrow: Arrow) -> bool {
    match arrow {
        Arrow::Left => {
            (focused.right > rect.right || focused.left >= rect.right) && focused.left > rect.left
        }
        Arrow::Right => {
            (focused.left < rect.left || focused.right <= rect.left) && focused.right < rect.right
        }
        Arrow::Up => {
            (focused.bottom > rect.bottom || focused.top >= rect.bottom) && focused.top > rect.top
        }
        Arrow::Down => {
            (focused.top < rect.top || focused.bottom <= rect.top) && focused.bottom < rect.bottom
        }
    }
}

// The gap from the focused rect's edge in the arrow's direction to the
// nearest edge of `rect`, or 0 where they overlap.
fn major_axis_distance(rect: Edges, focused: Edges, arrow: Arrow) -> f32 {
    let distance = match arrow {
        Arrow::Left => focused.left - rect.right,
        Arrow::Right => rect.left - focused.right,
        Arrow::Up => focused.top - rect.bottom,
        Arrow::Down => rect.top - focused.bottom,
    };
    distance.max(0.0)
}

fn major_axis_distance_to_far_edge(rect: Edges, focused: Edges, arrow: Arrow) -> f32 {
    let distance = match arrow {
        Arrow::Left => focused.left - rect.left,
        Arrow::Right => rect.right - focused.right,
        Arrow::Up => focused.top - rect.top,
        Arrow::Down => rect.bottom - focused.bottom,
    };
    distance.max(1.0)
}

fn weighted_distance(rect: Edges, focused: Edges, arrow: Arrow) -> u64 {
    let major = major_axis_distance(rect, focused, arrow) as u64;
    let minor = if arrow.is_horizontal() {
        (focused.center_y() - rect.center_y()).abs()
    } else {
        (focused.center_x() - rect.center_x()).abs()
    } as u64;
    13 * major * major + minor * minor
}

fn is_better_candidate(proposed: Edges, current: Edges, focused: Edges, arrow: Arrow) -> bool {
    if !is_candidate(proposed, focused, arrow) {
        return false;
    }
    if !is_candidate(current, focused, arrow) || beam_beats(focused, proposed, current, arrow) {
        return true;
    }
    if beam_beats(focused, current, proposed, arrow) {
        return false;
    }
    weighted_distance(proposed, focused, arrow) < weighted_distance(current, focused, arrow)
}

fn in_beam(rect: Edges, source: Edges, arrow: Arrow) -> bool {
    if arrow.is_horizontal() {
        rect.bottom > source.top && rect.top < source.bottom
    } else {
        rect.right > source.left && rect.left < source.right
    }
}

fn is_in_direction(rect: Edges, source: Edges, arrow: Arrow) -> bool {
    match arrow {
        Arrow::Left => source.left >= rect.right,
        Arrow::Right => source.right <= rect.left,
        Arrow::Up => source.top >= rect.bottom,
        Arrow::Down => source.bottom <= rect.top,
    }
}

// Whether `first` wins over `second` by lying in the source's beam when
// `second` does not. Across the arrow, a beam always wins; along a vertical
// arrow, it wins unless `second` lies wholly nearer.
fn beam_beats(source: Edges, first: Edges, second: Edges, arrow: Arrow) -> bool {
    if in_beam(second, source, arrow) || !in_beam(first, source, arrow) {
        return false;
    }
    if !is_in_direction(second, source, arrow) || arrow.is_horizontal() {
        return true;
    }
    major_axis_distance(first, source, arrow)
        < major_axis_distance_to_far_edge(second, source, arrow)
}
