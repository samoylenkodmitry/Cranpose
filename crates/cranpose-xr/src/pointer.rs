//! One pointer over the panels, from the rays and triggers of two controllers or of two tracked
//! hands.

use std::time::Duration;

use crate::{
    Hand as Side, InputSource, Pose, Ray, SpatialMoveEventType, SpatialResizeEventType,
    aim_filter::{AimFilter, angle_between},
    carry::Carry,
    handle::HandleKind,
    ray::Beam,
    resize::Resize,
    space::{Cursor, PanelKind, PanelMove, PanelResize, PanelSlot, Ride},
    surface_input::{Gesture, SurfaceGesture},
};

// A trigger counts as down above PRESS and as up below RELEASE, so a trigger held near one value
// does not flicker.
const PRESS: f32 = 0.6;
const RELEASE: f32 = 0.4;
// A trigger below REST is untouched. The pull of a trigger turns the controller, so a press
// points where the ray pointed in the last frame of rest. A pull that took longer than PULL_TIME,
// or a ray that turned CLICK_ANGLE degrees since that frame, presses where the ray points now:
// the user aimed anew.
const REST: f32 = 0.1;
const PULL_TIME: Duration = Duration::from_millis(500);
// A press holds its point while the hand shakes: until the ray turns CLICK_ANGLE degrees from
// where it pointed at the pull in the press's first CLICK_TIME, or DRAG_ANGLE degrees later. Then
// the press drags.
const CLICK_TIME: Duration = Duration::from_millis(350);
const CLICK_ANGLE: f32 = 10.0;
const DRAG_ANGLE: f32 = 4.0;
// A trigger that falls LET_GO below the highest pull of its press is on its way up. Letting go
// turns the controller as the pull does, so a press that still holds its point keeps it then.
const LET_GO: f32 = 0.15;
// A ray that meets no panel and no handle shows this long, in meters.
const RAY_LENGTH: f32 = 3.0;
// A thumbstick pushed all the way moves a carried panel by this share of its distance each
// second. A thumbstick within DEAD_ZONE of its middle moves nothing.
const PUSH_SPEED: f32 = 1.2;
const DEAD_ZONE: f32 = 0.15;

// What a controller or a tracked hand does in a frame: where it points, how far its trigger is
// pulled or its index finger pinches from 0 to 1, and how far its thumbstick is pushed forward
// from -1 to 1. A hand has no thumbstick, so its stick stays at 0.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Aim {
    pub(crate) ray: Ray,
    pub(crate) pose: Pose,
    pub(crate) trigger: f32,
    pub(crate) stick: f32,
    pub(crate) source: InputSource,
}

// A trigger in a frame, compared with the frame before.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Trigger {
    Up,
    // Down now, up in the frame before.
    Pulled,
    Held,
}

// Where the pointer's events go: the app's composition, which one atlas holds for every panel with
// content, or one surface or volume, which take the events as a callback of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Atlas,
    Surface(u64),
}

// Where a ray meets a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Hit {
    slot: PanelSlot,
    // The place on the panel in dp from its top left corner
    x: f32,
    y: f32,
    // How far along the ray, in meters
    distance: f32,
}

impl Hit {
    // Whether the panel has Cranpose content that the pointer can move over and press.
    fn on_content(&self) -> bool {
        self.slot.kind == PanelKind::Content
    }

    // Where the pointer's events about this hit go.
    fn target(&self) -> Target {
        match self.slot.kind {
            PanelKind::Content => Target::Atlas,
            PanelKind::Surface(_) | PanelKind::Volume { .. } => Target::Surface(self.slot.id),
        }
    }

    fn position(&self) -> (f32, f32) {
        (self.slot.atlas[0] + self.x, self.slot.atlas[1] + self.y)
    }

    fn cursor(&self, pressed: bool) -> Cursor {
        Cursor {
            panel: self.slot.id,
            x: self.x,
            y: self.y,
            pressed,
        }
    }
}

// A handle that a ray meets: which panel's, and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Handle {
    slot: PanelSlot,
    kind: HandleKind,
}

// One controller or tracked hand across frames.
#[derive(Clone, Copy, Debug, Default)]
struct Hand {
    // What drove this side's aim in the last frame
    source: Option<InputSource>,
    filter: AimFilter,
    down: bool,
    // The ray in the last frame where the trigger rested, and that frame's time
    rest: Option<(Duration, Ray)>,
    // This frame's ray, the panel it meets and the panel whose handle it meets. A panel
    // that holds no content, such as a video surface or a volume, stops the ray as a panel does.
    ray: Option<Ray>,
    hit: Option<Hit>,
    // The hit, if the panel has content, which shows the cursor's dot
    content_hit: Option<Hit>,
    handle: Option<(Handle, f32)>,
    // The controller's own pose and its thumbstick, which carry a panel
    pose: Option<Pose>,
    stick: f32,
    // How far the trigger is pulled in this frame
    trigger: f32,
}

