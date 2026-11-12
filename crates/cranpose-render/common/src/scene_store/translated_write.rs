// The write that ends a translate step: what a translated container holds
// once its children moved, in one call. One props edit takes the place of
// one for its translated offset and one for its pointer handling.

use std::rc::Rc;

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
        let mut same_handlers = true;
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
                same_handlers = match (&props.hit_test, &hit_test) {
                    (Some(held), Some(now)) => Rc::ptr_eq(&held.handlers, &now.handlers),
                    (None, None) => true,
                    (Some(_), None) | (None, Some(_)) => false,
                };
                props.hit_test = hit_test;
            });
        }
        // The content change noted below leaves the container's containment
        // and reach to be worked out again.
        let items_replaced = items.is_some_and(|items| self.replace_items(id, items));
        let flags = self.tree[index].flags;
        let flags = if origin_sinks {
            flags.with(RecordFlags::ORIGIN_SINKS)
        } else {
            flags.without(RecordFlags::ORIGIN_SINKS)
        };
        let flags_written = self.write_flag_row(index, flags);
        if flags_written && let Some(parent) = self.tree[index].parent {
            self.stale(parent.index());
        }
        // A container that draws nothing of its own and keeps its children,
        // handlers and flags changes only in the offset it moves them by.
        if translated_content_offset.is_some()
            && same_handlers
            && !items_replaced
            && !flags_written
            && self.items(id).iter().all(|item| item.as_child().is_some())
        {
            self.note_translate_step(id);
        }
        self.note_content_change(id);
    }

    // Notes that the update in progress changed the content of `id`, which
    // keeps its props, only by its offset.
    fn note_translate_step(&mut self, id: RecordId) {
        let index = id.index();
        let held = self.tree[index].content_rev;
        let update = self.update();
        if let Some(entry) = self.warm[index].props {
            self.props[entry.index()].translate_run.note(held, update);
        }
    }
}
