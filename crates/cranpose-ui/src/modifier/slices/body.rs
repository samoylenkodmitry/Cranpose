// What a node's slices draw and the text the node shows, kept as the common
// cases need: the draws alone, one draw in place of a list, or a text with
// the draws.

use super::{super::coordinator_rect::CoordinatorRect, SliceText};
use crate::draw::DrawCommand;

#[derive(Clone)]
pub(super) enum SliceBody {
    // The draws of a node that shows no text, in order.
    Draws(Vec<DrawCommand>),
    // The one draw of a node that shows no text, which takes no list.
    One(DrawCommand),
    // The text a node shows, where it draws it, and the node's draws.
    Text {
        text: SliceText,
        coordinator: CoordinatorRect,
        draws: Vec<DrawCommand>,
    },
}

impl Default for SliceBody {
    fn default() -> Self {
        Self::Draws(Vec::new())
    }
}

impl SliceBody {
    // What a collection of `draws` and `text` keeps. A list it does not
    // keep goes back to the thread's spare, which the next collection takes.
    pub(super) fn collected(
        mut draws: Vec<DrawCommand>,
        text: Option<(SliceText, CoordinatorRect)>,
    ) -> Self {
        match text {
            Some((text, coordinator)) => Self::Text {
                text,
                coordinator,
                draws: kept(draws),
            },
            None if draws.len() > 1 => Self::Draws(kept(draws)),
            None => {
                let one = draws.pop();
                spare(draws);
                one.map_or_else(Self::default, Self::One)
            }
        }
    }

    // The list a collection into this body pushes its draws to: the body's
    // own, emptied, or else the thread's spare. The body keeps nothing.
    pub(super) fn take_draws(&mut self) -> Vec<DrawCommand> {
        let mut draws = self.take_owned();
        if draws.capacity() == 0 {
            return crate::thread_state::with(|state| state.spare_draws.take());
        }
        draws.clear();
        draws
    }

    pub(super) fn draws(&self) -> &[DrawCommand] {
        match self {
            Self::Draws(draws) | Self::Text { draws, .. } => draws,
            Self::One(draw) => std::slice::from_ref(draw),
        }
    }

    // How many draws the body has room for.
    pub(super) fn capacity(&self) -> usize {
        match self {
            Self::Draws(draws) | Self::Text { draws, .. } => draws.capacity(),
            Self::One(_) => 1,
        }
    }

    pub(super) fn text(&self) -> Option<&SliceText> {
        match self {
            Self::Text { text, .. } => Some(text),
            Self::Draws(_) | Self::One(_) => None,
        }
    }

    pub(super) fn text_coordinator(&self) -> Option<&CoordinatorRect> {
        match self {
            Self::Text { coordinator, .. } => Some(coordinator),
            Self::Draws(_) | Self::One(_) => None,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        matches!(self, Self::Draws(draws) if draws.is_empty())
    }

    // Leaves no draws and no text, keeping the room of a list it holds.
    pub(super) fn clear(&mut self) {
        let mut draws = self.take_owned();
        draws.clear();
        *self = Self::Draws(draws);
    }

    // The body's own list, or an empty one.
    fn take_owned(&mut self) -> Vec<DrawCommand> {
        match std::mem::take(self) {
            Self::Draws(draws) | Self::Text { draws, .. } => draws,
            Self::One(_) => Vec::new(),
        }
    }
}

// `draws` as a body keeps them: in a list of their exact length, which a
// list collected into once again already is. A longer list goes back to the
// thread's spare.
fn kept(mut draws: Vec<DrawCommand>) -> Vec<DrawCommand> {
    if draws.len() == draws.capacity() {
        return draws;
    }
    let mut exact = Vec::with_capacity(draws.len());
    exact.append(&mut draws);
    spare(draws);
    exact
}

// The most draws the thread's spare list keeps room for: a chain with more
// gets a list of its own, which goes when its collection ends.
const SPARE_DRAWS: usize = 8;

// Gives the room of an emptied `draws` to the thread's spare, unless the
// spare holds as much room already or `draws` has room for more than
// `SPARE_DRAWS`.
fn spare(draws: Vec<DrawCommand>) {
    if draws.capacity() > SPARE_DRAWS {
        return;
    }
    crate::thread_state::with(|state| {
        let held = state.spare_draws.take();
        state
            .spare_draws
            .set(if held.capacity() >= draws.capacity() {
                held
            } else {
                draws
            });
    });
}
