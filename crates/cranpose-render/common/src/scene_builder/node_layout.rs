use cranpose_ui::{Point, Size, widgets::LayoutState};

// What lowering reads of a node's layout state: read field by field, as
// the whole state is several times larger and a move of it is a call to
// `memcpy` on the chips.
#[derive(Clone, Copy)]
pub(super) struct NodeLayout {
    size: Size,
    position: Point,
    content_offset: Point,
    placed: bool,
}

impl NodeLayout {
    pub(super) fn of(state: &LayoutState) -> Self {
        Self {
            size: state.size(),
            position: state.position(),
            content_offset: state.content_offset(),
            placed: state.is_placed(),
        }
    }

    pub(super) fn size(&self) -> Size {
        self.size
    }

    pub(super) fn position(&self) -> Point {
        self.position
    }

    pub(super) fn content_offset(&self) -> Point {
        self.content_offset
    }

    pub(super) fn is_placed(&self) -> bool {
        self.placed
    }

    // The layout of a window's root, which draws at its window's origin.
    pub(super) fn at_origin(self) -> Self {
        Self {
            position: Point::default(),
            ..self
        }
    }
}
