// The changes of a store's records, kept until every reader that follows
// them has read them.

use cranpose_core::{
    NodeId, RecordRef, SceneUpdate, StoreId, collections::recent_peak::RecentPeak,
};

use super::{Reach, RecordKind};

/// What an update did to a record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RecordEvent {
    /// The record was made, drawing for its node.
    Inserted,
    /// The record's properties changed, or the order of the children it
    /// keeps: all it draws may change.
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
    /// What the record draws of its own, beside its children, changed
    /// within the reach, in the record's space.
    Redrawn(Reach),
}

impl RecordEvent {
    const fn bit(self) -> u16 {
        1 << match self {
            Self::Inserted => 0,
            Self::Changed => 1,
            Self::Moved => 2,
            Self::Removed => 3,
            Self::Released => 4,
            Self::Claimed => 5,
            Self::Redrawn(_) => 6,
        }
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

// How many entries a record row and a store keep past the slowest reader:
// a reader further behind rebuilds what it follows from the store.
const ROWS_KEPT: usize = 4;
const ENTRIES_KEPT: usize = 256;

// The bit of a set of changes that asks for the reach of each redraw.
const REDRAWN_REACH: u16 = 1 << 7;
// The bit of a set of changes that asks whether an update changed anything.
const ANY_CHANGE: u16 = 1 << 8;

/// The kinds of change a journal reader follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordEvents(u16);

impl RecordEvents {
    /// Every change.
    pub const ALL: Self = Self(u16::MAX);
    /// Whether an update changed anything: the first change of each update,
    /// whatever its kind, for a reader that only tells an update that
    /// changes nothing from one that changes something. The store spends
    /// nothing on the other changes for it.
    pub const ANY: Self = Self(ANY_CHANGE);
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
#[derive(Clone, Copy, Debug, PartialEq)]
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
    pub parent: Option<RecordRef>,
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
    wanted: u16,
    // The update that logged an entry last.
    logged: Option<SceneUpdate>,
    // An update that cleared the store: what it writes after is left out,
    // as every reader has lost the store's earlier state anyway.
    cleared: Option<SceneUpdate>,
    // The most entries a trim found lately.
    recent: RecentPeak,
}

impl Journal {
    fn end(&self) -> u64 {
        self.start + self.entries.len() as u64
    }

    // Whether an entry of `event` in `update` is kept: the update did not
    // clear the store, and an open reader follows such entries or learns
    // only whether the update changed anything, which no entry told yet.
    #[inline]
    pub(super) fn wants(&self, event: RecordEvent, update: SceneUpdate) -> bool {
        self.cleared != Some(update)
            && (self.follows(event)
                || (self.wanted & ANY_CHANGE != 0 && self.logged != Some(update)))
    }

    // Whether an open reader follows each change of the kind of `event`.
    #[inline]
    pub(super) fn follows(&self, event: RecordEvent) -> bool {
        self.wanted & event.bit() != 0
    }

    // Whether an open reader follows where the records `update` redraws
    // changed.
    #[inline]
    pub(super) fn wants_redrawn_reach(&self, update: SceneUpdate) -> bool {
        self.wanted & REDRAWN_REACH != 0 && self.wants(RecordEvent::Redrawn(Reach::Empty), update)
    }

    pub(super) fn log(&mut self, entry: JournalEntry) {
        self.logged = Some(entry.update);
        self.entries.push(entry);
    }

    // Forgets every entry: each reader's next read finds its changes lost.
    pub(super) fn clear(&mut self, update: SceneUpdate) {
        self.start = self.end() + 1;
        self.entries.clear();
        self.cleared = Some(update);
    }

    // Drops the entries every reader has read, and the oldest beyond what a
    // store of `rows` record rows keeps, whose readers lose them.
    pub(super) fn trim(&mut self, rows: usize) {
        let keep = ROWS_KEPT * rows + ENTRIES_KEPT;
        let read = self.readers.iter().flatten().map(|&(next, _)| next).min();
        let read = read.map_or(self.entries.len(), |next| {
            usize::try_from(next.saturating_sub(self.start)).unwrap_or(usize::MAX)
        });
        let drop = read
            .max(self.entries.len().saturating_sub(keep))
            .min(self.entries.len());
        self.recent.note(self.entries.len());
        self.entries.drain(..drop);
        self.start += drop as u64;
        self.recent.shrink(&mut self.entries, ENTRIES_KEPT);
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

    // Makes the reader of `cursor` follow `events`; a reader of another
    // store follows them once its next read opens it here.
    pub(super) fn follow(
        &mut self,
        store: StoreId,
        cursor: &mut JournalCursor,
        events: RecordEvents,
    ) {
        cursor.events = events;
        if cursor.store != store {
            return;
        }
        if let Some(Some((_, followed))) = self.readers.get_mut(cursor.slot) {
            *followed = events;
        }
        self.refresh_wanted();
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

    pub(super) fn changes(&self, store: StoreId, cursor: &JournalCursor) -> JournalRead<'_> {
        let Some(Some((next, _))) = self.readers.get(cursor.slot).copied() else {
            return JournalRead::Lost;
        };
        if cursor.store != store || next < self.start {
            return JournalRead::Lost;
        }
        let first = usize::try_from(next - self.start).unwrap_or(usize::MAX);
        JournalRead::Changes(&self.entries[first.min(self.entries.len())..])
    }

    pub(super) fn mark_read(&mut self, store: StoreId, cursor: &mut JournalCursor) {
        let end = self.end();
        match self.readers.get_mut(cursor.slot) {
            Some(Some((next, _))) if cursor.store == store => *next = end,
            _ => *cursor = self.open(store, cursor.events),
        }
    }
}
