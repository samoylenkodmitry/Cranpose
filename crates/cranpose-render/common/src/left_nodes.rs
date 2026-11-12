// The layout nodes a store stopped drawing through a record, as its journal
// tells: a node may have taken another record since, which the store's own
// table of node records tells.

use cranpose_core::NodeId;

use crate::scene_store::{JournalCursor, JournalRead, RecordEvents, SceneStore};

pub(crate) struct LeftNodes {
    cursor: JournalCursor,
}

impl LeftNodes {
    pub(crate) fn new(store: &mut SceneStore) -> Self {
        Self {
            cursor: store.open_journal(RecordEvents::LEAVES),
        }
    }

    // Adds to `left` the nodes whose records `store` removed or released
    // since the last read. `false` when the store cannot tell.
    pub(crate) fn read(&mut self, store: &mut SceneStore, left: &mut Vec<NodeId>) -> bool {
        // Most frames remove no record.
        if store.journal_caught_up(&self.cursor) {
            return true;
        }
        store.read_journal(&mut self.cursor, |_, read| match read {
            JournalRead::Changes(entries) => {
                left.extend(
                    entries
                        .iter()
                        .filter(|entry| entry.event.leaves())
                        .filter_map(|entry| entry.node),
                );
                true
            }
            JournalRead::Lost => false,
        })
    }
}
