// A text field on a mesh surface, in a scene on a stage that a layer holds
// in view while the page scrolls, as a scrolling article pins a scene, takes
// a click where it shows without scrolling the page: it is in view already,
// though layout puts it higher.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_app_shell::AppShell;
use cranpose_core::{location_key, remember};
use cranpose_testing::robot::TestRenderer;
use cranpose_ui::{
    BasicTextField, Box, BoxSpec, Column, ColumnSpec, Modifier, ScrollState, Size, TextFieldState,
    TextStyle,
    space::{Camera, Mat4, Material, Mesh, MeshSurface, Multisample, Scene3d, Vec3},
};

const WINDOW: Size = Size {
    width: 320.0,
    height: 240.0,
};
const FIELD: Size = Size {
    width: 200.0,
    height: 40.0,
};
// Where the field shows on the stage.
const FIELD_AT: (f32, f32) = (40.0, 60.0);

fn field() {
    let state = remember(|| TextFieldState::new("Edit me")).with(|state| *state);
    let middle = Vec3::new(WINDOW.width * 0.5, WINDOW.height * 0.5, 0.0);
    let plane = Arc::new(Mesh::plane());
    Box(
        Modifier::empty()
            .required_size(WINDOW)
            .scene3d(move || Scene3d {
                camera: Camera::look_at(
                    middle + Vec3::new(0.0, 0.0, -600.0),
                    middle,
                    Vec3::new(0.0, 1.0, 0.0),
                    600.0,
                ),
                light: None,
                eye: None,
                multisample: Multisample::Off,
            }),
        BoxSpec::default(),
        move || {
            let plane = Arc::clone(&plane);
            BasicTextField(
                state,
                Modifier::empty()
                    .required_size(FIELD)
                    .mesh_surface(move || MeshSurface {
                        mesh: Arc::clone(&plane),
                        transform: Mat4::scaling(Vec3::new(FIELD.width, FIELD.height, 1.0))
                            .then(&Mat4::translation(Vec3::new(FIELD_AT.0, FIELD_AT.1, 0.0))),
                        material: Material::Unlit,
                        casts_shadow: false,
                        solid: true,
                    }),
                TextStyle::default(),
            );
        },
    );
}

// The page's scroll before and after a click on the field, with the stage
// held in view one page down.
fn scroll_after_click() -> (f32, f32) {
    let slot: Rc<RefCell<Option<ScrollState>>> = Rc::default();
    let seen = Rc::clone(&slot);
    let mut shell = AppShell::new(
        TestRenderer::default(),
        location_key(file!(), line!(), column!()),
        move || {
            let scroll = remember(|| ScrollState::new(0.0)).with(|scroll| *scroll);
            seen.replace(Some(scroll));
            Column(
                Modifier::empty()
                    .fill_max_size()
                    .vertical_scroll(scroll, false),
                ColumnSpec::default(),
                move || {
                    Box(
                        Modifier::empty()
                            .required_size(Size::new(WINDOW.width, WINDOW.height * 3.0)),
                        BoxSpec::default(),
                        move || {
                            Box(
                                Modifier::empty()
                                    .required_size(WINDOW)
                                    .graphics_layer_block(move |layer| {
                                        layer.translation_y =
                                            scroll.value().clamp(0.0, WINDOW.height * 2.0);
                                    }),
                                BoxSpec::default(),
                                field,
                            );
                        },
                    );
                },
            );
        },
    );
    shell.set_viewport(WINDOW.width, WINDOW.height);
    shell.set_buffer_size(WINDOW.width as u32, WINDOW.height as u32);
    let mut frame = 16_000_000u64;
    shell.update_at_frame_time_nanos(frame);
    let scroll = slot.borrow().expect("the page composed");
    scroll.dispatch_raw_delta(WINDOW.height);
    for _ in 0..3 {
        frame += 16_000_000;
        shell.update_at_frame_time_nanos(frame);
    }
    let before = scroll.value_non_reactive();
    let (x, y) = (FIELD_AT.0 + 20.0, FIELD_AT.1 + FIELD.height * 0.5);
    let millis = i64::try_from(frame / 1_000_000).expect("the test clock fits in i64");
    shell.set_cursor_at_time(x, y, Some(millis));
    shell.pointer_pressed_at_time(Some(millis));
    shell.pointer_released_at_position_time(x, y, Some(millis + 30));
    for _ in 0..3 {
        frame += 16_000_000;
        shell.update_at_frame_time_nanos(frame);
    }
    (before, scroll.value_non_reactive())
}

#[test]
fn a_click_on_a_field_held_in_view_leaves_the_page_where_it_is() {
    let (before, after) = scroll_after_click();
    assert!(
        (before - WINDOW.height).abs() < 0.5 && (after - before).abs() < 0.5,
        "a click on the field scrolled the page from {before} to {after}"
    );
}
