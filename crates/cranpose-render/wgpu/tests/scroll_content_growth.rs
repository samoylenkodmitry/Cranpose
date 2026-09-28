//! A scroll container whose content grows after it was first drawn.

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Color, Modifier, ScrollState, Size, composable,
    widgets::{Box, BoxSpec, Column, ColumnSpec},
};

use crate::support;

const WIDTH: u32 = 200;
const HEIGHT: u32 = 400;
const RED: Color = Color(1.0, 0.0, 0.0, 1.0);

#[composable]
fn GrowingScroll(rows: MutableState<usize>) {
    let state = cranpose_core::remember(|| ScrollState::new(0.0)).with(|state| *state);
    Column(
        Modifier::empty()
            .fill_max_size()
            .vertical_scroll(state, false),
        ColumnSpec::default(),
        move || {
            Column(Modifier::empty(), ColumnSpec::default(), move || {
                for _ in 0..rows.get() {
                    Box(
                        Modifier::empty()
                            .size(Size {
                                width: 100.0,
                                height: 50.0,
                            })
                            .background(RED),
                        BoxSpec::default(),
                        || {},
                    );
                }
            });
        },
    );
}

fn red_at(frame: &CapturedFrame, x: u32, y: u32) -> bool {
    let index = ((y * frame.width + x) * 4) as usize;
    let pixel = &frame.pixels[index..index + 4];
    pixel[0] > 200 && pixel[1] < 60 && pixel[2] < 60
}

/// A scroll filling its window clips at the window, however tall its content
/// is: when the content grows, the new part of it is drawn, not cut off at
/// the content's old height.
#[test]
fn a_scroll_whose_content_grows_draws_all_of_it() {
    let Ok((_lock, renderer)) = support::headless_renderer_parts() else {
        eprintln!("skipping: no headless renderer");
        return;
    };
    let holder: Rc<RefCell<Option<MutableState<usize>>>> = Rc::new(RefCell::new(None));
    let captured = Rc::clone(&holder);
    let mut shell = AppShell::new(
        renderer,
        location_key(file!(), line!(), column!()),
        move || {
            let rows = cranpose_core::rememberMutableStateOf(|| 2usize);
            *captured.borrow_mut() = Some(rows);
            GrowingScroll(rows);
        },
    );
    shell.set_viewport(WIDTH as f32, HEIGHT as f32);
    shell.set_buffer_size(WIDTH, HEIGHT);
    let (_, before) = support::update_and_capture(&mut shell, WIDTH, HEIGHT);
    assert!(red_at(&before, 50, 50) && !red_at(&before, 50, 250));

    let rows = holder.borrow().as_ref().copied().expect("rows state");
    shell.debug_enter_app_context(|| rows.set(6));
    let (_, grown) = support::update_and_capture(&mut shell, WIDTH, HEIGHT);
    assert!(
        red_at(&grown, 50, 250),
        "the grown content is drawn below the container's old height"
    );
}
