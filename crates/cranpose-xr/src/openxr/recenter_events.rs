//! The runtime's recenter events, held until the next frame.

use openxr as xr;

use super::{clock, session::pose_of};
use crate::Recenter;

// The change of the app's room that the runtime announced since the last frame.
#[derive(Default)]
pub(crate) struct RecenterEvents {
    pending: Option<Recenter>,
}

impl RecenterEvents {
    // Notes `event`. Only a change of the LOCAL space counts, because the app's room is that
    // space. Two changes before the next frame become one.
    pub(crate) fn record(
        &mut self,
        instance: &xr::Instance,
        event: xr::ReferenceSpaceChangePending<'_>,
    ) {
        if event.reference_space_type() != xr::ReferenceSpaceType::LOCAL {
            return;
        }
        let change = Recenter {
            change_time: clock::monotonic(instance, event.change_time()),
            pose_in_previous_space: event
                .pose_valid()
                .then(|| pose_of(&event.pose_in_previous_space())),
        };
        self.pending = Some(match self.pending {
            Some(first) => first.then(change),
            None => change,
        });
    }

    // The change since the last call, if there is one.
    pub(crate) fn take(&mut self) -> Option<Recenter> {
        self.pending.take()
    }
}
