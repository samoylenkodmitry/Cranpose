//! The frames of a raster scene: containers whose items lie where a scroll
//! last moved them. A scroll that moves a container's content by whole pixels
//! moves the container's items by those pixels in place, in place of pushing
//! every item of the subtree again. The frame notes that the subtree's
//! records, passed-down entries and snap anchor still hold what the push
//! left, so that a change beneath it pushes the whole subtree again.

use cranpose_core::RecordRef;
use cranpose_ui_graphics::Point;

use crate::{draw::RenderItem, scene::RasterScene, shift::Shift};

pub(crate) struct Frame {
    // The container whose subtree the frame holds.
    pub(crate) record: RecordRef,
    // The snap anchor the subtree's items hold now.
    pub(crate) anchor: Point,
}

#[derive(Default)]
pub(crate) struct Frames {
    list: Vec<Frame>,
}

impl Frames {
    pub(crate) fn clear(&mut self) {
        self.list.clear();
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub(crate) fn records(&self) -> impl Iterator<Item = RecordRef> + '_ {
        self.list.iter().map(|frame| frame.record)
    }

    pub(crate) fn position(&self, record: RecordRef) -> Option<usize> {
        self.list.iter().position(|frame| frame.record == record)
    }

    pub(crate) fn add(&mut self, frame: Frame) -> usize {
        self.list.push(frame);
        self.list.len() - 1
    }

    pub(crate) fn anchor(&self, position: usize) -> Point {
        self.list[position].anchor
    }

    pub(crate) fn set_anchor(&mut self, position: usize, anchor: Point) {
        self.list[position].anchor = anchor;
    }

    // Keeps the frames whose container `keep` names.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(RecordRef) -> bool) {
        self.list.retain(|frame| keep(frame.record));
    }
}

// Moves `item` of `scene` by `shift`.
pub(crate) fn shift_item(scene: &mut RasterScene, item: RenderItem, shift: Shift) {
    match item {
        RenderItem::Shape(at) => scene.shapes[at].shift_by(shift),
        RenderItem::Image(at) => scene.images[at].shift_by(shift),
        RenderItem::Text(at) => scene.texts[at].shift_by(shift),
        // A frame holds no shadow: taking one into a frame is refused.
        RenderItem::Shadow(_) => {}
    }
}
