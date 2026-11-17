// One composable's content drawn on two meshes at once is one live
// content: a click on either mesh reaches the content point that mesh
// shows there, and a drag over text stays on the mesh it started on, also
// where the other mesh shows under the pointer and while the meshes are
// made again between the moves.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key, rememberMutableStateOf};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Box, BoxSpec, Modifier, Point, Size, Text, TextStyle,
    selection_container::SelectionRegistrar,
    space::{Mat4, Material, Mesh, MeshSurface, MeshSurfaces, Vec3},
    text::TextUnit,
    widgets::{SelectionContainer, local_selection_registrar},
};

const WINDOW: Size = Size {
    width: 340.0,
    height: 260.0,
};
const PANEL: Size = Size {
    width: 150.0,
    height: 70.0,
};
const LINE_HEIGHT: f32 = 30.0;

// Where a mesh shows the panel's top left corner, and at what scale. The
// camera shows the plane z = 0 at its own size, so a scene point is a
// window point.
#[derive(Clone, Copy)]
struct Placed {
    at: Vec3,
    scale: f32,
}

impl Placed {
    // The window point where this mesh shows content point `content`.
    fn shows(self, content: Point) -> Point {
        Point::new(
            self.at.x + content.x * self.scale,
            self.at.y + content.y * self.scale,
        )
    }

    // A new mesh placed here, as a surface whose mesh is built again each
    // frame has.
    fn surface(self) -> MeshSurface {
        MeshSurface {
            mesh: Arc::new(Mesh::plane()),
            transform: Mat4::scaling(Vec3::new(
                PANEL.width * self.scale,
                PANEL.height * self.scale,
                1.0,
            ))
            .then(&Mat4::translation(self.at)),
            material: Material::Unlit,
            casts_shadow: false,
            solid: true,
        }
    }
}

// A shell whose scene shows `content` on the meshes `owner` and `other`,
// and the state that makes their meshes again.
fn shell_on_two_meshes(
    (owner, other): (Placed, Placed),
    content: impl Fn() + Clone + 'static,
) -> (AppShell<TestRenderer>, MutableState<u32>) {
    let slot: Rc<RefCell<Option<MutableState<u32>>>> = Rc::default();
    let seen = Rc::clone(&slot);
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let generation = rememberMutableStateOf(|| 0u32);
            seen.replace(Some(generation));
            let middle = Vec3::new(WINDOW.width * 0.5, WINDOW.height * 0.5, 0.0);
            let content = content.clone();
            Box(
                Modifier::empty()
                    .fill_max_size()
                    .scene3d(move || crate::scene_facing::facing(middle)),
                BoxSpec::default(),
                move || {
                    let content = content.clone();
                    Box(
                        Modifier::empty()
                            .required_size(PANEL)
                            .mesh_surfaces(move || {
                                // A new generation makes both meshes again,
                                // which collects the hit targets again.
                                let _: u32 = generation.get();
                                MeshSurfaces::new(owner.surface(), vec![other.surface()])
                            }),
                        BoxSpec::default(),
                        content,
                    );
                },
            );
        },
    );
    crate::test_window::size_window(&mut shell, WINDOW);
    let generation = slot.borrow().expect("the scene composed");
    (shell, generation)
}

#[test]
fn a_click_on_either_mesh_reaches_the_content_point_it_shows() {
    let owner = Placed {
        at: Vec3::new(20.0, 30.0, 0.0),
        scale: 1.0,
    };
    let other = Placed {
        at: Vec3::new(190.0, 120.0, 0.0),
        scale: 0.8,
    };
    let clicks: Rc<RefCell<Vec<Point>>> = Rc::default();
    let sink = Rc::clone(&clicks);
    let (mut shell, _) = shell_on_two_meshes((owner, other), move || {
        let sink = Rc::clone(&sink);
        Box(
            Modifier::empty()
                .fill_max_size()
                .clickable(move |point| sink.borrow_mut().push(point)),
            BoxSpec::default(),
            || {},
        );
    });
    let mut frame = 16_000_000u64;
    shell.update_at_frame_time_nanos(frame);
    let content = Point::new(110.0, 40.0);
    for mesh in [owner, other] {
        let at = mesh.shows(content);
        let millis = i64::try_from(frame / 1_000_000).expect("the test clock fits in i64");
        shell.set_cursor_at_time(at.x, at.y, Some(millis));
        shell.pointer_pressed_at_time(Some(millis));
        shell.pointer_released_at_position_time(at.x, at.y, Some(millis + 30));
        frame += 16_000_000;
        shell.update_at_frame_time_nanos(frame);
    }
    let clicks = clicks.borrow();
    assert_eq!(clicks.len(), 2, "a click on each mesh reaches the content");
    for at in clicks.iter() {
        assert!(
            (at.x - content.x).abs() < 0.5 && (at.y - content.y).abs() < 0.5,
            "a click lands on content point {at:?}, not {content:?}"
        );
    }
}

#[test]
fn a_drag_that_starts_on_one_mesh_selects_text_there() {
    // The other mesh stands higher on the right, so the row of the owner's
    // first line runs on over the other's second line.
    let owner = Placed {
        at: Vec3::new(10.0, 120.0, 0.0),
        scale: 1.0,
    };
    let other = Placed {
        at: Vec3::new(170.0, 120.0 - LINE_HEIGHT, 0.0),
        scale: 1.0,
    };
    let registrar: Rc<RefCell<Option<SelectionRegistrar>>> = Rc::default();
    let seen = Rc::clone(&registrar);
    let (mut shell, generation) = shell_on_two_meshes((owner, other), move || {
        let seen = Rc::clone(&seen);
        SelectionContainer(Modifier::empty().fill_max_size(), move || {
            *seen.borrow_mut() = local_selection_registrar().current();
            let mut style = TextStyle::default();
            style.span_style.font_size = TextUnit::Sp(20.0);
            style.paragraph_style.line_height = TextUnit::Sp(LINE_HEIGHT);
            Text("first line\nsecond line", Modifier::empty(), style);
        });
    });
    let mut frame = 16_000_000u64;
    shell.update_at_frame_time_nanos(frame);
    let row = LINE_HEIGHT * 0.5;
    let path = [
        owner.shows(Point::new(1.0, row)),
        owner.shows(Point::new(70.0, row)),
        other.shows(Point::new(80.0, LINE_HEIGHT + row)),
    ];
    assert!(
        (path[2].y - path[0].y).abs() < 0.5,
        "the drag runs along one row"
    );
    for (step, at) in path.into_iter().enumerate() {
        let millis = i64::try_from(frame / 1_000_000).expect("the test clock fits in i64");
        shell.set_cursor_at_time(at.x, at.y, Some(millis));
        if step == 0 {
            shell.pointer_pressed_at_time(Some(millis));
        }
        generation.set(generation.get_non_reactive() + 1);
        frame += 16_000_000;
        shell.update_at_frame_time_nanos(frame);
    }
    let millis = i64::try_from(frame / 1_000_000).expect("the test clock fits in i64");
    shell.pointer_released_at_position_time(path[2].x, path[2].y, Some(millis));
    shell.update_at_frame_time_nanos(frame + 16_000_000);
    let registrar = registrar.borrow();
    let selected = registrar
        .as_ref()
        .expect("the container provides a registrar")
        .selected_text();
    assert!(
        selected.starts_with("first") && !selected.contains("second"),
        "a drag along the owner's first line, on over the other mesh, selected {selected:?}"
    );
}
