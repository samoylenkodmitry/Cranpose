// Content on a mesh surface that moves or changes shape from frame to
// frame, as a cloth does, takes a click where it shows now: the window's
// pointer is cast against the surface as it was last drawn.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Box, BoxSpec, Modifier, Point, Size,
    space::{Mat4, Material, Mesh, MeshSurface, Vec3},
};

const PANEL: Size = Size {
    width: 100.0,
    height: 80.0,
};
// Where the panel's top left corner first stands; the camera shows the
// plane z = 0 at its own size, so a scene point is a window point.
const PANEL_AT: Vec3 = Vec3::new(40.0, 50.0, 0.0);

type Clicks = Rc<RefCell<Vec<Point>>>;

// How the panel's place changes between frames.
#[derive(Clone, Copy)]
enum Motion {
    // The surface's transform moves the same mesh.
    Transform,
    // A new mesh, moved in its own coordinates, replaces the old one.
    Mesh,
}

struct MovingPanel {
    shell: AppShell<TestRenderer>,
    shift: MutableState<f32>,
    clicks: Clicks,
    frame: u64,
}

impl MovingPanel {
    fn new(motion: Motion) -> Self {
        let clicks: Clicks = Rc::default();
        let shift_slot: Rc<RefCell<Option<MutableState<f32>>>> = Rc::default();
        let (sink, slot) = (Rc::clone(&clicks), Rc::clone(&shift_slot));
        let mut shell = AppShell::new(
            TestRenderer::default(),
            location_key(file!(), line!(), column!()),
            move || {
                let shift = rememberMutableStateOf(|| 0.0f32);
                slot.replace(Some(shift));
                let sink = Rc::clone(&sink);
                Box(
                    Modifier::empty()
                        .fill_max_size()
                        .scene3d(|| crate::scene_facing::facing(Vec3::new(160.0, 100.0, 0.0))),
                    BoxSpec::default(),
                    move || {
                        let sink = Rc::clone(&sink);
                        Box(
                            Modifier::empty()
                                .required_size(PANEL)
                                .mesh_surface(move || panel_surface(motion, shift.get()))
                                .clickable(move |point| sink.borrow_mut().push(point)),
                            BoxSpec::default(),
                            || {},
                        );
                    },
                );
            },
        );
        shell.set_viewport(320.0, 200.0);
        shell.set_buffer_size(320, 200);
        let mut panel = Self {
            shell,
            shift: shift_slot.borrow().expect("the panel composed"),
            clicks,
            frame: 0,
        };
        panel.next_frame();
        panel
    }

    fn next_frame(&mut self) {
        self.frame += 16_000_000;
        self.shell.update_at_frame_time_nanos(self.frame);
    }

    // Clicks the window where the panel's content point `content` shows
    // with the panel `shift` to the right of where it started.
    fn click_content(&mut self, content: Point, shift: f32) {
        let x = PANEL_AT.x + shift + content.x;
        let y = PANEL_AT.y + content.y;
        let millis = i64::try_from(self.frame / 1_000_000).expect("the test clock fits in i64");
        self.shell.set_cursor_at_time(x, y, Some(millis));
        self.shell.pointer_pressed_at_time(Some(millis));
        self.shell
            .pointer_released_at_position_time(x, y, Some(millis + 30));
        self.next_frame();
    }
}

fn panel_surface(motion: Motion, shift: f32) -> MeshSurface {
    let scale = Mat4::scaling(Vec3::new(PANEL.width, PANEL.height, 1.0));
    let (mesh, transform) = match motion {
        Motion::Transform => (
            Arc::new(Mesh::plane()),
            scale.then(&Mat4::translation(PANEL_AT + Vec3::new(shift, 0.0, 0.0))),
        ),
        Motion::Mesh => {
            let step = shift / PANEL.width;
            let corners = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
            let mesh = Mesh::new(
                corners.iter().map(|[u, v]| [u + step, *v, 0.0]).collect(),
                corners.to_vec(),
                None,
                vec![0, 2, 1, 1, 2, 3],
            )
            .expect("two whole triangles");
            (Arc::new(mesh), scale.then(&Mat4::translation(PANEL_AT)))
        }
    };
    MeshSurface {
        mesh,
        transform,
        material: Material::Unlit,
        casts_shadow: false,
        solid: true,
    }
}

fn clicks_where_it_shows_now(motion: Motion) {
    let mut panel = MovingPanel::new(motion);
    let content = Point::new(30.0, 40.0);
    panel.click_content(content, 0.0);
    panel.shift.set(120.0);
    panel.next_frame();
    panel.click_content(content, 120.0);
    let clicks = panel.clicks.borrow();
    let [first, moved] = clicks.as_slice() else {
        panic!("{} clicks reached the panel, not two", clicks.len());
    };
    for at in [first, moved] {
        assert!(
            (at.x - content.x).abs() < 0.5 && (at.y - content.y).abs() < 0.5,
            "a click lands on content point {at:?}, not {content:?}"
        );
    }
}

#[test]
fn a_surface_moved_by_its_transform_takes_clicks_where_it_shows_now() {
    clicks_where_it_shows_now(Motion::Transform);
}

#[test]
fn a_surface_whose_mesh_changes_takes_clicks_where_it_shows_now() {
    clicks_where_it_shows_now(Motion::Mesh);
}
