//! The pointer input of a surface: where a controller's ray meets it, as Compose's pointer events.

use std::{rc::Rc, time::Duration};

use cranpose::{Point, PointerButton, PointerButtons, PointerEvent, PointerEventKind};

use crate::{
    Hand, HitInfo, InputAction, InputEvent, InputSource, Pose, Ray, content_ray::ContentRay,
    mesh_ref::MeshRef,
};

// What the pointer does at one target, in the target's own dp from its top left corner. The
// target is the app's composition, which reads the atlas's dp, or one surface.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(
    not(any(test, headset)),
    expect(dead_code, reason = "the pointer of the headset backend makes them")
)]
pub(crate) enum Gesture {
    Move { x: f32, y: f32 },
    Press,
    Release,
    // The pointer left the target.
    Leave,
}

// What the pointer did on a surface. A gesture on a volume carries the ray behind it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SurfaceGesture {
    pub(crate) surface: u64,
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

// What a surface knows of its pointer, to tell Compose's events from the pointer's gestures.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SurfaceInput {
    // Where the pointer is on the surface, or `None` while it is away
    at: Option<(f32, f32)>,
    pressed: bool,
}

impl SurfaceInput {
    // The event of Compose that `gesture` makes, with the position in the surface's dp, and the
    // action that it is for a volume's input event. The first move over the surface enters it, and
    // a release with no press makes none.
    pub(crate) fn event(&mut self, gesture: Gesture) -> Option<(PointerEvent, InputAction)> {
        let (kind, action, (x, y)) = match gesture {
            Gesture::Move { x, y } => {
                let (kind, action) = match (self.at.is_some(), self.pressed) {
                    (_, true) => (PointerEventKind::Move, InputAction::Move),
                    (true, false) => (PointerEventKind::Move, InputAction::HoverMove),
                    (false, false) => (PointerEventKind::Enter, InputAction::HoverEnter),
                };
                self.at = Some((x, y));
                (kind, action, (x, y))
            }
            Gesture::Press => {
                self.pressed = true;
                (PointerEventKind::Down, InputAction::Down, self.at?)
            }
            Gesture::Release => {
                let released = std::mem::take(&mut self.pressed);
                (
                    PointerEventKind::Up,
                    InputAction::Up,
                    self.at.filter(|_| released)?,
                )
            }
            Gesture::Leave => (
                PointerEventKind::Exit,
                InputAction::HoverExit,
                self.at.take()?,
            ),
        };
        let position = Point::new(x, y);
        let buttons = if self.pressed {
            PointerButtons::new().with(PointerButton::Primary)
        } else {
            PointerButtons::NONE
        };
        Some((
            PointerEvent::new(kind, position, position).with_buttons(buttons),
            action,
        ))
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