impl Hand {
    fn update(
        &mut self,
        side: Side,
        aim: Option<Aim>,
        time: Duration,
        panels: &[PanelSlot],
    ) -> Trigger {
        let Some(aim) = aim else {
            // A hand the runtime stops tracking lets its press go
            *self = Self::default();
            return Trigger::Up;
        };
        if self.source.is_some_and(|source| source != aim.source) {
            // The other kind of input takes the side over, with no frame between. The press of the
            // one before ends as if its aim were lost, so it cannot go on with the other's
            // trigger. A trigger that is down at the switch counts as held from before, so it
            // presses only after it went up.
            *self = Self {
                source: Some(aim.source),
                down: aim.trigger >= PRESS,
                ..Self::default()
            };
            return Trigger::Up;
        }
        if self.source.is_none() && aim.source == InputSource::Hands {
            // A pinch that is closed when the hand appears counts as held from before: a hand that
            // the cameras lose and find again, or that ends the system's menu gesture with its
            // fingers together, does not press until it opens and pinches anew.
            self.down = aim.trigger >= PRESS;
        }
        self.source = Some(aim.source);
        let ray = self.filter.apply(aim.ray, time);
        self.ray = Some(ray);
        self.hit = nearest_hit(&ray, panels, side);
        self.content_hit = self.hit.filter(Hit::on_content);
        self.handle = nearest_handle(&ray, panels, side);
        self.pose = Some(aim.pose);
        self.trigger = aim.trigger;
        self.stick = if aim.stick.abs() > DEAD_ZONE {
            aim.stick
        } else {
            0.0
        };
        let was_down = self.down;
        self.down = if was_down {
            aim.trigger > RELEASE
        } else {
            aim.trigger >= PRESS
        };
        if aim.trigger < REST {
            self.rest = Some((time, ray));
        }
        match (self.down, was_down) {
            (false, _) => Trigger::Up,
            (true, false) => Trigger::Pulled,
            (true, true) => Trigger::Held,
        }
    }

    // The hit that a press in this frame takes: the one from just before the pull, if the ray
    // still points near it.
    fn press_hit(&self, side: Side, time: Duration, panels: &[PanelSlot]) -> Option<Hit> {
        let now = self.ray?;
        let before = self
            .rest
            .filter(|(rest, ray)| {
                time.saturating_sub(*rest) <= PULL_TIME
                    && angle_between(ray.direction, now.direction).to_degrees() < CLICK_ANGLE
            })
            .and_then(|(_, ray)| nearest_hit(&ray, panels, side));
        before.or(self.hit)
    }

    // Whether the ray meets a panel or a handle.
    fn points(&self) -> bool {
        self.hit.is_some() || self.handle.is_some()
    }

    // The handle that a pull in this frame grabs: one that is nearer than any panel.
    fn grab(&self) -> Option<Handle> {
        let (handle, distance) = self.handle?;
        self.hit
            .is_none_or(|hit| distance < hit.distance)
            .then_some(handle)
    }

    // The ray up to the nearest panel or handle that it meets.
    fn beam(&self) -> Option<Beam> {
        let ray = self.ray?;
        let hit = self.hit.map(|hit| hit.distance);
        let handle = self.handle.map(|(_, distance)| distance);
        let length = match (hit, handle) {
            (Some(hit), Some(handle)) => hit.min(handle),
            (Some(distance), None) | (None, Some(distance)) => distance,
            (None, None) => RAY_LENGTH,
        };
        Some(Beam { ray, length })
    }
}

// A press in progress.
#[derive(Clone, Copy, Debug)]
struct Press {
    // Where the press's events go, also after the ray left the panel
    target: Target,
    start: Duration,
    // The direction that the ray pointed in at the pull
    direction: [f32; 3],
    // The highest pull of the trigger since the press began
    peak: f32,
    // The press's point while it holds; None once it drags
    held: Option<Hit>,
}

// A panel the user carries.
#[derive(Clone, Copy, Debug)]
struct Carried {
    panel: u64,
    carry: Carry,
    // The panel's pose in the last step, which a step that ends the carry repeats when the
    // controller is lost
    pose: Pose,
}

