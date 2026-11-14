// The recordings of the draw commands that record again, kept so that a
// command's next recording reuses the storage of one no reader holds. A
// command enters when the store lets go of a run's recording, which a reader
// may still hold: one that records once and stays drawn takes no entry, and
// a recording a run holds stays the run's alone, so the run can record into
// it again in place.

use std::rc::Rc;

use cranpose_core::{NodeId, collections::growth_slack::GrowthSlack};
use cranpose_ui_graphics::{CommandRecording, FxHashMap};

use crate::graph::DrawCommandId;

#[derive(Default)]
pub(crate) struct Recordings {
    slots: FxHashMap<RecorderKey, RecorderSlot>,
    update: u64,
}

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

    // Keeps `recording`, once: the runs of a command let go of one recording
    // in any order, and a recording held twice reads as held by a reader for
    // as long as the slot keeps it.
    fn hold(&mut self, update: u64, recording: &Rc<CommandRecording>) {
        self.update = update;
        if (self.handles.iter().flatten()).any(|held| Rc::ptr_eq(held, recording)) {
            return;
        }
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
            self.sweep();
        }
    }

    #[cold]
    #[inline(never)]
    fn sweep(&mut self) {
        let update = self.update;
        self.slots
            .retain(|_, slot| update.wrapping_sub(slot.update) <= 64);
    }

    // Gives back at rest the slots of the commands of nodes the store no
    // longer draws, which a store at rest never sweeps, and the room past
    // the slots it keeps.
    pub(super) fn trim(&mut self, draws: impl Fn(NodeId) -> bool) {
        self.slots.retain(|key, _| draws(key.node_id));
        self.slots.trim_growth_slack();
    }

    // The slot table and the recordings only its slots hold.
    pub(super) fn heap_bytes(&self) -> usize {
        // A table keeps one control byte beside each slot.
        let table = self.slots.capacity() * (size_of::<(RecorderKey, RecorderSlot)>() + 1);
        let kept = self
            .slots
            .values()
            .flat_map(|slot| slot.handles.iter().flatten())
            .filter(|recording| Rc::strong_count(recording) == 1)
            .map(|recording| recording.pod_heap_bytes() + size_of_val(recording.others()))
            .sum::<usize>();
        table + kept
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
