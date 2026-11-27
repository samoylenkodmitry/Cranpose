// A controller's ray against the meshes of a volume as a headset runs it: the scene's last draw
// leaves each mesh where it stands, the pointer meets the nearest triangle there, and the volume's
// callback gets the hit in the volume's content space with the node whose mesh it is.

use cranpose::space::Light as SceneLight;
use cranpose_core::NodeId;

use super::*;
use crate::{
    InputAction, InputEvent, InputSource, Ray, aim::Aim, pointer_settings::PointerChoice,
    pointers::Pointers, volume_pick::PickMeshes,
};

// What a volume reports to the test: the nodes of its two quads, and the events of its callback.
#[derive(Clone, Default, PartialEq)]
struct Reports {
    nodes: Rc<RefCell<Vec<NodeId>>>,
    events: Rc<RefCell<Vec<InputEvent>>>,
}

// What moves the content of a test volume.
#[derive(Clone, Copy, PartialEq)]
struct Motion {
    // Where the near quad stands along the content's x axis, in dp
    shift: MutableState<f32>,
    // How far a group node turns the far quad about the content's y axis, in radians
    turn: MutableState<f32>,
}

// Two quads of 400 dp: the near one in the plane z = 0, and the far one 100 dp behind it inside a
// group node that turns by `turn`. The near quad moves along x by `shift`.
#[composable]
fn quads_volume(reports: Reports, plane: Arc<Mesh>, motion: Motion) {
    let events = Rc::clone(&reports.events);
    SpatialVolume(
        SubspaceModifier::new().size(QUAD, QUAD).depth(400.0),
        move |scope| {
            scope.light(SceneLight {
                direction: Vec3::new(0.0, 0.0, 1.0),
                ambient: 1.0,
                shadows: false,
                size: 0.0,
            });
            let events = Rc::clone(&events);
            scope.on_input_event(move |event| events.borrow_mut().push(event));
            let far_nodes = Rc::clone(&reports.nodes);
            let far_plane = Arc::clone(&plane);
            // The group that turns the far quad
            Box(
                Modifier::empty().transform3d(move || Mat4::rotation_y(motion.turn.get())),
                BoxSpec::default(),
                move || {
                    let node = quad(Arc::clone(&far_plane), 100.0, None);
                    far_nodes.borrow_mut().push(node);
                },
            );
            let node = quad(Arc::clone(&plane), 0.0, Some(motion.shift));
            reports.nodes.borrow_mut().insert(0, node);
        },
    );
}

// A quad of 400 dp centered on the axis, `depth` dp away from the viewer, that `shift` moves along
// x. Returns its node.
#[composable]
fn quad(plane: Arc<Mesh>, depth: f32, shift: Option<MutableState<f32>>) -> NodeId {
    Box(
        Modifier::empty()
            .required_size(Size::new(CONTENT, CONTENT))
            .mesh_surface(move || MeshSurface {
                mesh: Arc::clone(&plane),
                transform: Mat4::scaling(Vec3::new(400.0, 400.0, 1.0)).then(&Mat4::translation(
                    Vec3::new(
                        -200.0 + shift.map_or(0.0, |shift| shift.get()),
                        -200.0,
                        depth,
                    ),
                )),
                material: Material::Emissive,
                casts_shadow: false,
                solid: true,
            })
            .background(Color(0.0, 1.0, 0.0, 1.0)),
        BoxSpec::default(),
        || {},
    )
}

// The scene of a headset and a pointer over it, as the frame loop runs them.
pub(super) struct Pointing {
    pub(super) headset: Headset,
    pub(super) pass: VolumePass,
    pub(super) eye: Eye,
    picks: PickMeshes,
    pointer: Pointers,
    frame: u64,
    // Where the volume's callback writes the events that it gets
    events: Rc<RefCell<Vec<InputEvent>>>,
}

impl Pointing {
    // A pointer over `headset`, whose volume's callback writes into `events`. The scene draws once.
    pub(super) fn new(mut headset: Headset, events: Rc<RefCell<Vec<InputEvent>>>) -> Self {
        let mut pass = VolumePass::default();
        let eye = Eye::new(&headset.gpu);
        assert!(draw(&mut headset, &mut pass, &self::eye(0.0), &eye));
        Self {
            headset,
            pass,
            eye,
            picks: PickMeshes::default(),
            pointer: Pointers::default(),
            frame: 0,
            events,
        }
    }

