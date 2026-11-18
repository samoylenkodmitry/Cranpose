//! Who applies the pose of a move, and who hears of it.

use std::{fmt, rc::Rc};

use crate::SpatialMoveEvent;

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
}

impl MovePolicy {
    /// The headset moves the content, and `on_move` hears of each step. Jetpack XR's
    /// `MovePolicy.system`. Without a callback it is [`MovePolicy::default`].
    pub fn system(on_move: impl Fn(SpatialMoveEvent) + 'static) -> Self {
        Self {
            on_move: Some(Rc::new(on_move)),
            app_moves: false,
        }
    }

    /// The headset only reports the steps. The app moves the content: a state that it sets from
    /// `on_move` feeds the content's `SubspaceModifier`, such as through `with_pose`. Jetpack XR's
    /// `MovePolicy.custom`.
    pub fn custom(on_move: impl Fn(SpatialMoveEvent) + 'static) -> Self {
        Self {
            on_move: Some(Rc::new(on_move)),
            app_moves: true,
        }
    }

    // Whether the headset gives the content the pose that the user carries it to.
    pub(crate) fn moves_content(&self) -> bool {
        !self.app_moves
    }

    // The callback that hears of the steps, held apart from any borrow so that it may call back.
    pub(crate) fn listener(&self) -> Option<OnMove> {
        self.on_move.clone()
    }
}

impl PartialEq for MovePolicy {
    fn eq(&self, other: &Self) -> bool {
        self.app_moves == other.app_moves
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
            .field("listens", &self.on_move.is_some())
            .finish()
    }
}
