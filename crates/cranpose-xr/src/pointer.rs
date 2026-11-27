//! One hand's pointer over the panels: its hover, its press and drag, and the other buttons and
//! the scroll of its controller. A controller or a tracked hand drives it. A tracked hand presses
//! with its ray and pinch, or by the direct touch of its index fingertip. The two hands have a
//! pointer each, so each hovers, presses, captures and drags on its own.

use std::time::Duration;

use cranpose::PointerButton;
use drift::Drift;
use extras::Extras;
use held::Held;

use crate::{
    ControllerInput, Hand as Side, PointerMapping,
    aim::{Aim, Buttons},
    aim_filter::angle_between,
    content_ray::ContentRay,
    direct_touch::{Contact, DirectTouch},
    gestures::Gestures,
    handle::HandleFocus,
    hit::{Hit, Target, ray_hit},
    mesh_ref::MeshRef,
    ray::Beam,
    reach::{CLICK_ANGLE, PressTarget, Pull, Reach, Scene},
    space::Cursor,
    surface_input::{Gesture, HandGesture, RayHit, SurfaceGesture, VolumeRay},
};

mod drift;
mod extras;
mod held;

// A press holds its point while the hand shakes: until the ray turns CLICK_ANGLE degrees from
// where it pointed at the pull in the press's first CLICK_TIME, or DRAG_ANGLE degrees later. Then
// the press drags.
const CLICK_TIME: Duration = Duration::from_millis(350);
const DRAG_ANGLE: f32 = 4.0;
// A trigger that falls LET_GO below the highest pull of its press is on its way up. Letting go
// turns the controller as the pull does, so a press that still holds its point keeps it then.
const LET_GO: f32 = 0.15;

// How far a hand must move a panel by its handle before the panel moves, in dp of the panel. A dp
// is a millimeter in the room. The value is three times Compose's touch slop of 8 dp. Each press
// logs how far it strayed, so the value can follow the strays of real presses.
pub(crate) const DRAG_SLOP: f32 = 24.0;
// How far a fingertip that presses a panel slides along the face before the press drags, in dp.
// Until then the press holds its point, so the jitter of a tracked finger does not move it off a
// button. A dp is a millimeter in the room.
const TOUCH_SLOP: f32 = 12.0;

// What a frame gives a pointer besides its aim: the time, the time since the frame before in
// seconds, what the aims meet, how the controller's buttons map to the pointer's and whether a hand
// touches the panels.
#[derive(Clone, Copy)]
pub(crate) struct Frame<'a> {
    pub(crate) time: Duration,
    pub(crate) dt: f32,
    pub(crate) scene: Scene<'a>,
    pub(crate) mapping: PointerMapping,
    pub(crate) direct_touch: bool,
}

// A press in progress.
#[derive(Clone, Copy, Debug)]
struct Press {
    // Where the press's events go, also after the ray or the finger left the panel
    target: Target,
    // The pointer button that the press holds down, as the mapping gave it at the pull, or `None`
    // for a trigger that presses nothing
    button: Option<PointerButton>,
    // The press's point while it holds; None once it drags
    held: Option<Hit>,
    by: PressBy,
}

// What holds a press down.
#[derive(Clone, Copy, Debug)]
enum PressBy {
    // The trigger or the pinch, with the ray
    Ray {
        start: Duration,
        // The direction that the ray pointed in at the pull
        direction: [f32; 3],
        // The highest pull of the trigger since the press began
        peak: f32,
        // The mesh of a volume that the press began on
        entity: Option<MeshRef>,
        drift: Drift,
    },
    // The fingertip on the panel's face
    Touch,
}

// The pointer of one hand.
#[derive(Debug)]
pub(crate) struct Pointer {
    side: Side,
    reach: Reach,
    press: Option<Press>,
    held: Option<Held>,
    // When the hand took hold of the handle that it holds
    grabbed_at: Duration,
    // The target that the pointer is over and its place there, in the target's dp
    over: Option<(Target, (f32, f32))>,
    extras: Extras,
    // The ray behind the frame's gestures on volumes
    volume_ray: Option<VolumeRay>,
    touch: DirectTouch,
    // Where the fingertip hovers or presses in this frame; `None` while the ray points
    contact: Option<Hit>,
}