    // Draws the scene again, as a frame does after the content changed.
    pub(super) fn redraw(&mut self) {
        assert!(draw(
            &mut self.headset,
            &mut self.pass,
            &eye(0.0),
            &self.eye
        ));
    }

    // One frame of the pointer: the picks are taken again from the scene's last draw, then a
    // controller `x` meters to the right of the volume's middle points straight ahead with its
    // trigger pulled by `trigger`. Returns the events that the volume's callback got.
    pub(super) fn point(&mut self, x: f32, trigger: f32) -> Vec<InputEvent> {
        self.point_turned(x, 0.0, trigger)
    }

    // The same, with the controller turned right by `yaw` degrees.
    pub(super) fn point_turned(&mut self, x: f32, yaw: f32, trigger: f32) -> Vec<InputEvent> {
        self.frame += 1;
        let (sin, cos) = yaw.to_radians().sin_cos();
        let panels = self.headset.space.panels();
        let root = self.pass.root().expect("the scene has a window root");
        self.picks.refresh(
            (&mut self.headset.guest, root),
            &self.headset.space,
            &panels,
            |_| true,
        );
        let ray = Ray {
            origin: [x, 0.0, 0.0],
            direction: [sin, 0.0, -cos],
        };
        let aim = Aim::pulling(ray, Pose::at([x, 0.0, 0.0]), trigger);
        self.pointer.update(
            [None, Some(aim)],
            std::time::Duration::from_millis(10 * self.frame),
            (&panels, &self.picks),
            PointerChoice::default(),
        );
        for gesture in self.pointer.surface_events() {
            self.headset.space.deliver_to_surface(gesture);
        }
        self.events.take()
    }
}

// The states that move the content, which the test sets from outside the composition.
struct Scene {
    pointing: Pointing,
    reports: Reports,
    shift: Rc<RefCell<Option<MutableState<f32>>>>,
    turn: Rc<RefCell<Option<MutableState<f32>>>>,
}

fn scene() -> Scene {
    let reports = Reports::default();
    let (shift, turn) = (Rc::new(RefCell::new(None)), Rc::new(RefCell::new(None)));
    let (keep_shift, keep_turn) = (Rc::clone(&shift), Rc::clone(&turn));
    let composer = {
        let reports = reports.clone();
        let plane = Arc::new(Mesh::plane());
        Rc::new(move |_: &Probes, _: MutableState<u32>| {
            let motion = remember(|| Motion {
                shift: mutableStateOf(0.0_f32),
                turn: mutableStateOf(0.0_f32),
            })
            .with(|motion| *motion);
            keep_shift.replace(Some(motion.shift));
            keep_turn.replace(Some(motion.turn));
            quads_volume(reports.clone(), Arc::clone(&plane), motion);
        })
    };
    let pointing = Pointing::new(headset_of(composer), Rc::clone(&reports.events));
    Scene {
        pointing,
        reports,
        shift,
        turn,
    }
}

impl Scene {
    fn point(&mut self, x: f32, trigger: f32) -> Vec<InputEvent> {
        self.pointing.point(x, trigger)
    }

    fn point_turned(&mut self, x: f32, yaw: f32, trigger: f32) -> Vec<InputEvent> {
        self.pointing.point_turned(x, yaw, trigger)
    }

    fn redraw(&mut self) {
        self.pointing.redraw();
    }

    fn set_shift(&self, shift: f32) {
        self.shift
            .borrow()
            .expect("the content composed")
            .set(shift);
    }

    fn set_turn(&self, turn: f32) {
        self.turn.borrow().expect("the content composed").set(turn);
    }
}

fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.5, "{a} is not {b}");
}

#[test]
fn a_ray_meets_the_mesh_of_a_node_and_the_volume_reports_the_node_and_the_place_in_dp() {
    let mut scene = scene();
    let events = scene.point(0.05, 0.0);
    assert_eq!(events.len(), 1);
    let event = events[0];
    assert_eq!(event.action, InputAction::HoverEnter);
    assert_eq!(event.source, InputSource::Controller);
    let hit = event.hit_info.expect("the ray meets a mesh");
    let [near_node, far_node] = scene.reports.nodes.borrow()[..] else {
        panic!("two quads composed");
    };
    // The near quad is the nearer of the two along the ray, and the ray passes through the far
    // quad too
    assert_eq!(hit.input_entity, near_node);
    assert_ne!(hit.input_entity, far_node);
    assert_eq!(hit.mesh_index, 0);
    let position = hit.hit_position.expect("the ray meets the mesh");
    near(position[0], 50.0);
    near(position[1], 0.0);
    near(position[2], 0.0);
    // The ray in the content: from the controller, 1 m before the volume's middle, along z
    near(event.origin[2], -1000.0);
    near(event.direction[2], 1.0);
}

