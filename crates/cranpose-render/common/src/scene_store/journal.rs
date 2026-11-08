// The changes of a store's records, kept until every reader that follows
// them has read them.

use cranpose_core::{NodeId, RecordId, RecordRef, SceneUpdate, StoreId};

use super::RecordKind;

/// What an update did to a record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordEvent {
    /// The record was made, drawing for its node.
    Inserted,
    /// What the record itself draws changed: its draws, properties or
    /// children.
    Changed,
    /// The record moved in its parent.
    Moved,
    /// The record and the subtree beneath it went.
    Removed,
    /// The record stopped drawing for the node the entry names, or as the
    /// kind the entry names.
    Released,
    /// The record started drawing for the node the entry names, or as the
    /// kind the entry names.
    Claimed,
}

impl RecordEvent {
    const fn bit(self) -> u8 {
        1 << self as u8
    }

    /// Whether the entry's node draws through the record from now on.
    pub fn joins(self) -> bool {
        matches!(self, Self::Inserted | Self::Claimed)
    }

    /// Whether the entry's node no longer draws through the record.
    pub fn leaves(self) -> bool {
        matches!(self, Self::Removed | Self::Released)
    }
}

/// The kinds of change a journal reader follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordEvents(u8);

impl RecordEvents {
    /// Every change.
    pub const ALL: Self = Self(u8::MAX);
    /// The changes that give a node a record or take its record away:
    /// [`RecordEvent::Inserted`], [`RecordEvent::Removed`],
    /// [`RecordEvent::Released`] and [`RecordEvent::Claimed`].
    pub const OWNERS: Self = Self(
        RecordEvent::Inserted.bit()
            | RecordEvent::Removed.bit()
            | RecordEvent::Released.bit()
            | RecordEvent::Claimed.bit(),
    );

    /// Whether the set holds `event`.
    pub fn contains(self, event: RecordEvent) -> bool {
        self.0 & event.bit() != 0
    }
}

/// One change to one record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    /// The update that made the change.
    pub update: SceneUpdate,
    /// What the change did.
    pub event: RecordEvent,
    /// What the record draws for in the change.
    pub kind: RecordKind,
    /// The record, with the generation its row had in the change.
    pub record: RecordRef,
    /// The record that held it.
    pub parent: Option<RecordId>,
    /// The layout node the record draws for in the change.
    pub node: Option<NodeId>,
}

/// A reader of a store's journal. Each reader reads every change once.
#[derive(Debug)]
pub struct JournalCursor {
    store: StoreId,
    slot: usize,
    events: RecordEvents,
}

/// What a reader learns from a read. The changes hold every kind of change
/// some open reader follows, so a reader skips the kinds it does not.
#[derive(Debug)]
pub enum JournalRead<'a> {
    /// The changes since the reader's last read, oldest first.
    Changes(&'a [JournalEntry]),
    /// The changes are not known: the store was cleared or replaced since
    /// the last read, or the reader fell further behind than the journal
    /// keeps. The reader works from the store as it is.
    Lost,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Journal {
    entries: Vec<JournalEntry>,
    // The sequence number of the first entry.
    start: u64,
    // The sequence number each open reader reads next, and the changes it
    // follows.
    readers: Vec<Option<(u64, RecordEvents)>>,
    // The changes some open reader follows.
    wanted: u8,
    // An update that cleared the store: what it writes after is left out,
    // as every reader has lost the store's earlier state anyway.
    cleared: Option<SceneUpdate>,
}

impl Journal {
    fn end(&self) -> u64 {
        self.start + self.entries.len() as u64
    }

    // Whether an entry of `event` in `update` is kept: an open reader
    // follows it and the update did not clear the store.
    pub(super) fn wants(&self, event: RecordEvent, update: SceneUpdate) -> bool {
        self.wanted & event.bit() != 0 && self.cleared != Some(update)
    }

    pub(super) fn log(&mut self, entry: JournalEntry) {
        self.entries.push(entry);
    }

    // Forgets every entry: each reader's next read finds its changes lost.
    pub(super) fn clear(&mut self, update: SceneUpdate) {
        self.start = self.end() + 1;
        self.entries.clear();
        self.cleared = Some(update);
    }

    // Drops the entries every reader has read, and the oldest beyond
    // `keep`, whose readers lose them.
    pub(super) fn trim(&mut self, keep: usize) {
        let read = self.readers.iter().flatten().map(|&(next, _)| next).min();
        let read = read.map_or(self.entries.len(), |next| {
            usize::try_from(next.saturating_sub(self.start)).unwrap_or(usize::MAX)
        });
        let drop = read
            .max(self.entries.len().saturating_sub(keep))
            .min(self.entries.len());
        self.entries.drain(..drop);
        self.start += drop as u64;
    }

    pub(super) fn open(&mut self, store: StoreId, events: RecordEvents) -> JournalCursor {
        let next = Some((self.end(), events));
        let slot = match self.readers.iter().position(Option::is_none) {
            Some(slot) => {
                self.readers[slot] = next;
                slot
            }
            None => {
                self.readers.push(next);
                self.readers.len() - 1
            }
        };
        self.refresh_wanted();
        JournalCursor {
            store,
            slot,
            events,
        }
    }

    pub(super) fn close(&mut self, cursor: &JournalCursor) {
        if let Some(reader) = self.readers.get_mut(cursor.slot) {
            *reader = None;
        }
        self.refresh_wanted();
    }

    fn refresh_wanted(&mut self) {
        self.wanted = self
            .readers
            .iter()
            .flatten()
            .fold(0, |wanted, &(_, events)| wanted | events.0);
    }

    pub(super) fn read(&mut self, store: StoreId, cursor: &mut JournalCursor) -> JournalRead<'_> {
        if cursor.store != store {
            *cursor = self.open(store, cursor.events);
            return JournalRead::Lost;
        }
        let end = self.end();
        let Some(Some((next, events))) = self.readers.get(cursor.slot).copied() else {
            *cursor = self.open(store, cursor.events);
            return JournalRead::Lost;
        };
        self.readers[cursor.slot] = Some((end, events));
        if next < self.start {
            return JournalRead::Lost;
        }
        let first = usize::try_from(next - self.start).unwrap_or(usize::MAX);
        JournalRead::Changes(&self.entries[first.min(self.entries.len())..])
    }
}
