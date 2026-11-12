// The layout nodes a store stopped drawing through a record, as its journal
// tells: a node may have taken another record since, which the store's own
// table of node records tells.

use cranpose_core::NodeId;

use crate::scene_store::{JournalRead, SceneStore, SceneVersion};

#[derive(Default)]
pub(crate) struct LeftNodes {
    // The version of the store read last.
    read: Option<SceneVersion>,
}

impl LeftNodes {
    // Adds to `left` the nodes whose records `store` removed or released
    // since the last read. `false` when the store cannot tell.
    pub(crate) fn read(&mut self, store: &SceneStore, left: &mut Vec<NodeId>) -> bool {
        let now = store.version();
        // Most frames read a store no update changed.
        if self.read == Some(now) {
            return true;
        }
        let read = self.read.replace(now);
        match store.changes_since(read) {
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
        }
    }
}
