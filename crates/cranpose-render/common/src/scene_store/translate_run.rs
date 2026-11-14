// The updates in a row whose only change to a translated record was the
// offset by which it moves its content, as a renderer that holds the record
// pushed, whose pixels follow from the offset alone, reads them.

use cranpose_core::SceneUpdate;

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct TranslateRun {
    // The content revision before the run's first update, and its last
    // update, while the run lasts.
    span: Option<(SceneUpdate, SceneUpdate)>,
}

impl TranslateRun {
    // Notes that `update` changed the record's content only by its offset,
    // which `held` was the content revision of before.
    pub(crate) fn note(&mut self, held: SceneUpdate, update: SceneUpdate) {
        self.span = match self.span {
            Some((first, last)) if last == held => Some((first, update)),
            _ => Some((held, update)),
        };
    }

    // Whether the record's content, which changed last in the update `now`,
    // changed after the update `read` only by its offset.
    pub(crate) fn covers(self, read: SceneUpdate, now: SceneUpdate) -> bool {
        self.span
            .is_some_and(|(first, last)| last == now && !first.is_after(read))
    }
}
