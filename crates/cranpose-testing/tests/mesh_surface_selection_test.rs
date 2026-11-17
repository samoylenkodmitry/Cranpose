// Text in a selection container on a mesh surface, shown away from where
// it is laid out, in a scene below other content, selects the words a drag
// passes over where they show.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    Box, BoxSpec, Column, ColumnSpec, Modifier, Size, Text, TextStyle,
    selection_container::SelectionRegistrar,
    space::{Mat4, Material, Mesh, MeshSurface, Vec3},
    text::TextUnit,
    widgets::{SelectionContainer, local_selection_registrar},
};

const PANEL: Size = Size {
    width: 260.0,
    height: 90.0,
};
// Where the panel's top left corner shows in the scene; it is laid out at
// the scene's top left corner. The camera shows the plane z = 0 at its own
// size, so a scene point shows `HEADER` below the window point.
const PANEL_AT: Vec3 = Vec3::new(30.0, 70.0, 0.0);
// How far down the window the scene starts.
const HEADER: f32 = 60.0;
const WINDOW: Size = Size {
    width: 320.0,
    height: 260.0,
};
const LINE_HEIGHT: f32 = 30.0;
// How far the warped panel's top edge hangs below where a flat panel's
// would.
const WARP: f32 = 40.0;

// How the panel is moved from where it is laid out to where it shows.
#[derive(Clone, Copy, Debug)]
enum Shown {
    // By the surface's transform.
    Transform,
    // By the mesh's own vertices, as a cloth's are.
    Mesh,
    // By vertices that also squeeze the content's upper half, as a cloth's
    // folds do, so no flat plane follows the content.
    Warped,
}

impl Shown {
    // The window row where the content's row `y` shows.
    fn row(self, y: f32) -> f32 {
        let scene_y = match self {
            Self::Transform | Self::Mesh => y,
            Self::Warped => {
                let half = PANEL.height * 0.5;
                WARP + (half - WARP) * (y / half)
            }
        };
        HEADER + PANEL_AT.y + scene_y
    }
}

fn panel_surface(shown: Shown) -> MeshSurface {
    let scale = Mat4::scaling(Vec3::new(PANEL.width, PANEL.height, 1.0));
    let (mesh, transform) = match shown {
        Shown::Transform => (Mesh::plane(), scale.then(&Mat4::translation(PANEL_AT))),
        Shown::Mesh => {
            let (dx, dy) = (PANEL_AT.x / PANEL.width, PANEL_AT.y / PANEL.height);
            let corners = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
            let mesh = Mesh::new(
                corners.iter().map(|[u, v]| [u + dx, v + dy, 0.0]).collect(),
                corners.to_vec(),
                None,
                vec![0, 2, 1, 1, 2, 3],
            )
            .expect("two whole triangles");
            (mesh, scale)
        }
        Shown::Warped => {
            let top = WARP / PANEL.height;
            let rows = [(0.0, top), (0.5, 0.5), (1.0, 1.0)];
            let positions = rows
                .iter()
                .flat_map(|&(_, y)| [[0.0, y, 0.0], [1.0, y, 0.0]])
                .collect();
            let uvs = rows
                .iter()
                .flat_map(|&(v, _)| [[0.0, v], [1.0, v]])
                .collect();
            let mesh = Mesh::new(
                positions,
                uvs,
                None,
                vec![0, 2, 1, 1, 2, 3, 2, 4, 3, 3, 4, 5],
            )
            .expect("four whole triangles");
            (mesh, scale.then(&Mat4::translation(PANEL_AT)))
        }
    };
    MeshSurface {
        mesh: Arc::new(mesh),
        transform,
        material: Material::Unlit,
        casts_shadow: false,
        solid: true,
    }
}

// How the moves of a drag reach the app: a frame after each, or all after
// the press within one frame, as a fast mouse sends them.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pace {
    FramePerMove,
    OneFrame,
}

fn selected_after_drag_over_first_line(shown: Shown, pace: Pace) -> String {
    let registrar: Rc<RefCell<Option<SelectionRegistrar>>> = Rc::default();
    let seen = Rc::clone(&registrar);
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let seen = Rc::clone(&seen);
            let scene = Size::new(WINDOW.width, WINDOW.height - HEADER);
            let middle = Vec3::new(scene.width * 0.5, scene.height * 0.5, 0.0);
            let seen = Rc::clone(&seen);
            Column(
                Modifier::empty().fill_max_size(),
                ColumnSpec::default(),
                move || {
                    Box(
                        Modifier::empty().required_size(Size::new(WINDOW.width, HEADER)),
                        BoxSpec::default(),
                        || {},
                    );
                    let seen = Rc::clone(&seen);
                    Box(
                        Modifier::empty()
                            .required_size(scene)
                            .scene3d(move || crate::scene_facing::facing(middle)),
                        BoxSpec::default(),
                        move || {
                            let seen = Rc::clone(&seen);
                            SelectionContainer(
                                Modifier::empty()
                                    .required_size(PANEL)
                                    .mesh_surface(move || panel_surface(shown)),
                                move || {
                                    *seen.borrow_mut() = local_selection_registrar().current();
                                    let mut style = TextStyle::default();
                                    style.span_style.font_size = TextUnit::Sp(20.0);
                                    style.paragraph_style.line_height = TextUnit::Sp(LINE_HEIGHT);
                                    Text("first line\nsecond line", Modifier::empty(), style);
                                },
                            );
                        },
                    );
                },
            );
        },
    );
    let mut frame = crate::test_window::first_frame(&mut shell, WINDOW);
    let row = shown.row(LINE_HEIGHT * 0.5);
    let path: Vec<f32> = match pace {
        Pace::FramePerMove => vec![1.0, 80.0, 200.0],
        Pace::OneFrame => (0..=10).map(|step| 1.0 + 19.9 * step as f32).collect(),
    };
    for (step, x) in path.iter().enumerate() {
        let millis = i64::try_from(frame / 1_000_000).expect("the test clock fits in i64");
        shell.set_cursor_at_time(PANEL_AT.x + x, row, Some(millis));
        if step == 0 {
            shell.pointer_pressed_at_time(Some(millis));
        }
        if pace == Pace::FramePerMove || step == 0 {
            frame += 16_000_000;
            shell.update_at_frame_time_nanos(frame);
        }
    }
    let millis = i64::try_from(frame / 1_000_000).expect("the test clock fits in i64");
    shell.pointer_released_at_position_time(PANEL_AT.x + 200.0, row, Some(millis));
    shell.update_at_frame_time_nanos(frame + 16_000_000);
    let registrar = registrar.borrow();
    registrar
        .as_ref()
        .expect("the container provides a registrar")
        .selected_text()
}

#[test]
fn a_drag_over_text_on_a_moved_surface_selects_the_words_where_they_show() {
    for shown in [Shown::Transform, Shown::Mesh, Shown::Warped] {
        for pace in [Pace::FramePerMove, Pace::OneFrame] {
            let selected = selected_after_drag_over_first_line(shown, pace);
            assert!(
                selected.starts_with("first") && !selected.contains("second"),
                "a {pace:?} drag along the first line of the {shown:?} panel selected {selected:?}"
            );
        }
    }
}
