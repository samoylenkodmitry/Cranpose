mod support;

use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Box, BoxSpec, Color, Modifier, Text, TextStyle, composable,
    text::{SpanStyle, TextUnit},
};

const FRAME_WIDTH: u32 = 400;
const FRAME_HEIGHT: u32 = 300;

#[composable]
#[allow(non_snake_case)]
fn AbsoluteOffsetTextRedrawProbe(start: MutableState<i32>) {
    let style = TextStyle::from_span_style(SpanStyle {
        color: Some(Color(1.0, 0.78, 0.42, 1.0)),
        font_size: TextUnit::Sp(11.0),
        ..Default::default()
    });
    let pattern = if start.get() == 0 {
        "IIIIIIIIIIII"
    } else {
        "WWWWWWWWWWWW"
    };

    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.015, 0.02, 0.03, 1.0)),
        BoxSpec::default(),
        move || {
            for row in 0..14 {
                Text(
                    format!("{pattern} row {:02}", start.get() + row),
                    Modifier::empty()
                        .size_points(380.0, 16.0)
                        .absolute_offset(8.0, row as f32 * 16.0),
                    style.clone(),
                );
            }
        },
    );
}

fn bright_pixel_count(frame: &CapturedFrame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 120 && pixel[1] > 80 && pixel[2] > 35)
        .count()
}

fn changed_pixel_count(before: &CapturedFrame, after: &CapturedFrame, threshold: u8) -> usize {
    assert_eq!(before.width, after.width);
    assert_eq!(before.height, after.height);
    before
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(after.pixels.as_chunks::<4>().0)
        .filter(|(before, after)| {
            before[0].abs_diff(after[0]) > threshold
                || before[1].abs_diff(after[1]) > threshold
                || before[2].abs_diff(after[2]) > threshold
                || before[3].abs_diff(after[3]) > threshold
        })
        .count()
}

#[test]
fn absolute_offset_text_pixels_redraw_after_state_only_change() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!(
                "skipping absolute-offset text redraw assertion because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<MutableState<i32>>>> = Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);
    let mut shell = AppShell::new(renderer, root_key, move || {
        let start = cranpose_core::rememberMutableStateOf(|| 0i32);
        *state_holder_for_app.borrow_mut() = Some(start);
        AbsoluteOffsetTextRedrawProbe(start);
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();

    let before = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("initial frame capture should succeed");
    let before_bright = bright_pixel_count(&before);
    assert!(
        before_bright > 250,
        "initial absolute-offset text should render visibly, bright pixels={before_bright}"
    );

    let start = state_holder
        .borrow()
        .as_ref()
        .cloned()
        .expect("start state should be captured");
    start.set(30);
    shell.update();

    let after = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("updated frame capture should succeed");
    let after_bright = bright_pixel_count(&after);
    assert!(
        after_bright > 250,
        "updated absolute-offset text should remain visibly rendered, bright pixels={after_bright}"
    );

    let changed_pixels = changed_pixel_count(&before, &after, 24);
    assert!(
        changed_pixels > 300,
        "state-only text recomposition should change the rasterized text without waiting for input, changed pixels={changed_pixels}"
    );
}
