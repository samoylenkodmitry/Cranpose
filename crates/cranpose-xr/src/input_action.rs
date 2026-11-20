//! What a ray does at a volume's mesh.

/// What a pointer did, as Jetpack XR's `InputEvent.Action` tells it: a sequence of events on the
/// same target is `HoverEnter`, `HoverMove`, `HoverExit` while no press is on, and `Down`, `Move`,
/// `Up` for a press. Jetpack XR's `CANCEL` has no counterpart: a controller that is lost during a
/// press ends it with an `Up`, as it does for panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InputAction {
    /// The ray first meets the volume's meshes.
    HoverEnter,
    /// The ray moves over the volume's meshes with no press on.
    HoverMove,
    /// The ray leaves the volume's meshes with no press on.
    HoverExit,
    /// The trigger goes down, or the pinch closes, on a mesh.
    Down,
    /// The ray moves while a press is on, also off the mesh that the press began on.
    Move,
    /// The trigger goes up, or the pinch opens.
    Up,
}