// A panel the user resizes.
#[derive(Clone, Copy, Debug)]
struct Resized {
    panel: u64,
    resize: Resize,
    // The panel's size in dp in the last step, which a step that ends the resize repeats when the
    // controller is lost
    size: [f32; 2],
    // The highest pull of the trigger since the grab
    peak: f32,
}

// What the user holds by a handle.
#[derive(Clone, Copy, Debug)]
enum Held {
    Carry(Carried),
    Resize(Resized),
}

impl Held {
    fn handle(&self) -> (u64, HandleKind) {
        match self {
            Self::Carry(carried) => (carried.panel, HandleKind::Move),
            Self::Resize(resized) => (resized.panel, HandleKind::Resize),
        }
    }
}

// The pointer. One controller or hand drives it at a time: the one whose trigger went down last,
// or the other one while only that one points at a panel or a handle.
#[derive(Debug, Default)]
pub(crate) struct Pointer {
    // The controller that drives the pointer: 0 for the left hand, 1 for the right
    hand: usize,
    hands: [Hand; 2],
    press: Option<Press>,
    held: Option<Held>,
    // The steps of the carry and of the resize in this frame, kept so a frame does not allocate
    moves: Vec<PanelMove>,
    resizes: Vec<PanelResize>,
    // The time of the frame before
    time: Option<Duration>,
    // The target that the pointer is over and its place there, in the target's dp
    over: Option<(Target, (f32, f32))>,
    // The frame's events for the composition and for the surfaces, kept so a frame does not
    // allocate
    events: Vec<Gesture>,
    surface_events: Vec<SurfaceGesture>,
}

impl Pointer {
    // The events of a frame at `time` with `aims` over `panels`, and the cursor to show.
    pub(crate) fn update(
        &mut self,
        aims: [Option<Aim>; 2],
        time: Duration,
        panels: &[PanelSlot],
    ) -> (&[Gesture], Option<Cursor>) {
        self.events.clear();
        self.surface_events.clear();
        self.moves.clear();
        self.resizes.clear();
        let dt = self
            .time
            .map_or(0.0, |last| time.saturating_sub(last).as_secs_f32());
        self.time = Some(time);
        let triggers = Side::BOTH
            .map(|side| self.hands[side.index()].update(side, aims[side.index()], time, panels));
        self.choose_hand(triggers);
        let hand = self.hands[self.hand];
        match triggers[self.hand] {
            Trigger::Pulled => match hand.grab() {
                Some(handle) => self.grab(&hand, &handle),
                None => self.pull(&hand, time, panels),
            },
            Trigger::Up if self.held.is_some() => self.put_down(&hand),
            Trigger::Up if self.press.is_some() => self.release(&hand),
            _ if self.held.is_some() => self.hold(&hand, dt, panels),
            _ => self.follow(&hand, time),
        }
        let cursor = self.cursor(&hand);
        (&self.events, cursor)
    }

    // The cursor to show on a panel with content: the point that a press holds, or where the ray
    // meets the panel.
    fn cursor(&self, hand: &Hand) -> Option<Cursor> {
        match self.press {
            Some(Press {
                held: Some(held), ..
            }) => held.on_content().then(|| held.cursor(true)),
            _ if self.held.is_some() => None,
            press => {
                let pressed = press.is_some_and(|press| press.target == Target::Atlas);
                hand.content_hit.map(|hit| hit.cursor(pressed))
            }
        }
    }

    // The trigger went up while a press was on: the release goes to the press's target.
    fn release(&mut self, hand: &Hand) {
        if let Some(press) = self.press.take() {
            self.emit(press.target, Gesture::Release);
        }
        self.hover(hand.hit);
    }

    // The steps of the user's carry in this frame: the grab, the new pose of each frame in which
    // the panel moved, and the release.
    pub(crate) fn moves(&self) -> &[PanelMove] {
        &self.moves
    }

    // The steps of the user's resize in this frame: the grab, the new size of each frame in which
    // it changed, and the release.
    pub(crate) fn resizes(&self) -> &[PanelResize] {
        &self.resizes
    }

    // The pointer's gestures on surfaces in this frame.
    pub(crate) fn surface_events(&self) -> &[SurfaceGesture] {
        &self.surface_events
    }

    // The panel whose handle the pointer holds or would grab, and which of its handles.
    pub(crate) fn handle(&self) -> Option<(u64, HandleKind)> {
        self.held.map(|held| held.handle()).or_else(|| {
            let handle = self.hands[self.hand].grab()?;
            Some((handle.slot.id, handle.kind))
        })
    }

