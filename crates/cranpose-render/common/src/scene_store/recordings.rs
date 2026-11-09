// The recordings of the draw commands that record again, kept so that a
// command's next recording reuses the storage of one no reader holds. A
// command enters when the store lets go of a run whose recording no other
// reader holds: one that records once and stays drawn takes no entry.

use std::rc::Rc;

use cranpose_core::NodeId;
use cranpose_ui_graphics::{CommandRecording, FxHashMap};

use crate::graph::DrawCommandId;

#[derive(Clone, Default)]
pub(crate) struct Recordings {
    slots: FxHashMap<RecorderKey, RecorderSlot>,
    update: u64,
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
        self.update = self.update.wrapping_add(1);
        if self.update.is_multiple_of(512) {
            let update = self.update;
            self.slots
                .retain(|_, slot| update.wrapping_sub(slot.update) <= 64);
        }
    }

    // Storage for the next recording of `id`: a kept recording no reader
    // holds, one that shares no shapes first, or a new one.
    pub(crate) fn acquire(&mut self, id: DrawCommandId) -> Rc<CommandRecording> {
        let Some(slot) = self.slots.get_mut(&RecorderKey::from(id)) else {
            return Rc::default();
        };
        let free = |handle: &Option<Rc<CommandRecording>>| {
            handle.as_ref().is_some_and(|recording| {
                Rc::strong_count(recording) == 1 && Rc::weak_count(recording) == 0
            })
        };
        let unshared = slot.handles.iter().position(|handle| {
            free(handle)
                && handle
                    .as_ref()
                    .is_some_and(|recording| !recording.shares_shapes())
        });
        let at = unshared.or_else(|| {
            (slot.handles.iter().all(Option::is_some) && free(&slot.handles[2])).then_some(2)
        });
        at.and_then(|at| slot.handles[at].take())
            .unwrap_or_default()
    }

    // Notes `recording`, which `id` just recorded, beside the two before it
    // when the command has an entry.
    pub(crate) fn publish(&mut self, id: DrawCommandId, recording: &Rc<CommandRecording>) {
        if let Some(slot) = self.slots.get_mut(&RecorderKey::from(id)) {
            slot.hold(self.update, recording);
        }
    }

    // Keeps `recording`, which `id` recorded and which no reader holds once
    // the store lets go of its run, for the command's next recording.
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
