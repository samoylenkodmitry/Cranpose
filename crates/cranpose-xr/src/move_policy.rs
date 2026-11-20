//! Who applies the pose of a move, and who hears of it.

use std::{fmt, rc::Rc};

use crate::{PlaneOrientation, PlaneSemantic, SpatialMoveEvent, plane_filter::PlaneFilter};

type OnMove = Rc<dyn Fn(SpatialMoveEvent)>;

/// What a movable composable does when the user carries it, as Jetpack XR's `MovePolicy`.
/// `SubspaceModifier::movable_with` takes it.
///
/// The callbacks run on the thread of the composition, once for each move event, in the frame in
/// which the controller moved. They may set state.
#[derive(Clone, Default)]
pub struct MovePolicy {
    on_move: Option<OnMove>,
    app_moves: bool,
    snap: Option<PlaneFilter>,
}

impl MovePolicy {
    /// The headset moves the content, and `on_move` hears of each step. Jetpack XR's
    /// `MovePolicy.system`. Without a callback it is [`MovePolicy::default`].
    pub fn system(on_move: impl Fn(SpatialMoveEvent) + 'static) -> Self {
        Self {
            on_move: Some(Rc::new(on_move)),
            app_moves: false,
            snap: None,
        }
    }

    /// The headset moves the content as [`system`](Self::system) does, and the content snaps to a
    /// plane of the room when the user lets it go near one that the two lists admit. A plane
    /// is admitted when its direction is in `orientations` and its label is in `semantics`. An
    /// empty list admits every plane, as in Jetpack Compose XR, whose `MovePolicy.anchor` this
    /// follows. Jetpack's SceneCore `AnchorPlacement.createForPlanes` anchors nothing for an empty
    /// set. `on_move` hears of each step, including the pose that the snap gave.
    ///
    /// The content comes to rest on a plane at most 25 centimeters from the place where the user
    /// holds it, if that place is over the plane or within 30 centimeters of its edge. As in Jetpack
    /// XR, its +Z axis then points out of the plane, so a panel lies flat along the plane. A panel's
    /// center is on the plane, and a volume's back face is. A wall holds the content upright. A
    /// floor, a table or a ceiling holds it with its top edge pointing away from where it faced, so a
    /// panel that faced the user reads from the user's side. The snap shows while the user carries:
    /// the content follows the controller again when the user moves it beyond the distance.
    ///
    /// The app needs `Use::plane_tracking()` in its build and the user's permission. Without
    /// planes the content moves freely. Jetpack Compose XR also reparents the content to the plane,
    /// which this policy does not. An app that keeps the content in place after a restart makes an
    /// [`Anchor`](crate::Anchor) at the pose of the
    /// [`SpatialMoveEventType::End`](crate::SpatialMoveEventType::End) event.
    pub fn anchor(
        orientations: &[PlaneOrientation],
        semantics: &[PlaneSemantic],
        on_move: impl Fn(SpatialMoveEvent) + 'static,
    ) -> Self {
        Self {
            on_move: Some(Rc::new(on_move)),
            app_moves: false,
            snap: Some(PlaneFilter::new(orientations, semantics)),
        }
    }

    /// The headset only reports the steps. The app moves the content: a state that it sets from
    /// `on_move` feeds the content's `SubspaceModifier`, such as through `with_pose`. Jetpack XR's
    /// `MovePolicy.custom`.
    pub fn custom(on_move: impl Fn(SpatialMoveEvent) + 'static) -> Self {
        Self {
            on_move: Some(Rc::new(on_move)),
            app_moves: true,
            snap: None,
        }
    }

    // Whether the headset gives the content the pose that the user carries it to.
    pub(crate) fn moves_content(&self) -> bool {
        !self.app_moves
    }

    // The planes that the content snaps to, or `None` for content that moves freely.
    #[cfg(any(test, headset))]
    pub(crate) fn snap_filter(&self) -> Option<PlaneFilter> {
        self.snap
    }

    // The callback that hears of the steps, held apart from any borrow so that it may call back.
    pub(crate) fn listener(&self) -> Option<OnMove> {
        self.on_move.clone()
    }
}

impl PartialEq for MovePolicy {
    fn eq(&self, other: &Self) -> bool {
        self.app_moves == other.app_moves
            && self.snap == other.snap
            && match (&self.on_move, &other.on_move) {
                (Some(a), Some(b)) => Rc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            }
    }
}

impl fmt::Debug for MovePolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MovePolicy")
            .field("app_moves", &self.app_moves)
            .field("snaps", &self.snap.is_some())
            .field("listens", &self.on_move.is_some())
            .finish()
    }
}

#[cfg(test)]
#[path = "tests/move_policy_tests.rs"]
mod tests;