    // The rays to show, left then right.
    pub(crate) fn beams(&self) -> [Option<Beam>; 2] {
        self.hands.map(|hand| hand.beam())
    }

    // A trigger that goes down takes the pointer to its controller. While no press is on, a
    // controller that points at a panel or a handle takes it from one that points at neither. A press keeps
    // its controller until the trigger goes up.
    fn choose_hand(&mut self, triggers: [Trigger; 2]) {
        if self.press.is_some() || self.held.is_some() {
            return;
        }
        let pulled = (0..2).rev().find(|&hand| triggers[hand] == Trigger::Pulled);
        if let Some(hand) = pulled {
            self.hand = hand;
        } else if !self.hands[self.hand].points() && self.hands[1 - self.hand].points() {
            self.hand = 1 - self.hand;
        }
    }

    // A press starts where the trigger goes down over a panel. A trigger that was down before
    // the ray reached a panel does not press.
    fn pull(&mut self, hand: &Hand, time: Duration, panels: &[PanelSlot]) {
        match (
            hand.press_hit(Side::BOTH[self.hand], time, panels),
            hand.ray,
        ) {
            (Some(hit), Some(ray)) => {
                self.move_to(hit.target(), hit.position());
                self.emit(hit.target(), Gesture::Press);
                self.press = Some(Press {
                    target: hit.target(),
                    start: time,
                    direction: ray.direction,
                    peak: hand.trigger,
                    held: Some(hit),
                });
            }
            _ => self.hover(hand.hit),
        }
    }

    // A pull on a handle takes hold of its panel: a move handle carries the panel along with the
    // controller, and a corner handle resizes it. The panel's composition sees the pointer leave.
    fn grab(&mut self, hand: &Hand, handle: &Handle) {
        let slot = &handle.slot;
        let grabbed = match handle.kind {
            HandleKind::Move => hand.pose.map(|aim| {
                self.step(slot.id, SpatialMoveEventType::Start, slot.pose);
                Held::Carry(Carried {
                    panel: slot.id,
                    carry: Carry::grab(&aim, &slot.pose),
                    pose: slot.pose,
                })
            }),
            HandleKind::Resize => hand
                .ray
                .and_then(|ray| Resize::grab(&ray, slot))
                .map(|resize| {
                    let size = slot.size();
                    self.resize_step(slot.id, SpatialResizeEventType::Start, size);
                    Held::Resize(Resized {
                        panel: slot.id,
                        resize,
                        size,
                        peak: hand.trigger,
                    })
                }),
        };
        if grabbed.is_some() {
            self.held = grabbed;
            self.hover(None);
        }
    }

    // The held panel follows the controller.
    fn hold(&mut self, hand: &Hand, dt: f32, panels: &[PanelSlot]) {
        match self.held {
            Some(Held::Carry(_)) => self.carry(hand, dt),
            Some(Held::Resize(_)) => self.resize(hand, panels),
            None => {}
        }
    }

    // The carried panel follows the controller. The thumbstick pushes it away or pulls it near.
    fn carry(&mut self, hand: &Hand, dt: f32) {
        let (Some(Held::Carry(carried)), Some(aim)) = (&mut self.held, hand.pose) else {
            return;
        };
        let carry = &mut carried.carry;
        carry.push(hand.stick * PUSH_SPEED * carry.distance() * dt);
        let pose = carry.pose(&aim);
        if pose != carried.pose {
            carried.pose = pose;
            let panel = carried.panel;
            self.step(panel, SpatialMoveEventType::Moving, pose);
        }
    }

    // The resized panel takes the size that the ray gives it. A ray that meets the panel's plane
    // nowhere leaves the size as it was. So does a trigger that is on its way up: letting go turns
    // the controller as a pull does.
    fn resize(&mut self, hand: &Hand, panels: &[PanelSlot]) {
        let (Some(Held::Resize(resized)), Some(ray)) = (&mut self.held, hand.ray) else {
            return;
        };
        resized.peak = resized.peak.max(hand.trigger);
        if hand.trigger < resized.peak - LET_GO {
            return;
        }
        let Some(slot) = panels.iter().find(|slot| slot.id == resized.panel) else {
            return;
        };
        let Some(size) = resized.resize.size(&ray, &slot.face()) else {
            return;
        };
        // Sizes are whole dp, so two equal sizes compare equal
        #[expect(clippy::float_cmp, reason = "the sizes are rounded to whole dp")]
        let changed = size != resized.size;
        if changed {
            resized.size = size;
            let panel = resized.panel;
            self.resize_step(panel, SpatialResizeEventType::Resizing, size);
        }
    }

