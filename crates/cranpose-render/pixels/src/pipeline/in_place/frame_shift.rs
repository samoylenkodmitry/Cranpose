//! A scroll as one move. A translated container whose content a rigid snap
//! carries lies on whole pixels: when only the offset it moves its children
//! by changes, each item of its subtree moves by the whole pixels between the
//! snaps of the anchor the items hold and the anchor now. The update moves
//! the subtree's items in place, in place of pushing every item of the
//! subtree again.

use cranpose_core::{RecordId, RecordRef};
use cranpose_render_common::scene_store::{ItemKind, LayerRef, SceneStore};
use cranpose_ui_graphics::{Point, Rect};

use super::Repush;
use crate::{
    damage::{ItemReach, move_item},
    frame::Frame,
    pipeline::{RasterTraversalContext, UpdatedReach, layer_raster_mapping, layer_snap_anchors},
    scene::{RasterScene, SubtreeItems},
    scene_item::ItemRef,
    shift::Shift,
};

// The container of the frame that holds the record at `place`, beneath it.
#[inline]
pub(super) fn frame_above(
    scene: &RasterScene,
    store: &SceneStore,
    place: usize,
) -> Option<RecordId> {
    if scene.frames.is_empty() {
        return None;
    }
    frame_containing(scene, store, place)
}

#[inline(never)]
fn frame_containing(scene: &RasterScene, store: &SceneStore, place: usize) -> Option<RecordId> {
    scene.frames.records().find_map(|frame| {
        let container = scene.route(frame)?;
        let beneath = container < place && place < scene.subtrees[container].end;
        beneath.then(|| store.resolve(frame)).flatten()
    })
}

// Whether the record `id`, whose subtree the scene pushed as `old` says,
// can move as a frame: it pushed as one, it changed only the offset it moves
// its children by, and the store changed no record beneath it, `stamps` being
// the records it changed.
#[inline]
pub(super) fn can_shift(
    scene: &RasterScene,
    store: &SceneStore,
    (old, id): (&SubtreeItems, RecordId),
    stamps: &[RecordRef],
) -> bool {
    store.layer(id).translated_content_context()
        && can_shift_container(scene, store, (old, id), stamps)
}

#[inline(never)]
fn can_shift_container(
    scene: &RasterScene,
    store: &SceneStore,
    (old, id): (&SubtreeItems, RecordId),
    stamps: &[RecordRef],
) -> bool {
    old.passed_down
        .and_then(|at| scene.passed_down.get(at))
        .is_some_and(|passed| passed.framed)
        && store.layer(id).only_translated_since(old.changes)
        && !stamped_beneath(store, stamps, id)
}

// Whether any of `stamps` names a record beneath `container`.
fn stamped_beneath(store: &SceneStore, stamps: &[RecordRef], container: RecordId) -> bool {
    stamps
        .iter()
        .filter_map(|&stamped| store.resolve(stamped))
        .any(|stamped| {
            let mut above = store.parent(stamped);
            while let Some(record) = above {
                if record == container {
                    return true;
                }
                above = store.parent(record);
            }
            false
        })
}

// Where a push of `layer` would put its content's snap, with the transform
// that places it. Out of line: the push walk compiles the two calls inline.
#[inline(never)]
fn pushed_anchor(
    layer: LayerRef<'_>,
    context: &RasterTraversalContext<'_>,
) -> (
    cranpose_render_common::graph::ProjectiveTransform,
    Option<Point>,
) {
    let transform = layer.placement().then(*context.parent_transform);
    let mapping = layer_raster_mapping(layer, layer.graphics_layer(), &transform, context);
    let content_layer = mapping.content_layer();
    let (anchor, _) = layer_snap_anchors(layer, context, &transform, &mapping, &content_layer);
    (transform, anchor)
}

