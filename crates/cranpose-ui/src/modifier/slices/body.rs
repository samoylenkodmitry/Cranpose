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
    // What a collection of `draws` and `text` keeps.
    pub(super) fn collected(mut draws: Draws, text: Option<(SliceText, CoordinatorRect)>) -> Self {
        match text {
            Some((text, coordinator)) => Self::Text {
                text,
                coordinator,
                draws: listed(draws),
            },
            None if draws.len() > 1 => Self::Draws(listed(draws)),
            None => draws.pop().map_or_else(Self::default, Self::One),
        }
    }

    // What a collection into this body gathers its draws in: the body's own
    // list, emptied, when it has room for more than one. The body keeps
    // nothing.
    pub(super) fn take_draws(&mut self) -> Draws {
        let mut draws = self.take_owned();
        draws.clear();
        Draws::from_vec(draws)
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

// `draws` in a list of their exact length: the body keeps them for as long
// as the chain stays, and a collection into it again mostly pushes as many.
fn listed(draws: Draws) -> Vec<DrawCommand> {
    let mut list = draws.into_vec();
    list.shrink_to_fit();
    list
}
