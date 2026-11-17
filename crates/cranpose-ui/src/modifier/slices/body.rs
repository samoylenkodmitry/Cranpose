// What a node's slices draw and the text the node shows, kept as the common
// cases need: the draws alone, one draw in place of a list, or a text with
// the draws.

use smallvec::SmallVec;

use super::{super::coordinator_rect::CoordinatorRect, SliceText};
use crate::draw::DrawCommand;

// The draws a collection gathers: one stays in place, more go to a list.
pub(super) type Draws = SmallVec<[DrawCommand; 1]>;

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
    // What a collection of `draws` and `text` keeps. `room` is the list the
    // body held, which a list of the draws takes when it fits them.
    pub(super) fn collected(
        mut draws: Draws,
        room: Vec<DrawCommand>,
        text: Option<(SliceText, CoordinatorRect)>,
    ) -> Self {
        match text {
            Some((text, coordinator)) => Self::Text {
                text,
                coordinator,
                draws: listed(draws, room),
            },
            None if draws.len() > 1 => Self::Draws(listed(draws, room)),
            None => draws.pop().map_or_else(Self::default, Self::One),
        }
    }

    // What a collection into this body gathers its draws in, and the room it
    // keeps aside: the body's own list, emptied, gathers them when it has
    // room for more than one; a list of one waits aside for a text's draw.
    // The body keeps nothing.
    pub(super) fn take_draws(&mut self) -> (Draws, Vec<DrawCommand>) {
        let mut list = self.take_owned();
        list.clear();
        if list.capacity() > 1 {
            (Draws::from_vec(list), Vec::new())
        } else {
            (Draws::new(), list)
        }
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

// `draws` in a list of their exact length, in `room` when it fits them: the
// body keeps them for as long as the chain stays, and a collection into it
// again mostly gathers as many.
fn listed(draws: Draws, mut room: Vec<DrawCommand>) -> Vec<DrawCommand> {
    if draws.spilled() {
        let mut list = draws.into_vec();
        list.shrink_to_fit();
        return list;
    }
    if draws.is_empty() {
        return Vec::new();
    }
    if room.capacity() < draws.len() {
        room = Vec::with_capacity(draws.len());
    }
    room.extend(draws);
    room
}