impl Pointer {
    pub(crate) fn new(side: Side) -> Self {
        Self {
            side,
            reach: Reach::default(),
            press: None,
            held: None,
            grabbed_at: Duration::ZERO,
            over: None,
            extras: Extras::default(),
            volume_ray: None,
            touch: DirectTouch::default(),
            contact: None,
        }
    }

    // The gestures of a frame with this hand's `aim`, added to `out`.
    //
    // A fingertip that is near a panel takes the pointer from the ray: the ray's pull presses
    // nothing then, and its beam hides. A press keeps the input that began it until it ends, so a
    // ray's press or carry keeps the fingertip from touching, and a touch keeps the pinch from
    // pressing. A pinch that closed while the fingertip was near counts as held from before.
    pub(crate) fn update(&mut self, aim: Option<Aim>, frame: &Frame<'_>, out: &mut Gestures) {
        let pull = self.reach.update(self.side, aim, frame.time, frame.scene);
        let reach = self.reach;
        self.volume_ray = self.volume_ray_of(&reach, frame);
        // While the ray presses or carries, the fingertip counts as lost: after the ray's press it
        // must be seen for a frame in front of a face before it presses
        let by_ray = self.held.is_some()
            || self
                .press
                .is_some_and(|press| matches!(press.by, PressBy::Ray { .. }));
        let tip = aim
            .and_then(|aim| aim.tip)
            .filter(|_| frame.direct_touch && !by_ray);
        let contact = self.touch.update(tip, frame.scene, self.side);
        self.contact = contact.hit();
        match contact {
            Contact::Down(hit) => self.touch_down(hit, out),
            Contact::On(hit) => self.touch_on(hit, out),
            Contact::Up { near } => self.release(near.or(reach.hit), frame.time, out),
            Contact::Near(hit) => self.hover(Some(hit), out),
            Contact::Away => self.point(pull, &reach, frame, out),
        }
        self.press_others(
            aim.map_or_else(Buttons::default, |aim| aim.buttons),
            frame,
            out,
        );
        self.scroll(&reach, frame, out);
    }

    // What the ray and the trigger do in a frame in which the fingertip touches nothing.
    fn point(&mut self, pull: Pull, reach: &Reach, frame: &Frame<'_>, out: &mut Gestures) {
        match pull {
            Pull::Pulled => match reach.press_target(self.side, frame.time, frame.scene) {
                Some(PressTarget::Handle(handle)) => self.grab(reach, &handle, frame, out),
                Some(PressTarget::Panel(hit)) => self.pull(reach, hit, frame, out),
                None => self.hover(reach.hit, out),
            },
            Pull::Up if self.held.is_some() => self.put_down(reach, frame.time, out),
            Pull::Up if self.press.is_some() => self.release(reach.hit, frame.time, out),
            _ if self.held.is_some() => self.hold(reach, frame, out),
            _ => self.follow(reach, frame, out),
        }
    }

    // The cursor to show on a panel with content: the point that a press holds, or where the
    // fingertip or the ray is over the panel. The dot shows a touch's press as it shows a ray's.
    pub(crate) fn cursor(&self) -> Option<Cursor> {
        match self.press {
            Some(Press {
                held: Some(held), ..
            }) => held.on_content().then(|| held.cursor(true)),
            _ if self.held.is_some() => None,
            press => {
                let pressed = press.is_some_and(|press| press.target == Target::Atlas);
                let over = match self.contact {
                    Some(touch) => Some(touch).filter(Hit::on_content),
                    None => self.reach.content_hit,
                };
                over.map(|hit| hit.cursor(pressed))
            }
        }
    }

    // The surface or volume that a press is on, which takes the gestures until the release.
    pub(crate) fn pressed_surface(&self) -> Option<u64> {
        match self.press?.target {
            Target::Surface(surface) => Some(surface),
            Target::Atlas => None,
        }
    }

