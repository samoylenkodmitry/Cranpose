// A tray as a headset runs it: the scene draws its meshes on the GPU of the machine, a controller's
// ray presses its controls on the tray's panel through the frame loop's pointer, and the ray's pick
// measures where the meshes stand. A key sinks while pressed, a switch flips, a slider takes the
// value that a drag gives, and a tray that hid composes nothing and draws nothing.

use std::time::{Duration, Instant};

use super::*;
use crate::{
    ButtonShape, PointerMapping, Ray, SliderColors, SpatialButton, SpatialMoveEventType,
    SpatialSlider, SpatialSwitch, SpatialTray, SwitchColors,
    aim::Aim,
    atlas_input,
    content_ray::ContentRay,
    eye_camera::{EyeCamera, FirstRow},
    pointers::Pointers,
    space::{PanelKind, PanelMove, PanelSlot, field_dock::point_on},
    tray_scene::{KNOB, PRESS_DEPTH},
    volume_pick::PickMeshes,
};

// The tray: 400 by 120 dp, a meter ahead. Each test puts one control on it.
const TRAY: [f32; 2] = [400.0, 120.0];
// The control's place on the tray: left, top, width and height in dp
const CONTROL: [f32; 4] = [40.0, 30.0, 320.0, 60.0];

// What the content of a test reports and the states that the test sets.
#[derive(Clone, Copy)]
struct Controls {
    visible: MutableState<bool>,
    clicks: MutableState<u32>,
    checked: MutableState<bool>,
    value: MutableState<f32>,
    finished: MutableState<u32>,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Button,
    Switch,
    Slider,
}

// A headset with a tray that holds one control of `kind`, and a pointer over it.
struct Bench {
    headset: Headset,
    pass: VolumePass,
    eye: Eye,
    picks: PickMeshes,
    pointer: Pointers,
    frame: u64,
    controls: Controls,
}

fn bench(kind: Kind) -> Bench {
    let kept = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&kept);
    let headset = headset_of(Rc::new(move |_: &Probes, _| {
        let controls = remember(|| Controls {
            visible: mutableStateOf(true),
            clicks: mutableStateOf(0),
            checked: mutableStateOf(false),
            value: mutableStateOf(0.0),
            finished: mutableStateOf(0),
        })
        .with(|controls| *controls);
        keep.replace(Some(controls));
        SpatialTray(
            SubspaceModifier::new().size(TRAY[0], TRAY[1]).movable(),
            controls.visible.get(),
            Some(Color(0.1, 0.2, 0.5, 1.0)),
            move || {
                let [x, y, width, height] = CONTROL;
                let place = Modifier::empty().offset(x, y).size_points(width, height);
                match kind {
                    Kind::Button => SpatialButton(
                        move || {
                            controls.clicks.set(controls.clicks.get_non_reactive() + 1);
                        },
                        place,
                        true,
                        ButtonShape::Key,
                        Color(0.3, 0.4, 0.6, 1.0),
                        || {},
                    ),
                    Kind::Switch => SpatialSwitch(
                        controls.checked.get(),
                        move |on| controls.checked.set(on),
                        place,
                        true,
                        SwitchColors::default(),
                    ),
                    Kind::Slider => SpatialSlider(
                        controls.value.get(),
                        move |value| controls.value.set(value),
                        place,
                        true,
                        3,
                        move || {
                            controls
                                .finished
                                .set(controls.finished.get_non_reactive() + 1);
                        },
                        SliderColors::default(),
                    ),
                }
            },
        );
    }));
    let mut bench = Bench {
        eye: Eye::new(&headset.gpu),
        headset,
        pass: VolumePass::default(),
        picks: PickMeshes::default(),
        pointer: Pointers::default(),
        frame: 0,
        controls: kept.take().expect("the content composed"),
    };
    // The tray shows in 180 ms from the first frame, which also builds the scene's pipelines and
    // may take longer than the show. The last frame comes after the show's end.
    bench.redraw();
    let shown = Instant::now() + Duration::from_millis(250);
    while Instant::now() < shown {
        bench.redraw();
    }
    bench.redraw();
    bench
}

impl Bench {
    // One frame of the host: the composition's frame callbacks run, so the animations advance, and
    // the scene draws.
    fn redraw(&mut self) -> bool {
        self.headset.guest.shell().update();
        draw(&mut self.headset, &mut self.pass, &eye(0.0), &self.eye)
    }

