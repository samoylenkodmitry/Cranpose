//! What one hand's ray reaches in a frame: the panel it meets, the handle it meets, and how far its
//! trigger is pulled.

use std::time::Duration;

use crate::{
    Hand as Side, InputSource, Pose, Ray,
    aim::Aim,
    aim_filter::{AimFilter, angle_between},
    handle::HandleKind,
    hit::{Hit, nearest_hit, reachable},
    press_threshold::{Was, pressed},
    ray::Beam,
    space::PanelSlot,
    volume_pick::PickMeshes,
};

// A trigger below REST is untouched. The pull of a trigger turns the controller, so a press
// points where the ray pointed in the last frame of rest. A pull that took longer than PULL_TIME,
// or a ray that turned CLICK_ANGLE degrees since that frame, presses where the ray points now:
// the user aimed anew.
const REST: f32 = 0.1;
const PULL_TIME: Duration = Duration::from_millis(500);
pub(crate) const CLICK_ANGLE: f32 = 10.0;
// A ray that meets no panel and no handle shows this long, in meters.
const RAY_LENGTH: f32 = 3.0;
// A thumbstick within DEAD_ZONE of its middle moves nothing.
const DEAD_ZONE: f32 = 0.15;

// A handle that a ray meets: which panel's, and what it does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Handle {
    pub(crate) slot: PanelSlot,
    pub(crate) kind: HandleKind,
}

// What a pull takes: a panel's handle, or a place on a panel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PressTarget {
    Handle(Handle),
    Panel(Hit),
}

// A trigger in a frame, compared with the frame before.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Pull {
    Up,
    // Down now, up in the frame before.
    Pulled,
    Held,
}

// What the world offers one hand's ray in a frame: the panels, the meshes of the volumes among
// them, and the panel that the other hand holds by a handle, which this hand cannot grab.
#[derive(Clone, Copy)]
pub(crate) struct Scene<'a> {
    pub(crate) panels: &'a [PanelSlot],
    pub(crate) picks: &'a PickMeshes,
    pub(crate) claimed: Option<u64>,
}

// One controller or tracked hand across frames.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Reach {
    // What drove this side's aim in the last frame
    source: Option<InputSource>,
    filter: AimFilter,
    down: bool,
    // The ray in the last frame where the trigger rested, and that frame's time
    rest: Option<(Duration, Ray)>,
    // This frame's ray, the panel it meets and the panel whose handle it meets. A panel
    // that holds no content, such as a video surface or a volume, stops the ray as a panel does.
    pub(crate) ray: Option<Ray>,
    pub(crate) hit: Option<Hit>,
    // The hit, if the panel has content, which shows the cursor's dot
    pub(crate) content_hit: Option<Hit>,
    handle: Option<(Handle, f32)>,
    // The controller's own pose and its thumbstick's lean to the right and forward, which scroll a
    // panel or carry one
    pub(crate) pose: Option<Pose>,
    pub(crate) stick: [f32; 2],
    // How far the trigger is pulled in this frame
    pub(crate) trigger: f32,
    // Whether any panel is in reach of this hand's ray, which decides whether the ray shows
    in_reach: bool,
}

