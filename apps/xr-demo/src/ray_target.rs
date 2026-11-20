// What the rays meet in the demo's volumes, as the panel shows it: the torus, the crate and the
// glTF node of the crate that a ray hits, or nothing. Each volume reports its ray's input events
// here. The pointer emits the exit from one volume before the enter into the next, so the last
// report is the volume that the ray is over.

use cranpose::prelude::*;
use cranpose_xr::{HitInfo, InputAction, InputEvent};

const NOTHING: &str = "nothing";

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct RayTarget(MutableState<String>);

#[composable]
pub(crate) fn rememberRayTarget() -> RayTarget {
    RayTarget(rememberMutableStateOf(|| NOTHING.to_owned()))
}

impl RayTarget {
    // The name of what the ray meets.
    pub(crate) fn text(&self) -> String {
        self.0.get()
    }

    // Takes an input event of a volume: a hit names `mesh` of the volume, and a ray that leaves
    // the volume meets nothing. The text changes only with the name, so a ray that moves over a
    // mesh redraws nothing.
    pub(crate) fn report(&self, event: &InputEvent, mesh: impl FnOnce(&HitInfo) -> String) {
        let name = match (event.action, event.hit_info) {
            (InputAction::HoverExit, _) => NOTHING.to_owned(),
            (_, Some(hit)) => mesh(&hit),
            (_, None) => return,
        };
        if self.0.get() != name {
            self.0.set(name);
        }
    }
}