impl Repush<'_> {
    // Moves the items of the subtree of `record`, the container whose record
    // is at `place`, to where its content offset now puts them, in place of
    // pushing them again, and adds the areas that differ. `false`, with
    // nothing changed, when the subtree cannot be a frame: it pushes again.
    #[inline(never)]
    pub(super) fn shift_frame(
        &mut self,
        store: &SceneStore,
        record: RecordId,
        place: usize,
        parent: Option<usize>,
    ) -> bool {
        let layer = store.layer(record);
        let head = self.scene.subtrees[place];
        let Some(passed) = head
            .passed_down
            .and_then(|at| self.scene.passed_down.get(at))
        else {
            return false;
        };
        let Some(pushed) = passed.snap_anchor.filter(|_| passed.framed) else {
            return false;
        };
        let (pushed_transform, clip) = (passed.transform, passed.clip);
        if layer.translated_content_offset() != layer.content_offset() {
            return false;
        }
        let above = parent.and_then(|at| self.scene.passed_down.get(at));
        let inherited = self.inherited(above);
        let context = self.context(above, &inherited);
        let (transform, anchor) = pushed_anchor(layer, &context);
        let Some(anchor) = anchor.filter(|_| transform == pushed_transform) else {
            return false;
        };
        let frame_ref = store.record_ref(record);
        let held = self.scene.frames.position(frame_ref);
        let from = held.map_or(pushed, |position| self.scene.frames.anchor(position));
        let Some((shift, moved)) = Shift::snapping(from, anchor) else {
            return false;
        };
        let position = match held {
            Some(position) => position,
            None if holds_only_content(self.scene, &head, layer, (pushed, clip)) => {
                self.scene.frames.add(Frame {
                    record: frame_ref,
                    anchor: pushed,
                })
            }
            None => return false,
        };
        if !shift.is_none() {
            for index in head.start.order..head.stop.order {
                let item = self.scene.order[index];
                let held = match self.reach.get_mut(index) {
                    Some(held) if self.held == UpdatedReach::Held => ItemReach::Replaced(held),
                    _ => ItemReach::Unknown,
                };
                move_item(self.scene, item, shift, held, self.window, self.areas);
            }
            self.scene.frames.set_anchor(position, moved);
        }
        true
    }

    // Forgets the frames whose container lies in the subtree of the record at
    // `place`, which pushes again.
    #[inline(never)]
    pub(super) fn forget_frames_within(&mut self, place: usize) {
        let end = self.scene.subtrees[place].end;
        let mut frames = std::mem::take(&mut self.scene.frames);
        frames.retain(|record| {
            !self
                .scene
                .route(record)
                .is_some_and(|container| place <= container && container < end)
        });
        self.scene.frames = frames;
    }
}

// Whether the items of the subtree whose record is `head`, container
// `layer`, move together as one frame: the container draws nothing of its
// own, and each item takes the snap `anchor` and the `clip` the container
// passed down, so that each lies where the snap of the offset puts it. A
// shadow follows the light and an image that turns goes by its own
// transform.
fn holds_only_content(
    scene: &RasterScene,
    head: &SubtreeItems,
    layer: LayerRef<'_>,
    (anchor, clip): (Point, Option<Rect>),
) -> bool {
    let items = head.start.order..head.stop.order;
    !items.is_empty()
        && head.start.shadows == head.stop.shadows
        && layer.item_kinds().all(|kind| kind == ItemKind::Child)
        && scene.order[items]
            .iter()
            .all(|&item| follows_snap_and_clip(scene.item(item), anchor, clip))
}

// Whether `item` takes the snap `anchor` and the `clip` and so moves with a
// whole frame of such items: a shadow, whose clip follows the snap, and an
// image placed by a transform do not.
fn follows_snap_and_clip(item: ItemRef<'_>, anchor: Point, clip: Option<Rect>) -> bool {
    let (held_anchor, held_clip) = match item {
        ItemRef::Shape(draw) => (draw.snap_anchor, draw.clip),
        ItemRef::Text(draw) => (draw.snap_anchor, draw.clip),
        ItemRef::Image(draw) if draw.transform.is_none() => (draw.snap_anchor, draw.clip),
        ItemRef::Image(_) | ItemRef::Shadow(_) => return false,
    };
    held_anchor == Some(anchor) && held_clip == clip
}
