// The write that ends a translate step: what a translated container holds
// once its children moved, in one call. One props edit takes the place of
// one for its translated offset and one for its pointer handling.

use cranpose_core::RecordId;
use cranpose_ui::Point;

use super::{Item, RecordFlags, SceneStore};
use crate::graph::HitTestNode;

pub(crate) struct TranslatedWrite<'a> {
    // `None` for a container outside a translated content context.
    pub(crate) translated_content_offset: Option<Point>,
    // `None` when the step kept the container's items.
    pub(crate) items: Option<&'a [Item]>,
    pub(crate) hit_test: Option<HitTestNode>,
    pub(crate) origin_sinks: bool,
}

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl SceneStore {
    // Writes `write` into `id`, whose content the update in progress
    // changed. The draws and children no longer among its items stay in the
    // store: the caller removes them.
    pub(crate) fn write_translated(&mut self, id: RecordId, write: TranslatedWrite<'_>) {
        let TranslatedWrite {
            translated_content_offset,
            items,
            hit_test,
            origin_sinks,
        } = write;
        let index = id.index();
        // A container outside a translated content context with no pointer
        // handling, before or after, edits no props.
        if translated_content_offset.is_some()
            || hit_test.is_some()
            || self.props(id).is_some_and(|props| props.hit_test.is_some())
        {
            self.edit_props(id, |props| {
                if let Some(offset) = translated_content_offset {
                    props.translated_content_offset = offset;
                }
                props.hit_test = hit_test;
            });
        }
        // The content change noted below leaves the container's containment
        // and reach to be worked out again.
        if let Some(items) = items {
            self.replace_items(id, items);
        }
        let flags = self.tree[index].flags;
        let flags = if origin_sinks {
            flags.with(RecordFlags::ORIGIN_SINKS)
        } else {
            flags.without(RecordFlags::ORIGIN_SINKS)
        };
        if self.write_flag_row(index, flags)
            && let Some(parent) = self.tree[index].parent
        {
            self.stale(parent.index());
        }
        self.note_content_change(id);
    }
}
