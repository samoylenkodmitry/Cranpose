// A tray as a headset runs it: the scene draws its meshes on the GPU of the machine, a controller's
// ray presses its controls on the tray's panel through the frame loop's pointer, and the ray's pick
// measures where the meshes stand. A key sinks while pressed, a switch flips, a slider takes the
// value that a drag gives, and a tray that hid composes nothing and draws nothing.

use std::time::Duration;

use super::*;
use crate::{
    ButtonShape, Ray, SliderColors, SpatialButton, SpatialMoveEventType, SpatialSlider,
    SpatialSwitch, SpatialTray, SwitchColors,
    aim::Aim,
    atlas_input,
    content_ray::ContentRay,
    eye_camera::{EyeCamera, FirstRow},
    pointer_settings::PointerChoice,
    pointers::Pointers,
    space::{PanelKind, PanelMove, PanelSlot, field_dock::point_on},
    tray_scene::{KNOB, PRESS_DEPTH},
    volume_pick::PickMeshes,
};

// The tray: 400 by 120 dp, a meter ahead. Each test puts one control on it.
const TRAY: [f32; 2] = [400.0, 120.0];
// The control's place on the tray: left, top, width and height in dp
const CONTROL: [f32; 4] = [40.0, 30.0, 320.0, 60.0];

#[path = "spatial_hologram_tests.rs"]
mod hologram;

// What the content of a test reports and the states that the test sets.
#[derive(Clone, Copy)]
struct Controls {
    visible: MutableState<bool>,
    clicks: MutableState<u32>,
    checked: MutableState<bool>,
    value: MutableState<f32>,
    finished: MutableState<u32>,
    selected: MutableState<Option<usize>>,
}

// What holds a bench's controls: a tray, or a hologram with a stage, on a headset that asks for no
// frames of its own.
#[derive(Clone, Copy, PartialEq)]
enum Holder {
    Tray,
    Hologram(crate::HologramStage),
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
    bench_of(TRAY, move |controls| {
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
    })
}

// A headset with a tray of `size` dp, a meter ahead, whose content is `content`, and a pointer
// over it.
fn bench_of(size: [f32; 2], content: impl Fn(Controls) + Clone + 'static) -> Bench {
    bench_in(Holder::Tray, size, content)
}

// A headset with the controls that `content` composes in `holder`, of `size` dp a meter ahead, and
// a pointer over it.
fn bench_in(holder: Holder, size: [f32; 2], content: impl Fn(Controls) + Clone + 'static) -> Bench {
    let kept = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&kept);
    let ticks = match holder {
        Holder::Tray => Ticks::EachFrame,
        Holder::Hologram(_) => Ticks::None,
    };
    let headset = headset_with(
        Rc::new(move |_: &Probes, _| {
            let controls = remember(|| Controls {
                visible: mutableStateOf(true),
                clicks: mutableStateOf(0),
                checked: mutableStateOf(false),
                value: mutableStateOf(0.0),
                finished: mutableStateOf(0),
                selected: mutableStateOf(None),
            })
            .with(|controls| *controls);
            keep.replace(Some(controls));
            let content = content.clone();
            let modifier = SubspaceModifier::new().size(size[0], size[1]).movable();
            match holder {
                Holder::Tray => SpatialTray(
                    modifier,
                    controls.visible.get(),
                    Some(Color(0.1, 0.2, 0.5, 1.0)),
                    move || content(controls),
                ),
                Holder::Hologram(stage) => crate::SpatialHologram(
                    modifier,
                    controls.visible.get(),
                    crate::HologramColors::CYAN,
                    stage,
                    move || content(controls),
                ),
            }
        }),
        ticks,
    );
    let mut bench = Bench {
        eye: Eye::new(&headset.gpu),
        headset,
        pass: VolumePass::default(),
        picks: PickMeshes::default(),
        pointer: Pointers::default(),
        frame: 0,
        controls: kept.take().expect("the content composed"),
    };
    // The tray shows in 180 ms and a hologram in 420 ms of the clock. The last frame comes after
    // the show's end.
    for _ in 0..frames_in(500) + 1 {
        bench.redraw();
    }
    bench
}

