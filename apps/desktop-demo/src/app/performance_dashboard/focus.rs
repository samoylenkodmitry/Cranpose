// What the pointer and the clicks of a card pick out among its subjects, by
// their indices in the run, for its chart, its bars and its names alike.

use cranpose_core::MutableState;
use cranpose_ui::Color;

// How much of its colour a subject keeps while another one is hovered, and
// while the bars picked leave it out.
pub(super) const DIMMED_WEIGHT: f32 = 0.3;
const OFF_WEIGHT: f32 = 0.45;

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Focus {
    // The subject the pointer is over: in colour, the others dimmed.
    pub(super) hovered: MutableState<Option<usize>>,
    // The subjects whose bars were picked: while any, the chart draws only
    // them.
    pub(super) picked: MutableState<Vec<usize>>,
}

impl Focus {
    pub(super) fn remember() -> Self {
        cranpose_core::remember(|| Self {
            hovered: cranpose_core::mutableStateOf(None),
            picked: cranpose_core::mutableStateOf(Vec::new()),
        })
        .with(|focus| *focus)
    }

    // How much of its colour the row of `subject` keeps. Reads the state it
    // follows.
    pub(super) fn weight(&self, subject: usize) -> f32 {
        let picked = self.picked.get();
        let off = !picked.is_empty() && !picked.contains(&subject);
        match self.hovered.get() {
            Some(hovered) if hovered == subject => 1.0,
            Some(_) if off => OFF_WEIGHT * DIMMED_WEIGHT,
            Some(_) => DIMMED_WEIGHT,
            None if off => OFF_WEIGHT,
            None => 1.0,
        }
    }

    pub(super) fn toggle(&self, subject: usize) {
        self.picked.update(
            |picked| match picked.iter().position(|picked| *picked == subject) {
                Some(at) => {
                    picked.remove(at);
                }
                None => picked.push(subject),
            },
        );
    }

    pub(super) fn clear(&self) {
        self.picked.set(Vec::new());
    }
}

// `color` at `weight` of its own opacity.
pub(super) fn faded(color: Color, weight: f32) -> Color {
    color.with_alpha(color.3 * weight)
}
