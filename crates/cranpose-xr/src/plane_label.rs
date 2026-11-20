//! What a plane of the room is.

/// A semantic description of a [`Plane`](crate::Plane). Jetpack XR's `PlaneLabel`.
///
/// A runtime that labels more kinds of surfaces than these, as Meta's scene model labels a couch, a
/// bed or a door frame, gives them [`Unknown`](Self::Unknown).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlaneLabel {
    /// The plane is a ceiling. Jetpack XR's `PlaneLabel.CEILING`.
    Ceiling,
    /// The plane is a floor. Jetpack XR's `PlaneLabel.FLOOR`.
    Floor,
    /// The plane is a table. Jetpack XR's `PlaneLabel.TABLE`.
    Table,
    /// The plane is of an unknown kind. Jetpack XR's `PlaneLabel.UNKNOWN`.
    #[default]
    Unknown,
    /// The plane is a wall. Jetpack XR's `PlaneLabel.WALL`.
    Wall,
}
