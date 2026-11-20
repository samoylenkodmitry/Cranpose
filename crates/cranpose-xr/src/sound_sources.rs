//! The point sources of sound that the app's composables put in a space.

use crate::{PointSource, Pose, follow_origin::FollowOrigins, space::Origin};

// Where a source stands.
#[derive(Clone, Copy)]
pub(crate) enum SoundAnchor {
    // At the pose of the panel with this id, which the user may carry
    Panel(u64),
    // At a pose in a subspace. `pose` is relative to the room, or relative to the following
    // subspace's live origin when `follower` is set.
    Placed { pose: Pose, follower: Option<u64> },
}

// A source and where it stands.
#[derive(Clone, Copy)]
pub(crate) struct SoundEntry {
    pub(crate) id: u64,
    pub(crate) source: PointSource,
    pub(crate) anchor: SoundAnchor,
}

// A point source of sound and where it stands in the room.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SoundPlace {
    pub(crate) id: u64,
    pub(crate) source: PointSource,
    // The place in the room, or `None` while the source's following subspace has none
    pub(crate) pose: Option<Pose>,
}

// The sources of a space, in the order they registered.
#[derive(Default)]
pub(crate) struct SoundSources {
    entries: Vec<SoundEntry>,
}

impl SoundSources {
    // Registers the source `id`, or replaces what it holds.
    pub(crate) fn set(&mut self, entry: SoundEntry) {
        match self.entries.iter_mut().find(|known| known.id == entry.id) {
            Some(known) => *known = entry,
            None => self.entries.push(entry),
        }
    }

    // Registers a source at `pose` of the subspace with `origin`.
    pub(crate) fn place(&mut self, id: u64, source: PointSource, origin: Origin, pose: Pose) {
        self.set(SoundEntry {
            id,
            source,
            anchor: SoundAnchor::Placed {
                pose,
                follower: origin.follower(),
            },
        });
    }

    // Makes the panel `panel` a source of `source`, or stops it from being one.
    pub(crate) fn attach(&mut self, panel: u64, source: Option<PointSource>) {
        match source {
            Some(source) => self.set(SoundEntry {
                id: panel,
                source,
                anchor: SoundAnchor::Panel(panel),
            }),
            None => self.remove(panel),
        }
    }

    pub(crate) fn remove(&mut self, id: u64) {
        self.entries.retain(|entry| entry.id != id);
    }

    // Puts the sources and where they stand now into `places` in place of what it held. A frame
    // loop passes the same list at each frame, so the list keeps its storage. `panel_pose` gives
    // the live pose of a panel, which a carry or a following subspace moves. A source's pose is
    // `None` while its panel is gone or its following subspace has no place.
    pub(crate) fn places_into(
        &self,
        places: &mut Vec<SoundPlace>,
        panel_pose: impl Fn(u64) -> Option<Pose>,
        follow: &FollowOrigins,
    ) {
        places.clear();
        places.extend(self.entries.iter().map(|sound| {
            SoundPlace {
                id: sound.id,
                source: sound.source,
                pose: match sound.anchor {
                    SoundAnchor::Panel(panel) => panel_pose(panel),
                    SoundAnchor::Placed {
                        pose,
                        follower: None,
                    } => Some(pose),
                    SoundAnchor::Placed {
                        pose,
                        follower: Some(id),
                    } => follow
                        .standing(id)
                        .and_then(|standing| standing.at)
                        .map(|at| at.then(&pose)),
                },
            }
        }));
    }
}