// The time between two frames on a bench's own clock, which the animations and the pointer read.
// A loaded machine can take a hundred milliseconds for one frame. The animations still advance by
// this time for each frame, so a hold of a test lasts as many frames on every machine.
const FRAME_MILLIS: u64 = 10;
const FRAME: Duration = Duration::from_millis(FRAME_MILLIS);

// The frames of a bench in `millis` milliseconds of its clock.
fn frames_in(millis: u64) -> u64 {
    millis / FRAME_MILLIS
}

// The time of the bench's clock at its frame `frame`.
fn time_of(frame: u64) -> Duration {
    Duration::from_millis(frame * FRAME_MILLIS)
}

impl Bench {
    // One frame of the host, `FRAME` after the last on the bench's clock: the composition's frame
    // callbacks run, so the animations advance, and the scene draws.
    fn redraw(&mut self) -> bool {
        self.headset
            .guest
            .shell()
            .update_after_exact_interval(FRAME);
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
            time_of(self.frame),
            (&panels, &self.picks),
            PointerChoice::default(),
        );
        atlas_input::deliver(self.headset.guest.shell(), self.pointer.atlas());
        self.redraw();
    }

    // Frames of the hand that `script` plays, a frame apart on the script's clock, from `from` to
    // `until` milliseconds after its start: the pointer takes the hand's aim as the frame loop does,
    // the panel takes the pointer's events and the scene draws.
    fn play(&mut self, script: &crate::scripted_hand::ScriptedHand, (from, until): (u64, u64)) {
        for step in frames_in(from)..frames_in(until) {
            self.frame += 1;
            let aims = crate::pointers::direct_touch_tests::aims_at(&[script], time_of(step));
            let panels = self.headset.space.panels();
            self.pointer.update(
                aims,
                time_of(self.frame),
                (&panels, &self.picks),
                PointerChoice::default(),
            );
            atlas_input::deliver(self.headset.guest.shell(), self.pointer.atlas());
            self.redraw();
        }
    }

    // Frames at `at` for `millis` milliseconds of the clock, as the animations count them.
    fn hold(&mut self, at: [f32; 2], trigger: f32, millis: u64) {
        for _ in 0..frames_in(millis) {
            self.frame(at, trigger);
        }
    }

    // Frames with no wait for their pixels until the content's panels have gone, for a second of the
    // clock at most: a hide animates before the panels go.
    fn draw_until_hidden(&mut self) {
        let camera = EyeCamera::of(&eye(0.0), (EYE, EYE), FirstRow::Top).expect("a camera");
        for _ in 0..frames_in(1000) {
            if self.headset.space.panels().is_empty() {
                return;
            }
            self.headset
                .guest
                .shell()
                .update_after_exact_interval(FRAME);
            self.pass
                .draw(
                    &mut self.headset.guest,
                    &self.headset.space,
                    &camera,
                    &self.eye.image(),
                )
                .expect("a frame");
        }
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
    bench.draw_until_hidden();
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

// What a word of the layout test is: a key's words or a row's label beside its keys.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Words {
    OnKey,
    Label,
}