impl Reach {
    pub(crate) fn update(
        &mut self,
        side: Side,
        aim: Option<Aim>,
        time: Duration,
        scene: Scene<'_>,
    ) -> Pull {
        let Some(aim) = aim else {
            // A hand the runtime stops tracking lets its press go
            *self = Self::default();
            return Pull::Up;
        };
        if self.source.is_some_and(|source| source != aim.source) {
            // The other kind of input takes the side over, with no frame between. The press of the
            // one before ends as if its aim were lost, so it cannot go on with the other's
            // trigger. A trigger that is down at the switch counts as held from before, so it
            // presses only after it went up.
            *self = Self {
                source: Some(aim.source),
                down: pressed(aim.trigger, Was::Up),
                ..Self::default()
            };
            return Pull::Up;
        }
        if self.source.is_none() && aim.source == InputSource::Hands {
            // A pinch that is closed when the hand appears counts as held from before: a hand that
            // the cameras lose and find again, or that ends the system's menu gesture with its
            // fingers together, does not press until it opens and pinches anew.
            self.down = pressed(aim.trigger, Was::Up);
        }
        self.source = Some(aim.source);
        let ray = self.filter.apply(aim.ray, time);
        self.ray = Some(ray);
        self.hit = nearest_hit(&ray, scene.panels, side, scene.picks);
        self.content_hit = self.hit.filter(Hit::on_content);
        self.handle = nearest_handle(&ray, scene, side);
        self.in_reach = scene.panels.iter().any(|slot| reachable(slot, side));
        self.pose = Some(aim.pose);
        self.trigger = aim.trigger;
        self.stick = aim
            .stick
            .map(|lean| if lean.abs() > DEAD_ZONE { lean } else { 0.0 });
        let was_down = self.down;
        self.down = pressed(aim.trigger, was_down.into());
        if aim.trigger < REST {
            self.rest = Some((time, ray));
        }
        match (self.down, was_down) {
            (false, _) => Pull::Up,
            (true, false) => Pull::Pulled,
            (true, true) => Pull::Held,
        }
    }

    // The ray that a press in this frame takes: the one from just before the pull, if the ray
    // still points near it, else the ray of this frame.
    fn press_ray(&self, time: Duration) -> Option<Ray> {
        let now = self.ray?;
        let before = self.rest.filter(|(rest, ray)| {
            time.saturating_sub(*rest) <= PULL_TIME
                && angle_between(ray.direction, now.direction).to_degrees() < CLICK_ANGLE
        });
        Some(before.map_or(now, |(_, ray)| ray))
    }

    // What a pull in this frame takes, judged by the ray from just before the pull as a press
    // is: the handle that this ray meets nearer than any panel, or else the panel that it meets.
    // A pull turns the controller, and a turn toward a handle below a button must not carry the
    // panel that the user pressed the button of.
    pub(crate) fn press_target(
        &self,
        side: Side,
        time: Duration,
        scene: Scene<'_>,
    ) -> Option<PressTarget> {
        let ray = self.press_ray(time)?;
        let hit = nearest_hit(&ray, scene.panels, side, scene.picks);
        let handle = nearest_handle(&ray, scene, side)
            .filter(|(_, distance)| hit.is_none_or(|hit| *distance < hit.distance));
        match (handle, hit) {
            (Some((handle, _)), _) => Some(PressTarget::Handle(handle)),
            (None, Some(hit)) => Some(PressTarget::Panel(hit)),
            (None, None) => None,
        }
    }

    // What drove this hand's aim in the frame, or `None` while nothing points.
    pub(crate) fn source(&self) -> Option<InputSource> {
        self.source
    }

    // The handle that a pull in this frame grabs: one that is nearer than any panel.
    pub(crate) fn grab(&self) -> Option<Handle> {
        let (handle, distance) = self.handle?;
        self.hit
            .is_none_or(|hit| distance < hit.distance)
            .then_some(handle)
    }

    // The ray up to the nearest panel or handle that it meets, or `None` while no panel is in
    // reach of this hand's ray: a ray that points at nothing the app shows has no beam.
    pub(crate) fn beam(&self) -> Option<Beam> {
        let ray = self.ray.filter(|_| self.in_reach)?;
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

// The handle that the ray of `side` meets first, and how far along the ray. The handles of a panel
// that the other hand holds are out of reach.
fn nearest_handle(ray: &Ray, scene: Scene<'_>, side: Side) -> Option<(Handle, f32)> {
    scene
        .panels
        .iter()
        .filter(|slot| reachable(slot, side) && scene.claimed != Some(slot.id))
        .flat_map(|slot| {
            HandleKind::ALL
                .into_iter()
                .filter(|kind| kind.offered_by(slot))
                .filter_map(move |kind| Some((Handle { slot: *slot, kind }, kind.hit(ray, slot)?)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}
