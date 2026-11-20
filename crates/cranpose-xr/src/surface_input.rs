//! The pointer input of a surface: where a controller's ray meets it, as Compose's pointer events.

use std::{rc::Rc, time::Duration};

use cranpose::{Point, PointerButton, PointerButtons, PointerEvent, PointerEventKind};

use crate::{
    Hand, HitInfo, InputAction, InputEvent, InputSource, Pose, Ray, content_ray::ContentRay,
    mesh_ref::MeshRef,
};

// What a hand's pointer does at one target, in the target's own dp from its top left corner. The
// target is the app's composition, which reads the atlas's dp, or one surface.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the pointer of the headset backend makes them")
)]
pub(crate) enum Gesture {
    Move { x: f32, y: f32 },
    // A button of the pointer goes down or up. The primary button is a press, as the trigger
    // makes it.
    Press(PointerButton),
    Release(PointerButton),
    // The thumbstick scrolls by `x` and `y` dp, in the direction that a wheel gives.
    Scroll { x: f32, y: f32 },
    // The pointer left the target.
    Leave,
}

// What a hand's pointer did on the app's composition.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the pointer of the headset backend makes them")
)]
pub(crate) struct HandGesture {
    pub(crate) hand: Hand,
    pub(crate) gesture: Gesture,
}

// What a hand's pointer did on a surface. A gesture on a volume carries the ray behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceGesture {
    pub(crate) surface: u64,
    pub(crate) hand: Hand,
    pub(crate) gesture: Gesture,
    pub(crate) ray: Option<VolumeRay>,
}

// The ray behind a gesture on a volume, in the room, and the mesh that it meets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct VolumeRay {
    pub(crate) source: InputSource,
    pub(crate) hand: Hand,
    pub(crate) time: Duration,
    pub(crate) ray: Ray,
    pub(crate) hit: Option<RayHit>,
}

// The mesh that a ray meets, or that a press began on, and where the ray meets it: the triangle and
// the place in the volume's content space. The place is `None` while a press that began on the mesh
// goes on with the ray off it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RayHit {
    pub(crate) mesh: MeshRef,
    pub(crate) place: Option<(u32, [f32; 3])>,
}

// The callback that the app gave a surface for its pointer events.
pub(crate) type PointerSink = Rc<dyn Fn(PointerEvent)>;

// The callback that the app gave a volume for its input events.
pub(crate) type InputSink = Rc<dyn Fn(InputEvent)>;

// What a surface knows of one hand's pointer, to tell Compose's events from the pointer's gestures.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SurfaceInput {
    // Where the pointer is on the surface, or `None` while it is away
    at: Option<(f32, f32)>,
    buttons: PointerButtons,
}

impl SurfaceInput {
    // The event of Compose that `gesture` of `hand` makes, with the position in the surface's dp,
    // and the action that it is for a volume's input event. The first move over the surface enters
    // it, and a release with no press makes none. A volume's input event has no buttons and no
    // scroll, so only the primary button and the moves make an action.
    pub(crate) fn event(
        &mut self,
        hand: Hand,
        gesture: Gesture,
    ) -> Option<(PointerEvent, Option<InputAction>)> {
        let pressed = self.buttons != PointerButtons::NONE;
        let primary = self.buttons.contains(PointerButton::Primary);
        let mut scroll = None;
        let (kind, action, (x, y)) = match gesture {
            Gesture::Move { x, y } => {
                let (kind, action) = match (self.at.is_some(), pressed) {
                    (_, true) => (PointerEventKind::Move, primary.then_some(InputAction::Move)),
                    (true, false) => (PointerEventKind::Move, Some(InputAction::HoverMove)),
                    (false, false) => (PointerEventKind::Enter, Some(InputAction::HoverEnter)),
                };
                self.at = Some((x, y));
                (kind, action, (x, y))
            }
            Gesture::Press(button) => {
                self.buttons.insert(button);
                let (kind, action) = match button {
                    PointerButton::Primary => (PointerEventKind::Down, Some(InputAction::Down)),
                    other => (PointerEventKind::ButtonDown(other), None),
                };
                (kind, action, self.at?)
            }
            Gesture::Release(button) => {
                let released = self.buttons.contains(button);
                self.buttons.remove(button);
                let (kind, action) = match button {
                    PointerButton::Primary => (PointerEventKind::Up, Some(InputAction::Up)),
                    other => (PointerEventKind::ButtonUp(other), None),
                };
                (kind, action, self.at.filter(|_| released)?)
            }
            Gesture::Scroll { x, y } => {
                scroll = Some(Point::new(x, y));
                (PointerEventKind::Scroll, None, self.at?)
            }
            Gesture::Leave => {
                self.buttons = PointerButtons::NONE;
                (
                    PointerEventKind::Exit,
                    Some(InputAction::HoverExit),
                    self.at.take()?,
                )
            }
        };
        let position = Point::new(x, y);
        let mut event = PointerEvent::new(kind, position, position)
            .with_buttons(self.buttons)
            .with_id(hand.pointer_id());
        if let Some(delta) = scroll {
            event = event.with_scroll_delta(delta);
        }
        Some((event, action))
    }
}

// The input event of a volume at `pose` that `ray` makes as `action`, with the ray and the hit in
// the volume's content space.
pub(crate) fn input_event(ray: &VolumeRay, action: InputAction, pose: &Pose) -> InputEvent {
    let content = ContentRay::of(&ray.ray, pose);
    InputEvent {
        source: ray.source,
        pointer_type: ray.hand,
        timestamp: ray.time,
        origin: content.origin,
        direction: content.unit_direction(),
        action,
        hit_info: ray.hit.map(|hit| HitInfo {
            input_entity: hit.mesh.node,
            mesh_index: hit.mesh.index,
            hit_position: hit.place.map(|(_, position)| position),
            triangle: hit.place.map(|(triangle, _)| triangle),
        }),
    }
}