// The words of a tray laid out as a menu: rows of a label with a switch or keys beside it, and a
// row of keys with no label, each word with the place where it lays out in the window.
type Placed = Rc<RefCell<Vec<(Words, &'static str, MutableState<Rect>)>>>;

// A word that notes where it lays out. The harness measures no glyphs, so a box of the size that
// the words take on a menu stands in for them: a key's words 60 by 24 dp, a label's line 34 dp tall.
#[composable]
fn Word(placed: Placed, (kind, text): (Words, &'static str), modifier: Modifier) {
    let rect = remember(|| mutableStateOf(Rect::default())).with(|rect| *rect);
    {
        let mut placed = placed.borrow_mut();
        if !placed.iter().any(|(_, known, _)| *known == text) {
            placed.push((kind, text, rect));
        }
    }
    let size = match kind {
        Words::OnKey => Modifier::empty().size_points(60.0, 24.0),
        Words::Label => Modifier::empty().fill_max_width().height(34.0),
    };
    Box(
        modifier.then(size).report_window_rect_state(rect),
        BoxSpec::default(),
        || {},
    );
}

// A key with its words on it.
#[composable]
fn MenuKey(placed: Placed, text: &'static str, modifier: Modifier) {
    SpatialButton(
        || {},
        modifier.height(52.0),
        true,
        ButtonShape::Key,
        Color(0.3, 0.4, 0.6, 1.0),
        move || Word(Rc::clone(&placed), (Words::OnKey, text), Modifier::empty()),
    );
}

// A label that fills the room that its row's controls leave, 10 dp below the row's top.
fn label() -> Modifier {
    Modifier::empty().weight(1.0).padding_symmetric(0.0, 10.0)
}

#[composable]
fn Menu(placed: Placed, root: MutableState<Rect>) {
    Column(
        Modifier::empty()
            .fill_max_size()
            .report_window_rect_state(root)
            .padding(22.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            let row = || Modifier::empty().fill_max_width().height(52.0);
            let spaced =
                || RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(8.0));
            let first = Rc::clone(&placed);
            Row(row(), spaced(), move || {
                Word(Rc::clone(&first), (Words::Label, "Stream numbers"), label());
                SpatialSwitch(
                    false,
                    |_| {},
                    Modifier::empty().width(92.0).height(52.0),
                    true,
                    SwitchColors::default(),
                );
            });
            let keys = Rc::clone(&placed);
            Row(row(), spaced(), move || {
                for text in ["Steam", "Stream", "Connection"] {
                    MenuKey(Rc::clone(&keys), text, Modifier::empty().weight(1.0));
                }
            });
            let third = Rc::clone(&placed);
            Row(row(), spaced(), move || {
                Word(Rc::clone(&third), (Words::Label, "Interface"), label());
                for text in ["3D", "Flat"] {
                    MenuKey(Rc::clone(&third), text, Modifier::empty().width(92.0));
                }
            });
            let last = Rc::clone(&placed);
            Row(row(), spaced(), move || {
                for text in ["Back", "Close"] {
                    MenuKey(Rc::clone(&last), text, Modifier::empty().weight(1.0));
                }
            });
        },
    );
}

// Each word of a tray stands on its own control's body or on the base, from the same layout: a
// ray through the middle of a key's words meets that key's face, and a ray through any corner of a
// row's label meets the base, never a key of a row above or below it.
#[test]
fn words_lie_on_their_own_keys_and_labels_on_the_base_beside_their_row() {
    let placed: Placed = Rc::new(RefCell::new(Vec::new()));
    let root = Rc::new(RefCell::new(None::<MutableState<Rect>>));
    let (keep_placed, keep_root) = (Rc::clone(&placed), Rc::clone(&root));
    let mut bench = bench_of([470.0, 300.0], move |_| {
        let rect = remember(|| mutableStateOf(Rect::default())).with(|rect| *rect);
        keep_root.replace(Some(rect));
        Menu(Rc::clone(&keep_placed), rect);
    });
    let root = root.borrow().expect("the menu composed").get();
    let words = placed.borrow().clone();
    assert_eq!(words.len(), 9, "two labels and seven keys' words");
    for (kind, text, rect) in words {
        let rect = rect.get();
        assert!(rect.width > 0.0, "{text} laid out");
        // The word's place on the panel, in dp from its top left corner
        let on_panel = |x: f32, y: f32| [rect.x - root.x + x, rect.y - root.y + y];
        let middle = on_panel(rect.width / 2.0, rect.height / 2.0);
        match kind {
            Words::OnKey => {
                let depth = bench.depth_at(middle);
                assert!(
                    (0.2..2.0).contains(&depth),
                    "the words {text} stand {depth} mm in front of what lies under them, not on a key"
                );
            }
            Words::Label => {
                let inset = 1.0;
                for at in [
                    middle,
                    on_panel(inset, inset),
                    on_panel(rect.width - inset, inset),
                    on_panel(inset, rect.height - inset),
                    on_panel(rect.width - inset, rect.height - inset),
                ] {
                    let depth = bench.depth_at(at);
                    assert!(
                        depth > 9.0,
                        "the label {text} lies over a part {depth} mm behind it at {at:?}"
                    );
                }
            }
        }
    }
}