    fn panel(&self) -> PanelSlot {
        self.headset
            .space
            .panels()
            .into_iter()
            .find(|slot| slot.kind == PanelKind::Content)
            .expect("the tray's panel")
    }

    fn volume(&self) -> PanelSlot {
        self.headset
            .space
            .panels()
            .into_iter()
            .find(|slot| matches!(slot.kind, PanelKind::Volume { .. }))
            .expect("the tray's volume")
    }

    // The ray from the head through the point `at` dp from the panel's top left corner.
    fn ray_to(&self, at: [f32; 2]) -> Ray {
        let panel = self.panel();
        let target = point_on(&panel.pose, panel.size(), at);
        let length = target.iter().map(|v| v * v).sum::<f32>().sqrt();
        Ray {
            origin: [0.0; 3],
            direction: target.map(|v| v / length),
        }
    }

    // One frame: the controller points at `at` with its trigger pulled by `trigger`, the panel
    // takes the pointer's events and the scene draws.
    fn frame(&mut self, at: [f32; 2], trigger: f32) {
        self.frame += 1;
        let ray = self.ray_to(at);
        let aim = Aim::pulling(ray, Pose::IDENTITY, trigger);
        let panels = self.headset.space.panels();
        self.pointer.update(
            [None, Some(aim)],
            Duration::from_millis(10 * self.frame),
            (&panels, &self.picks),
            (PointerMapping::default(), true),
        );
        atlas_input::deliver(self.headset.guest.shell(), self.pointer.atlas());
        self.redraw();
    }

    // Frames at `at` for `millis` milliseconds of the clock, as the animations count them.
    fn hold(&mut self, at: [f32; 2], trigger: f32, millis: u64) {
        let end = Instant::now() + Duration::from_millis(millis);
        while Instant::now() < end {
            self.frame(at, trigger);
        }
    }

    // Draws one frame of the scene with no wait for its pixels. Returns whether it drew.
    fn draw_once(&mut self) -> bool {
        self.headset.guest.shell().update();
        let camera = EyeCamera::of(&eye(0.0), (EYE, EYE), FirstRow::Top).expect("a camera");
        self.pass
            .draw(
                &mut self.headset.guest,
                &self.headset.space,
                &camera,
                &self.eye.image(),
            )
            .is_ok_and(|drew| drew)
    }

    // How far behind the panel's plane the ray through `at` meets the tray's nearest mesh, in dp.
    fn depth_at(&mut self, at: [f32; 2]) -> f32 {
        let volume = self.volume();
        let root = self.pass.root().expect("the scene has a window root");
        self.picks.refresh(
            (&mut self.headset.guest, root),
            &self.headset.space,
            &[volume],
            |_| true,
        );
        let ray = self.ray_to(at);
        let pick = self
            .picks
            .pick(volume.id, &ContentRay::of(&ray, &volume.pose), None)
            .expect("the ray meets a mesh of the tray");
        pick.position[2]
    }
}

// The middle of the control, and points a quarter of its width from each end, on the panel
const MIDDLE: [f32; 2] = [CONTROL[0] + CONTROL[2] / 2.0, CONTROL[1] + CONTROL[3] / 2.0];
const START: [f32; 2] = [CONTROL[0] + CONTROL[2] / 4.0, MIDDLE[1]];
const END: [f32; 2] = [CONTROL[0] + CONTROL[2] * 3.0 / 4.0, MIDDLE[1]];

#[test]
fn a_key_sinks_while_a_ray_presses_it_rises_at_the_release_and_clicks_once() {
    let mut bench = bench(Kind::Button);
    // The ray rests on the key: its face stands just behind the panel's plane
    bench.hold(MIDDLE, 0.0, 200);
    let resting = bench.depth_at(MIDDLE);
    assert!(
        (0.2..1.5).contains(&resting),
        "the face rests {resting} mm behind the plane"
    );
    bench.hold(MIDDLE, 1.0, 200);
    let pressed = bench.depth_at(MIDDLE);
    assert!(
        (pressed - resting - PRESS_DEPTH).abs() < 0.1,
        "the key sank {} mm",
        pressed - resting
    );
    assert_eq!(
        bench.controls.clicks.get(),
        0,
        "a press is no click before its release"
    );
    bench.hold(MIDDLE, 0.0, 200);
    let released = bench.depth_at(MIDDLE);
    assert!(
        (released - resting).abs() < 0.05,
        "the key rose to {released} mm"
    );
    assert_eq!(bench.controls.clicks.get(), 1);
}

