// The changes of a store's records in its latest updates, read by update
// number: a reader keeps the version of the store it read last and reads
// the changes of the updates after it. A reader further behind than the
// journal keeps works from the store as it is.

use std::ops::BitOr;

use cranpose_core::{NodeId, RecordRef, SceneUpdate};

use super::{
    Reach,
    update_log::{UpdateLog, UpdatesKept},
};

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
    /// The record stopped drawing for the node the entry names, or as what
    /// it drew.
    Released,
    /// The record started drawing for the node the entry names, or as what
    /// it draws now.
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

    /// Whether the entry's node no longer draws through the record.
    pub fn leaves(self) -> bool {
        matches!(self, Self::Removed | Self::Released)
    }
}

// The updates whose entries the journal keeps, and the most entries a record
// row and a store keep: their readers read each frame. Past them a reader
// works out what it follows again, as it would walk most of the store
// anyway.
const ENTRY_UPDATES: UpdatesKept = UpdatesKept { most: 8, kept: 2 };
const ROWS_KEPT: usize = 4;
const ENTRIES_KEPT: usize = 256;
// The updates whose stamps the journal keeps, and the most records it keeps
// and an update stamps: a reader of stamps may lag by a few pictures.
const STAMP_UPDATES: UpdatesKept = UpdatesKept { most: 32, kept: 8 };
const MOST_STAMPS: usize = 256;

// The bit of a set of changes that asks for the reach of each redraw.
#[cfg(feature = "window-damage")]
const REDRAWN_REACH: u16 = 1 << 7;
// The bit of a set of changes that asks for the records each update stamped.
const STAMPED: u16 = 1 << 8;

/// The kinds of change a store's journal keeps for its readers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecordEvents(u16);

impl RecordEvents {
    /// No change.
    pub const NONE: Self = Self(0);
    /// Every change as an entry.
    pub const ALL: Self = Self(!STAMPED);
    /// The changes that take a node's record away: [`RecordEvent::Removed`]
    /// and [`RecordEvent::Released`].
    pub const LEAVES: Self = Self(RecordEvent::Removed.bit() | RecordEvent::Released.bit());
    /// The records each update made, changed or moved, as handles alone.
    pub const STAMPS: Self = Self(STAMPED);

    /// Whether the set holds `event`.
    pub fn contains(self, event: RecordEvent) -> bool {
        self.0 & event.bit() != 0
    }
}

impl BitOr for RecordEvents {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// One change to one record.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JournalEntry {
    /// The update that made the change.
    pub update: SceneUpdate,
    /// What the change did.
    pub event: RecordEvent,
    /// The record, with the generation its row had in the change.
    pub record: RecordRef,
    /// The record that held it.
    pub parent: Option<RecordRef>,
    /// The layout node the record draws for in the change.
    pub node: Option<NodeId>,
}

/// What a reader learns from a read.
#[derive(Clone, Copy, Debug)]
pub enum JournalRead<'a> {
    /// The changes of the updates after the reader's version, oldest first,
    /// of the kinds the store follows.
    Changes(&'a [JournalEntry]),
    /// The changes are not known: the store was cleared or replaced since
    /// the reader's version, or the reader fell further behind than the
    /// journal keeps. The reader works from the store as it is.
    Lost,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Journal {
    entries: UpdateLog<JournalEntry>,
    // The records each update made, changed or moved, while the store
    // follows the stamps.
    stamps: UpdateLog<RecordRef>,
    followed: RecordEvents,
    // The latest update that changed a record, whatever the store follows.
    changed: SceneUpdate,
    // An update that cleared the store: what it writes after is left out,
    // as every reader has lost the store's earlier state anyway.
    cleared: Option<SceneUpdate>,
}

impl Default for RecordEvents {
    fn default() -> Self {
        Self::NONE
    }
}

impl Journal {
    // Whether an entry of `event` in `update` is kept: the update did not
    // clear the store, and the store follows such entries.
    #[inline]
    pub(super) fn wants(&self, event: RecordEvent, update: SceneUpdate) -> bool {
        self.cleared != Some(update) && self.follows(event)
    }

    // Notes that `update` changed a record.
    #[inline]
    pub(super) fn note_change(&mut self, update: SceneUpdate) {
        self.changed = update;
    }

    // Whether an update after `read` changed a record.
    #[inline]
    pub(super) fn changed_since(&self, read: SceneUpdate) -> bool {
        self.changed.is_after(read)
    }

    // Whether the store follows each change of the kind of `event`.
    #[inline]
    pub(super) fn follows(&self, event: RecordEvent) -> bool {
        self.followed.0 & event.bit() != 0
    }

    // Whether the store follows where the records `update` redraws changed.
    #[cfg(feature = "window-damage")]
    #[inline]
    pub(super) fn wants_redrawn_reach(&self, update: SceneUpdate) -> bool {
        self.followed.0 & REDRAWN_REACH != 0
            && self.wants(RecordEvent::Redrawn(Reach::Empty), update)
    }

    pub(super) fn followed(&self) -> RecordEvents {
        self.followed
    }

    // Keeps the changes of the kinds in `events` from now on. Stamps kept
    // before a time the store did not follow them leave a gap: they go.
    pub(super) fn follow(&mut self, events: RecordEvents) {
        if !self.follows_stamps() {
            self.stamps.clear();
        }
        self.followed = events;
    }

    // Starts the changes of `update`, forgetting the oldest beyond what a
    // store of `rows` record rows keeps.
    pub(super) fn begin(&mut self, update: SceneUpdate, rows: usize) {
        self.entries
            .begin(update, ENTRY_UPDATES, ROWS_KEPT * rows + ENTRIES_KEPT);
        if self.follows_stamps() {
            self.stamps.begin(update, STAMP_UPDATES, MOST_STAMPS);
        }
    }

    pub(super) fn log(&mut self, entry: JournalEntry) {
        self.entries.push(entry, usize::MAX);
    }

    // Whether the store follows the records each update stamps.
    #[inline]
    pub(super) fn follows_stamps(&self) -> bool {
        self.followed.0 & STAMPED != 0
    }

    // Lists `record` among those the update in progress stamped.
    pub(super) fn stamp(&mut self, record: RecordRef) {
        self.stamps.push(record, MOST_STAMPS);
    }

    // Forgets every change: a reader of an earlier version finds its
    // changes lost. `update` is the store's update from now on.
    pub(super) fn clear(&mut self, update: SceneUpdate) {
        self.entries.clear();
        self.stamps.clear();
        self.cleared = Some(update);
        self.changed = update;
    }

    // Gives back at rest the changes and the stamps it keeps: every reader
    // that draws read the latest update before the rest ended, and one
    // further behind works from the store as it is.
    pub(super) fn trim(&mut self) {
        self.entries.trim();
        self.stamps.trim();
    }

    pub(super) fn heap_bytes(&self) -> usize {
        self.entries.heap_bytes() + self.stamps.heap_bytes()
    }

    // The changes of the updates after `read`, the update a reader read
    // last, when the store's update is `now`.
    pub(super) fn since(&self, read: SceneUpdate, now: SceneUpdate) -> JournalRead<'_> {
        self.entries
            .since(read, now)
            .map_or(JournalRead::Lost, JournalRead::Changes)
    }

    // The records the updates after `read` stamped, when the store's update
    // is `now`; `None` when the journal lost some of them.
    pub(super) fn stamps_since(&self, read: SceneUpdate, now: SceneUpdate) -> Option<&[RecordRef]> {
        self.stamps.since(read, now)
    }
}
