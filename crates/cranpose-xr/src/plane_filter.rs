//! Which planes a movable composable snaps to.

#[cfg(any(test, headset))]
use crate::{PlaneLabel, PlaneType};
use crate::{PlaneOrientation, PlaneSemantic};

// The sets of orientations and semantics of `MovePolicy::anchor`, as bits of the variants' order.
// An empty set admits every plane, as in Jetpack Compose XR.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PlaneFilter {
    orientations: u8,
    semantics: u8,
}

impl PlaneFilter {
    pub(crate) fn new(orientations: &[PlaneOrientation], semantics: &[PlaneSemantic]) -> Self {
        Self {
            orientations: orientations
                .iter()
                .fold(0, |bits, orientation| bits | 1 << *orientation as u8),
            semantics: semantics
                .iter()
                .fold(0, |bits, semantic| bits | 1 << *semantic as u8),
        }
    }

    // Whether a plane of `plane_type` with `label` passes both sets.
    #[cfg(any(test, headset))]
    pub(crate) fn admits(self, plane_type: PlaneType, label: PlaneLabel) -> bool {
        let orientation = [
            PlaneOrientation::Any,
            PlaneOrientation::Horizontal,
            PlaneOrientation::Vertical,
        ]
        .into_iter()
        .any(|orientation| {
            self.orientations & (1 << orientation as u8) != 0 && orientation.admits(plane_type)
        });
        let semantic = [
            PlaneSemantic::Any,
            PlaneSemantic::Ceiling,
            PlaneSemantic::Floor,
            PlaneSemantic::Table,
            PlaneSemantic::Wall,
        ]
        .into_iter()
        .any(|semantic| self.semantics & (1 << semantic as u8) != 0 && semantic.admits(label));
        (self.orientations == 0 || orientation) && (self.semantics == 0 || semantic)
    }
}