    // The handle that the pointer presses, or else the one that a pull would grab. A press on a
    // panel shows no handle: it cannot become a carry. A fingertip near a panel grabs no handle,
    // since its touch presses the panel.
    pub(crate) fn handle(&self) -> Option<HandleFocus> {
        self.held.map(|held| held.handle()).or_else(|| {
            if self.press.is_some() || self.contact.is_some() {
                return None;
            }
            let handle = self.reach.grab()?;
            Some(HandleFocus {
                panel: handle.slot.id,
                kind: handle.kind,
                pressed: false,
            })
        })
    }

    // The ray to show, or `None` while it shows nothing. The fingertip hides the beam while it is
    // near a panel.
    pub(crate) fn beam(&self) -> Option<Beam> {
        self.contact.is_none().then(|| self.reach.beam()).flatten()
    }

    // The panel that this hand holds by a handle, which the other hand cannot grab.
    pub(crate) fn held_panel(&self) -> Option<u64> {
        self.held.map(|held| held.handle().panel)
    }

    // A press starts where the trigger goes down over a panel, at `hit`, which the ray from
    // before the pull meets. A trigger that was down before the ray reached a panel does not
    // press. The controller ticks, so the hand feels that the press took.
    fn pull(&mut self, reach: &Reach, hit: Hit, frame: &Frame<'_>, out: &mut Gestures) {
        let Some(ray) = reach.ray else {
            return;
        };
        self.move_to(hit.target(), hit.position(), out);
        // The press goes where the ray pointed before the pull, which may be another mesh than the
        // one it points at now
        if let Some(volume_ray) = &mut self.volume_ray {
            volume_ray.hit = hit.pick.map(ray_hit);
        }
        let button = frame.mapping.button(ControllerInput::Trigger);
        if let Some(button) = button {
            self.emit(out, hit.target(), Gesture::Press(button));
        }
        self.press = Some(Press {
            target: hit.target(),
            button,
            held: Some(hit),
            by: PressBy::Ray {
                start: frame.time,
                direction: ray.direction,
                peak: reach.trigger,
                entity: hit.pick.map(|pick| pick.mesh),
                drift: Drift::begin(&hit.slot, &ray, frame.time),
            },
        });
        self.tick(out);
    }

    // The fingertip went into a panel's face at `hit`: a press of the primary button starts there,
    // as a finger's press does on a screen, and the press ticks as a ray's does.
    fn touch_down(&mut self, hit: Hit, out: &mut Gestures) {
        self.move_to(hit.target(), hit.position(), out);
        let button = PointerButton::Primary;
        self.emit(out, hit.target(), Gesture::Press(button));
        self.press = Some(Press {
            target: hit.target(),
            button: Some(button),
            held: Some(hit),
            by: PressBy::Touch,
        });
        self.tick(out);
    }

    // The fingertip stays on the panel that it pressed, over `hit`. The press holds its point
    // until the fingertip slides TOUCH_SLOP from it, and then follows the fingertip.
    fn touch_on(&mut self, hit: Hit, out: &mut Gestures) {
        let Some(press) = &mut self.press else {
            return;
        };
        if press
            .held
            .is_some_and(|held| (hit.x - held.x).hypot(hit.y - held.y) > TOUCH_SLOP)
        {
            press.held = None;
        }
        if press.held.is_none() {
            self.move_to(hit.target(), hit.position(), out);
        }
    }

    // The trigger went up, or the fingertip left the panel that it pressed, while a press was on:
    // the release goes to the press's target. A press that still held its point clicks and ticks
    // again. One that dragged ends with no tick. The pointer then hovers over `hover`.
    fn release(&mut self, hover: Option<Hit>, time: Duration, out: &mut Gestures) {
        if let Some(press) = self.press.take() {
            if let Some(button) = press.button {
                self.emit(out, press.target, Gesture::Release(button));
            }
            let clicked = press.held.is_some();
            if let PressBy::Ray { drift, .. } = press.by {
                drift.report(self.side, time, if clicked { "a click" } else { "a drag" });
            }
            if clicked {
                self.tick(out);
            }
        }
        self.hover(hover, out);
    }

