//! The runtime's events about spatial entities, held until the next frame.

use openxr as xr;

use super::uuid_wire::uuid_of;
use crate::Uuid;

// An event that finishes one of the asynchronous requests of Meta's spatial entities. Each carries
// the id of the request that started it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum SpatialEvent {
    AnchorCreated {
        request: u64,
        result: xr::sys::Result,
        space: xr::sys::Space,
        uuid: Uuid,
    },
    StatusSet {
        request: u64,
        result: xr::sys::Result,
    },
    Saved {
        request: u64,
        result: xr::sys::Result,
        uuid: Uuid,
    },
    Erased {
        request: u64,
        result: xr::sys::Result,
        uuid: Uuid,
    },
    QueryResults {
        request: u64,
    },
    QueryDone {
        request: u64,
        result: xr::sys::Result,
    },
}

// The events that arrived since the last frame. The list keeps its storage from frame to frame.
#[derive(Default)]
pub(crate) struct SpatialEvents {
    pending: Vec<SpatialEvent>,
}

impl SpatialEvents {
    // Notes `event` when it belongs to a spatial entity, and tells whether it did.
    pub(crate) fn record(&mut self, event: &xr::Event<'_>) -> bool {
        let noted = match event {
            xr::Event::SpatialAnchorCreateCompleteFB(done) => SpatialEvent::AnchorCreated {
                request: done.request_id().into_raw(),
                result: done.result(),
                space: done.space(),
                uuid: uuid_of(&done.uuid()),
            },
            xr::Event::SpaceSetStatusCompleteFB(done) => SpatialEvent::StatusSet {
                request: done.request_id().into_raw(),
                result: done.result(),
            },
            xr::Event::SpaceSaveCompleteFB(done) => SpatialEvent::Saved {
                request: done.request_id().into_raw(),
                result: done.result(),
                uuid: uuid_of(&done.uuid()),
            },
            xr::Event::SpaceEraseCompleteFB(done) => SpatialEvent::Erased {
                request: done.request_id().into_raw(),
                result: done.result(),
                uuid: uuid_of(&done.uuid()),
            },
            xr::Event::SpaceQueryResultsAvailableFB(available) => SpatialEvent::QueryResults {
                request: available.request_id().into_raw(),
            },
            xr::Event::SpaceQueryCompleteFB(done) => SpatialEvent::QueryDone {
                request: done.request_id().into_raw(),
                result: done.result(),
            },
            _ => return false,
        };
        self.pending.push(noted);
        true
    }

    // Moves the events into `out`, which keeps its storage from frame to frame.
    pub(crate) fn take_into(&mut self, out: &mut Vec<SpatialEvent>) {
        out.append(&mut self.pending);
    }
}