#[test]
fn a_press_on_a_switch_flips_it_and_its_end_half_lies_deeper_while_on() {
    let mut bench = bench(Kind::Switch);
    let tilted = |bench: &mut Bench| bench.depth_at(END) - bench.depth_at(START);
    // Off: the start half lies deeper
    let off = tilted(&mut bench);
    assert!(off < -1.0, "off tilts {off} mm");
    bench.hold(MIDDLE, 1.0, 100);
    bench.hold(MIDDLE, 0.0, 300);
    assert!(bench.controls.checked.get());
    let on = tilted(&mut bench);
    assert!(on > 1.0, "on tilts {on} mm");
    // A second press flips it back
    bench.hold(MIDDLE, 1.0, 100);
    bench.hold(MIDDLE, 0.0, 300);
    assert!(!bench.controls.checked.get());
    assert!(tilted(&mut bench) < -1.0);
}

#[test]
fn a_ray_that_drags_the_knob_sets_the_nearest_value_and_the_knob_follows() {
    let mut bench = bench(Kind::Slider);
    let track = CONTROL[2] - KNOB;
    let knob_at = |value: f32| [CONTROL[0] + KNOB / 2.0 + track * value, MIDDLE[1]];
    // The knob stands at the start, in front of the groove
    let groove = bench.depth_at(knob_at(0.5));
    let knob = bench.depth_at(knob_at(0.0));
    assert!(
        knob < groove - 3.0,
        "the knob stands at {knob} mm, the groove at {groove} mm"
    );
    // The ray takes the knob, holds it, drags it to 70% of the track in 3 dp steps and lets go
    for _ in 0..40 {
        bench.frame(knob_at(0.0), 1.0);
    }
    let mut moved = 0.0;
    while moved <= track * 0.7 {
        let at = knob_at(0.0);
        bench.frame([at[0] + moved, at[1]], 1.0);
        moved += 3.0;
    }
    bench.frame(knob_at(0.7), 0.0);
    let value = bench.controls.value.get();
    assert!(
        (value - 0.75).abs() < 1e-6,
        "the drag ends at {value}, the nearest of 0, 1/4, 1/2, 3/4 and 1"
    );
    assert_eq!(bench.controls.finished.get(), 1);
    // The knob glides to the value
    bench.hold(knob_at(0.0), 0.0, 150);
    assert!(bench.depth_at(knob_at(0.75)) < groove - 3.0);
}

#[test]
fn a_tray_that_hid_composes_nothing_and_the_scene_draws_nothing() {
    let mut bench = bench(Kind::Button);
    assert_eq!(
        bench.headset.space.panels().len(),
        2,
        "a panel and the volume under it"
    );
    bench.controls.visible.set(false);
    let deadline = Instant::now() + Duration::from_secs(1);
    while !bench.headset.space.panels().is_empty() && Instant::now() < deadline {
        bench.draw_once();
    }
    assert!(
        bench.headset.space.panels().is_empty(),
        "the hidden tray holds no panel and no volume"
    );
    assert_eq!(
        crate::volumes::scene_count(&bench.headset.space),
        0,
        "the frame needs no scene"
    );
    // It shows again
    bench.controls.visible.set(true);
    assert!(bench.redraw());
    assert_eq!(bench.headset.space.panels().len(), 2);
}

#[test]
fn the_tray_stands_under_its_panel_where_the_user_carries_it() {
    let bench = bench(Kind::Button);
    let panel = bench.panel().id;
    let to = Pose::at([0.2, 0.1, -0.8]);
    for event_type in [SpatialMoveEventType::Start, SpatialMoveEventType::Moving] {
        bench.headset.space.carry(&PanelMove {
            panel,
            event_type,
            pose: to,
        });
    }
    assert_eq!(bench.panel().pose, to);
    assert_eq!(
        bench.volume().pose,
        to,
        "the volume moves with the panel in the same step"
    );
}
