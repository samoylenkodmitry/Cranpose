// The records an update stamped: the ones it made, changed themselves or
// moved. A reader that holds the scene of the update before brings itself up
// to this one from them, with no walk of the records that held still.

use cranpose_core::{RecordRef, SceneUpdate};

// The most records an update lists: past them a reader builds its scene
// again, as it would walk most of the store anyway.
const MOST: usize = 256;

#[derive(Clone, Debug, Default)]
pub(super) struct Touched {
    // The update before the one the records are of.
    since: SceneUpdate,
    records: Vec<RecordRef>,
    // Whether the update stamped more records than the list keeps, or put
    // another scene in the store's place.
    lost: bool,
}

impl Touched {
    // Starts the list of the update after `since`.
    pub(super) fn begin(&mut self, since: SceneUpdate) {
        self.since = since;
        self.records.clear();
        self.lost = false;
    }

    // A record may enter twice, when an update writes its items and then
    // stamps it: a reader pushes it once either way.
    #[inline]
    pub(super) fn note(&mut self, record: RecordRef) {
        if self.records.len() < MOST {
            self.records.push(record);
        } else {
            self.lost = true;
        }
    }

    // Leaves the update's records unknown: its readers build their scenes
    // again.
    pub(super) fn lose(&mut self) {
        self.lost = true;
    }

    pub(super) fn read(&self) -> Option<(SceneUpdate, &[RecordRef])> {
        (!self.lost).then_some((self.since, self.records.as_slice()))
    }
}