#[test]
fn a_ray_beside_every_mesh_passes_through_the_volumes_box() {
    let mut scene = scene();
    // The box is 600 dp wide and the quads 400 dp: 25 cm right of the middle is inside the box and
    // beside both quads
    assert!(scene.point(0.25, 0.0).is_empty());
    // A ray that meets a quad, then one that moves off it, leaves the volume
    assert_eq!(scene.point(0.05, 0.0)[0].action, InputAction::HoverEnter);
    let events = scene.point(0.25, 0.0);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].action, InputAction::HoverExit);
}

#[test]
fn a_node_that_moves_is_hit_where_it_stands_after_the_scene_drew_it_there() {
    let mut scene = scene();
    // A ray 25 cm to the right meets neither quad
    assert!(scene.point(0.25, 0.0).is_empty());
    // The near quad moves 250 dp to the right: the closure of its mesh runs in the next draw
    scene.set_shift(250.0);
    scene.redraw();
    let events = scene.point(0.25, 0.0);
    assert_eq!(events.len(), 1, "{events:?}");
    let hit = events[0].hit_info.expect("the ray meets the moved quad");
    near(hit.hit_position.expect("a place")[0], 250.0);
    assert_eq!(events[0].action, InputAction::HoverEnter);
}

#[test]
fn a_mesh_under_a_turned_group_node_is_hit_through_the_nodes_transform() {
    let mut scene = scene();
    // Behind the near quad stands the far quad. A ray that misses the near quad meets the far one
    // where its group puts it. Turned by 90 degrees about y the far quad stands edge on to the ray
    // and is not met anywhere.
    scene.set_shift(500.0);
    scene.redraw();
    let events = scene.point(0.05, 0.0);
    let hit = events[0].hit_info.expect("the far quad is met");
    let [near_node, far_node] = scene.reports.nodes.borrow()[..] else {
        panic!("two quads composed");
    };
    assert_eq!(hit.input_entity, far_node);
    assert_ne!(hit.input_entity, near_node);
    near(hit.hit_position.expect("a place")[2], 100.0);
    scene.set_turn(std::f32::consts::FRAC_PI_2);
    scene.redraw();
    let events = scene.point(0.05, 0.0);
    assert_eq!(
        events.iter().map(|event| event.action).collect::<Vec<_>>(),
        [InputAction::HoverExit]
    );
}

#[test]
fn a_press_keeps_the_mesh_it_began_on_and_reports_no_place_where_the_ray_leaves_it() {
    let mut scene = scene();
    scene.point(-0.19, 0.0);
    let down = scene.point(-0.19, 1.0);
    assert_eq!(down[0].action, InputAction::Down);
    let pressed = down[0]
        .hit_info
        .expect("the press is on a mesh")
        .input_entity;
    assert_eq!(
        scene.pointing.pointer.pressed_surfaces(),
        [None, Some(scene.pointing.headset.space.panels()[0].id)]
    );
    // The controller turns by 16 degrees, more than the click angle. The aim's filter follows over
    // a few frames, and then the press drags over the quad
    let dragged = (0..80)
        .find_map(|_| scene.point_turned(-0.19, 16.0, 1.0).into_iter().next())
        .expect("the press drags");
    assert_eq!(dragged.action, InputAction::Move);
    let hit = dragged.hit_info.expect("on the quad");
    assert_eq!(hit.input_entity, pressed);
    assert!(hit.hit_position.is_some());
    // The controller turns on until the ray passes the quad's edge inside the box: the press
    // reports nothing more, and the release still names the mesh
    for _ in 0..80 {
        scene.point_turned(-0.19, 25.0, 1.0);
    }
    let up = scene.point_turned(-0.19, 25.0, 0.0);
    assert_eq!(up[0].action, InputAction::Up);
    let hit = up[0].hit_info.expect("the release names the mesh");
    assert_eq!(hit.input_entity, pressed);
    assert_eq!(hit.hit_position, None);
}