    // Asks for the light vibration that tells the hand that a press took or ended.
    fn tick(&self, out: &mut Gestures) {
        out.ticks[self.side.index()] = true;
    }

    // Moves with the ray: while no press is on, or once a press drags. A press that drags keeps
    // its target: it follows the ray over that target only.
    fn follow(&mut self, reach: &Reach, frame: &Frame<'_>, out: &mut Gestures) {
        let time = frame.time;
        let Some(press) = &mut self.press else {
            self.hover(reach.hit, out);
            return;
        };
        // A touch's press follows the fingertip, in `touch_on`
        let PressBy::Ray {
            start,
            direction,
            peak,
            drift,
            ..
        } = &mut press.by
        else {
            return;
        };
        *peak = peak.max(reach.trigger);
        if let Some(ray) = reach.ray {
            drift.note(&ray, frame.scene);
        }
        let letting_go = reach.trigger < *peak - LET_GO;
        if press.held.is_some()
            && !letting_go
            && let Some(ray) = reach.ray
        {
            let turn = angle_between(*direction, ray.direction).to_degrees();
            let limit = if time.saturating_sub(*start) < CLICK_TIME {
                CLICK_ANGLE
            } else {
                DRAG_ANGLE
            };
            if turn > limit {
                press.held = None;
            }
        }
        if press.held.is_none()
            && let Some(hit) = reach.hit.filter(|hit| hit.target() == press.target)
        {
            self.move_to(hit.target(), hit.position(), out);
        }
    }

    fn hover(&mut self, hit: Option<Hit>, out: &mut Gestures) {
        match hit {
            Some(hit) => self.move_to(hit.target(), hit.position(), out),
            None => self.leave(out),
        }
    }

    // Moves the pointer to `position` on `target`. The target that it was over sees it leave.
    fn move_to(&mut self, target: Target, position: (f32, f32), out: &mut Gestures) {
        if self.over == Some((target, position)) {
            return;
        }
        if self.over.is_some_and(|(over, _)| over != target) {
            self.leave(out);
        }
        self.emit(
            out,
            target,
            Gesture::Move {
                x: position.0,
                y: position.1,
            },
        );
        self.over = Some((target, position));
    }

    fn leave(&mut self, out: &mut Gestures) {
        if let Some((target, _)) = self.over.take() {
            self.emit(out, target, Gesture::Leave);
        }
    }

    fn emit(&self, out: &mut Gestures, target: Target, gesture: Gesture) {
        match target {
            Target::Atlas => out.atlas.push(HandGesture {
                hand: self.side,
                gesture,
            }),
            Target::Surface(surface) => out.surfaces.push(SurfaceGesture {
                surface,
                hand: self.side,
                gesture,
                ray: self.volume_ray,
            }),
        }
    }

    // The ray behind this frame's gestures on volumes: this hand's, with the mesh that it meets. A
    // press that began on a mesh keeps that mesh, and meets it again where the ray still does.
    fn volume_ray_of(&self, reach: &Reach, frame: &Frame<'_>) -> Option<VolumeRay> {
        let (ray, source) = (reach.ray?, reach.source()?);
        let hit = match self.press {
            Some(Press {
                target: Target::Surface(volume),
                by: PressBy::Ray {
                    entity: Some(mesh), ..
                },
                ..
            }) => {
                let slot = frame.scene.panels.iter().find(|slot| slot.id == volume)?;
                let again =
                    frame
                        .scene
                        .picks
                        .pick(volume, &ContentRay::of(&ray, &slot.pose), Some(mesh));
                Some(RayHit {
                    mesh,
                    place: again.map(|pick| (pick.triangle, pick.position)),
                })
            }
            _ => reach.hit.and_then(|hit| hit.pick).map(ray_hit),
        };
        Some(VolumeRay {
            source,
            hand: self.side,
            time: frame.time,
            ray,
            hit,
        })
    }
}
