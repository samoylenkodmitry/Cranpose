//! The point sources of sound in a space.

use super::{Origin, Space};
use crate::{PointSource, Pose, sound_sources::SoundPlace};

#[expect(clippy::multiple_inherent_impl, reason = "one file per concern")]
impl Space {
    // A new id for a point source of sound.
    pub(crate) fn new_sound_id(&self) -> u64 {
        let id = self.inner.next_id.get();
        self.inner.next_id.set(id + 1);
        id
    }

    // Registers the source `id` at `pose` of the subspace with `origin`, or moves it there.
    pub(crate) fn place_point_source(
        &self,
        id: u64,
        source: PointSource,
        origin: Origin,
        pose: Pose,
    ) {
        let sounds = &mut *self.inner.sounds.borrow_mut();
        sounds.place(id, source, origin, pose);
    }

    // Forgets the source `id`.
    pub(crate) fn remove_point_source(&self, id: u64) {
        self.inner.sounds.borrow_mut().remove(id);
    }

    // Makes the panel `id` a source of `source`, or stops it from being one. The source stands at
    // the panel's pose, wherever a carry or a following subspace puts it.
    pub(crate) fn set_point_source(&self, id: u64, source: Option<PointSource>) {
        self.inner.sounds.borrow_mut().attach(id, source);
    }

    #[cfg_attr(
        not(any(test, headset)),
        expect(dead_code, reason = "the headset backend calls it")
    )]
    // Puts the sources and where they stand now into `places`. See `SoundSources::places_into`.
    pub(crate) fn sounds_into(&self, places: &mut Vec<SoundPlace>) {
        let entries = self.inner.entries.borrow();
        let panel_pose = |panel| {
            let entry = entries.iter().find(|entry| entry.slot.id == panel);
            entry.map(|entry| entry.slot.pose)
        };
        let follow = self.inner.follow_origins.borrow();
        self.inner
            .sounds
            .borrow()
            .places_into(places, panel_pose, &follow);
    }
}
