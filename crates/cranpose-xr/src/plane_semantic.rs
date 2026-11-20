//! Which planes a movable panel may snap to by what they are.

#[cfg(any(test, headset))]
use crate::PlaneLabel;

/// What the planes are that a movable panel may anchor to. Jetpack Compose XR's `PlaneSemantic`,
/// which `MovePolicy.anchor` takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaneSemantic {
    /// Every plane, whatever its label, including a plane with no label. Jetpack XR's
    /// `PlaneSemantic.Any`.
    Any,
    /// Ceilings. Jetpack XR's `PlaneSemantic.Ceiling`.
    Ceiling,
    /// Floors. Jetpack XR's `PlaneSemantic.Floor`.
    Floor,
    /// Tables. Jetpack XR's `PlaneSemantic.Table`.
    Table,
    /// Walls. Jetpack XR's `PlaneSemantic.Wall`.
    Wall,
}

impl PlaneSemantic {
    // Whether a plane with `label` is of this kind.
    #[cfg(any(test, headset))]
    pub(crate) const fn admits(self, label: PlaneLabel) -> bool {
        matches!(
            (self, label),
            (Self::Any, _)
                | (Self::Ceiling, PlaneLabel::Ceiling)
                | (Self::Floor, PlaneLabel::Floor)
                | (Self::Table, PlaneLabel::Table)
                | (Self::Wall, PlaneLabel::Wall)
        )
    }
}
