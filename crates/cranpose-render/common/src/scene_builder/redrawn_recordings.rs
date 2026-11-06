use std::rc::Rc;

use cranpose_core::collections::map::HashMap as NodeMap;
use cranpose_ui_graphics::{CommandRecording, RecordingRoom};

use super::RecorderKey;
use crate::graph::DrawCommandId;

// The most heap a recording's tables may hold for its command to keep it to
// the next update with little memory left: a gauge's or a switch's few
// shapes, whose buffers cost more to allocate each update than they hold,
// not a chart's or a canvas's many.
const TIGHT_KEPT_BYTES: usize = 1024;

// What the commands of `RecordingStore::Redrawn` keep to record into: the
// recordings of those that drew in two updates in a row, the room of the
// others', and the update each command drew in last; the update that draws
// now, and whether the memory left was little when the last update ended.
#[derive(Default)]
pub(crate) struct RedrawnRecordings {
    kept: NodeMap<RecorderKey, Kept>,
    update: u64,
    // How many commands kept their buffers in the update that draws now.
    kept_now: usize,
    tight: bool,
}

struct Kept {
    buffers: Option<RedrawnBuffers>,
    drawn: u64,
}

// When the command of a recording that a lowered node gives back draws
// again.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Redraw {
    // In this update: the node of a kept head lowers again.
    Now,
    // When the updates before say: a command drawn in two updates in a row
    // draws in the next.
    AsBefore,
}

// What a command keeps to the next update to record into.
enum RedrawnBuffers {
    // Its recording, to record into again.
    Recording(Rc<CommandRecording>),
    // The room its recording had, for a new one to start with: boxed, so
    // that a command's entry moves as two words.
    Room(Box<RecordingRoom>),
}

impl RedrawnRecordings {
    // The recording of the command `id` that the update before kept, for
    // the command to record into again, or a new one with the room the kept
    // one had; a new one when there is none. A command takes no buffers
    // another command filled: a node that keeps its recording past the
    // update, as a moved subtree does, would hold their room. A kept room
    // stays in its box, which the command's next room takes.
    pub(crate) fn acquire(&mut self, id: DrawCommandId) -> Rc<CommandRecording> {
        let Some(kept) = self.kept.get_mut(&RecorderKey::from(id)) else {
            return Rc::default();
        };
        match &mut kept.buffers {
            Some(RedrawnBuffers::Room(room)) => Rc::new(room.empty_recording()),
            buffers => match buffers.take() {
                Some(RedrawnBuffers::Recording(recording)) => recording,
                _ => Rc::default(),
            },
        }
    }

    // Keeps `recording`, the command `id` drew in this update, when no
    // reader holds it any more: until the next update, when the command
    // draws again now, as the node of a kept head that lowers again does, or
    // drew in the update before as well, so that a command drawn each update
    // records into the buffers it filled; else only its room. With little
    // memory left only a small recording stays to the next update: a canvas
    // drawn each frame then holds what one drawn once does.
    pub(crate) fn keep(
        &mut self,
        id: DrawCommandId,
        recording: Rc<CommandRecording>,
        redraw: Redraw,
    ) {
        if Rc::strong_count(&recording) != 1 || Rc::weak_count(&recording) != 0 {
            return;
        }
        let update = self.update;
        let kept = self.kept.entry(RecorderKey::from(id)).or_insert(Kept {
            buffers: None,
            drawn: u64::MAX,
        });
        if kept.drawn != update {
            self.kept_now += 1;
        }
        let again =
            redraw == Redraw::Now || kept.drawn == update.wrapping_sub(1) || kept.drawn == update;
        let keeps = again && (!self.tight || recording.pod_heap_bytes() <= TIGHT_KEPT_BYTES);
        match &mut kept.buffers {
            buffers if keeps => *buffers = Some(RedrawnBuffers::Recording(recording)),
            Some(RedrawnBuffers::Room(room)) => **room = RecordingRoom::of(&recording),
            buffers => {
                *buffers = Some(RedrawnBuffers::Room(Box::new(RecordingRoom::of(
                    &recording,
                ))));
            }
        }
        kept.drawn = update;
    }

    // Lets go the recordings of the commands this update did not draw as it
    // ends; `tight` is whether the memory left is little, which the next
    // update keeps recordings by. An update that drew every command kept
    // has none to let go.
    pub(crate) fn end_update(&mut self, tight: bool) {
        self.tight = tight;
        let update = self.update;
        if self.kept_now < self.kept.len() {
            self.kept.retain(|_, kept| kept.drawn == update);
        }
        self.kept_now = 0;
        self.update = update.wrapping_add(1);
    }

    pub(crate) fn clear(&mut self) {
        self.kept.clear();
        self.kept_now = 0;
    }
}
