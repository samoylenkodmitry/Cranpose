// The recordings of the draw commands that record again, kept so that a
// command's next recording reuses the storage of one no reader holds. A
// command enters when the store lets go of a run's recording, which a reader
// may still hold: one that records once and stays drawn takes no entry, and
// a recording a run holds stays the run's alone, so the run can record into
// it again in place.

use std::{ops::Range, rc::Rc};

use cranpose_core::NodeId;
use cranpose_ui_graphics::{CommandRecording, FxHashMap};

use crate::graph::DrawCommandId;

// A recording a command made, with the segments of it that are the
// command's.
pub(crate) type Recorded = (Rc<CommandRecording>, Range<u32>);

#[derive(Clone, Default)]
pub(crate) struct Recordings {
    slots: FxHashMap<RecorderKey, RecorderSlot>,
    update: u64,
    // The recordings a redraw in place made before it gave up, with their
    // segments, for the fallback that records the node's draws anew in the
    // same update.
    parked: Vec<(DrawCommandId, Recorded)>,
}

#[derive(Clone)]
struct RecorderSlot {
    // The update that last recorded the command or let go of its run.
    update: u64,
    handles: [Option<Rc<CommandRecording>>; 3],
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RecorderKey {
    node_id: NodeId,
    command_index: u32,
}

impl From<DrawCommandId> for RecorderKey {
    fn from(id: DrawCommandId) -> Self {
        Self {
            node_id: id.node_id,
            command_index: id.command_index,
        }
    }
}

impl RecorderSlot {
    // Storage for the next recording: a kept recording no reader holds,
    // one that shares no shapes first, or a new one.
    fn lend(&mut self) -> Rc<CommandRecording> {
        let free = |handle: &Option<Rc<CommandRecording>>| {
            handle.as_ref().is_some_and(|recording| {
                Rc::strong_count(recording) == 1 && Rc::weak_count(recording) == 0
            })
        };
        let unshared = self.handles.iter().position(|handle| {
            free(handle)
                && handle
                    .as_ref()
                    .is_some_and(|recording| !recording.shares_shapes())
        });
        let at = unshared.or_else(|| {
            (self.handles.iter().all(Option::is_some) && free(&self.handles[2])).then_some(2)
        });
        at.and_then(|at| self.handles[at].take())
            .unwrap_or_default()
    }

    fn hold(&mut self, update: u64, recording: &Rc<CommandRecording>) {
        self.update = update;
        self.handles[2] = self.handles[1].take();
        self.handles[1] = self.handles[0].take();
        self.handles[0] = Some(Rc::clone(recording));
    }
}

impl Recordings {
    // Starts an update. Every 512 updates the commands that neither recorded
    // nor let go of a run for 64 leave.
    pub(super) fn begin_update(&mut self) {
        if !self.parked.is_empty() {
            self.drop_parked();
        }
        self.update = self.update.wrapping_add(1);
        if self.update.is_multiple_of(512) {
            self.sweep();
        }
    }

    // A fallback takes what a redraw parked in its update; what it left
    // goes.
    #[cold]
    #[inline(never)]
    fn drop_parked(&mut self) {
        self.parked.clear();
    }

    #[cold]
    #[inline(never)]
    fn sweep(&mut self) {
        let update = self.update;
        self.slots
            .retain(|_, slot| update.wrapping_sub(slot.update) <= 64);
    }

    // Records the command `id` anew through `record`, which takes storage
    // from the maker it gets once it records: a kept recording of the
    // command that no reader holds, one that shares no shapes first, or a
    // new one.
    pub(crate) fn record<T>(
        &mut self,
        id: DrawCommandId,
        record: impl FnOnce(
            &mut dyn FnMut() -> Rc<CommandRecording>,
        ) -> Option<(Rc<CommandRecording>, T)>,
    ) -> Option<(Rc<CommandRecording>, T)> {
        let update = self.update;
        let mut slot = self.slots.get_mut(&RecorderKey::from(id));
        let made = record(&mut || {
            slot.as_deref_mut().map_or_else(Rc::default, |slot| {
                slot.update = update;
                slot.lend()
            })
        })?;
        Some(made)
    }

    // Parks `recorded`, which the command `id` recorded in this update, for
    // a fallback that records the command again in it.
    pub(crate) fn park(&mut self, id: DrawCommandId, recorded: Recorded) {
        self.parked.push((id, recorded));
    }

    // The recording the command `id` parked in this update, with its
    // segments.
    #[inline]
    pub(crate) fn take_parked(&mut self, id: DrawCommandId) -> Option<Recorded> {
        if self.parked.is_empty() {
            return None;
        }
        let at = self.parked.iter().position(|(parked, _)| *parked == id)?;
        Some(self.parked.swap_remove(at).1)
    }

    // Keeps `recording`, which `id` recorded and which the store lets go
    // of, for the command's next recording once no reader holds it.
    pub(super) fn keep(&mut self, id: DrawCommandId, recording: &Rc<CommandRecording>) {
        let update = self.update;
        self.slots
            .entry(RecorderKey::from(id))
            .or_insert_with(|| RecorderSlot {
                update,
                handles: [None, None, None],
            })
            .hold(update, recording);
    }
}