    // The trigger went up while the user held a panel by a handle, or the controller was lost.
    fn put_down(&mut self, hand: &Hand) {
        match self.held.take() {
            Some(Held::Carry(carried)) => {
                self.step(carried.panel, SpatialMoveEventType::End, carried.pose);
            }
            Some(Held::Resize(resized)) => {
                self.resize_step(resized.panel, SpatialResizeEventType::End, resized.size);
            }
            None => {}
        }
        self.hover(hand.hit);
    }

    fn step(&mut self, panel: u64, event_type: SpatialMoveEventType, pose: Pose) {
        self.moves.push(PanelMove {
            panel,
            event_type,
            pose,
        });
    }

    fn resize_step(&mut self, panel: u64, event_type: SpatialResizeEventType, size: [f32; 2]) {
        self.resizes.push(PanelResize {
            panel,
            event_type,
            size,
        });
    }

    // Moves with the ray: while no press is on, or once a press drags. A press that drags keeps
    // its target: it follows the ray over that target only.
    fn follow(&mut self, hand: &Hand, time: Duration) {
        let Some(press) = &mut self.press else {
            self.hover(hand.hit);
            return;
        };
        press.peak = press.peak.max(hand.trigger);
        let letting_go = hand.trigger < press.peak - LET_GO;
        if press.held.is_some()
            && !letting_go
            && let Some(ray) = hand.ray
        {
            let turn = angle_between(press.direction, ray.direction).to_degrees();
            let limit = if time.saturating_sub(press.start) < CLICK_TIME {
                CLICK_ANGLE
            } else {
                DRAG_ANGLE
            };
            if turn > limit {
                press.held = None;
            }
        }
        if press.held.is_none()
            && let Some(hit) = hand.hit.filter(|hit| hit.target() == press.target)
        {
            self.move_to(hit.target(), hit.position());
        }
    }

    fn hover(&mut self, hit: Option<Hit>) {
        match hit {
            Some(hit) => self.move_to(hit.target(), hit.position()),
            None => self.leave(),
        }
    }

    // Moves the pointer to `position` on `target`. The target that it was over sees it leave.
    fn move_to(&mut self, target: Target, position: (f32, f32)) {
        if self.over == Some((target, position)) {
            return;
        }
        if self.over.is_some_and(|(over, _)| over != target) {
            self.leave();
        }
        self.emit(
            target,
            Gesture::Move {
                x: position.0,
                y: position.1,
            },
        );
        self.over = Some((target, position));
    }

    fn leave(&mut self) {
        if let Some((target, _)) = self.over.take() {
            self.emit(target, Gesture::Leave);
        }
    }

    fn emit(&mut self, target: Target, gesture: Gesture) {
        match target {
            Target::Atlas => self.events.push(gesture),
            Target::Surface(surface) => self
                .surface_events
                .push(SurfaceGesture { surface, gesture }),
        }
    }
}

// Whether the ray of the controller in hand `side` can meet the panel. A panel that stands in no
// place shows nowhere and takes no hits. A panel that rides a controller is out of reach of that
// controller's own ray: the left ray passes through the ball above the left controller.
fn reachable(slot: &PanelSlot, side: Side) -> bool {
    match slot.ride {
        Ride::Free => true,
        Ride::On(rides) => rides != side,
        Ride::Hidden => false,
    }
}

// The handle that the ray of `side` meets first, and how far along the ray.
fn nearest_handle(ray: &Ray, panels: &[PanelSlot], side: Side) -> Option<(Handle, f32)> {
    panels
        .iter()
        .filter(|slot| reachable(slot, side))
        .flat_map(|slot| {
            HandleKind::ALL
                .into_iter()
                .filter(|kind| kind.offered_by(slot))
                .filter_map(move |kind| Some((Handle { slot: *slot, kind }, kind.hit(ray, slot)?)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

// The panel the ray of `side` meets first and the place on it.
fn nearest_hit(ray: &Ray, panels: &[PanelSlot], side: Side) -> Option<Hit> {
    panels
        .iter()
        .filter(|slot| reachable(slot, side))
        .filter_map(|slot| {
            let size = slot.size();
            let hit = slot.hit(ray)?;
            Some(Hit {
                slot: *slot,
                x: hit.u * size[0],
                y: hit.v * size[1],
                distance: hit.distance,
            })
        })
        .min_by(|a, b| a.distance.total_cmp(&b.distance))
}

#[cfg(test)]
#[path = "tests/pointer_tests.rs"]
mod tests;
