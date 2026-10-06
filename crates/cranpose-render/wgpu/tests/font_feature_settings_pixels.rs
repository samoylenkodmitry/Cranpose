use std::{cell::RefCell, rc::Rc};

use cranpose_app_shell::AppShell;
use cranpose_core::{MutableState, location_key};
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::{
    Box, BoxSpec, Color, Modifier, Text, TextStyle, composable,
    text::{SpanStyle, TextUnit},
};

use crate::support;

const FRAME_WIDTH: u32 = 240;
const FRAME_HEIGHT: u32 = 80;
const SETTINGS: [Option<&str>; 3] = [None, Some("\"zero\""), Some("\"zero\" 0")];

#[composable]
fn FontFeatureSettingsProbe(settings: MutableState<usize>) {
    let style = TextStyle::from_span_style(SpanStyle {
        color: Some(Color(1.0, 1.0, 1.0, 1.0)),
        font_size: TextUnit::Sp(40.0),
        font_feature_settings: SETTINGS[settings.get()].map(Into::into),
        ..Default::default()
    });
    Box(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.0, 0.0, 0.0, 1.0)),
        BoxSpec::default(),
        move || {
            Text(
                "0000",
                Modifier::empty()
                    .size_points(220.0, 60.0)
                    .absolute_offset(10.0, 10.0),
                style.clone(),
            );
        },
    );
}

fn bright_pixel_count(frame: &CapturedFrame) -> usize {
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 128)
        .count()
}

fn changed_pixel_count(before: &CapturedFrame, after: &CapturedFrame) -> usize {
    assert_eq!((before.width, before.height), (after.width, after.height));
    before
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(after.pixels.as_chunks::<4>().0)
        .filter(|(before, after)| before != after)
        .count()
}

#[test]
fn a_slashed_zero_feature_draws_different_pixels_from_the_plain_zero() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!(
                "skipping font feature pixel assertion because headless WGPU init failed: {err}"
            );
            return;
        }
    };

    let root_key = location_key(file!(), line!(), column!());
    let state_holder: Rc<RefCell<Option<MutableState<usize>>>> = Rc::new(RefCell::new(None));
    let state_holder_for_app = Rc::clone(&state_holder);
    let mut shell = AppShell::new(renderer, root_key, move || {
        let settings = cranpose_core::rememberMutableStateOf(|| 0usize);
        *state_holder_for_app.borrow_mut() = Some(settings);
        FontFeatureSettingsProbe(settings);
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();
    let settings = state_holder
        .borrow()
        .as_ref()
        .copied()
        .expect("settings state should be captured");

    let mut capture = |index: usize| {
        settings.set(index);
        shell.update();
        shell
            .renderer()
            .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
            .expect("frame capture should succeed")
    };
    let plain = capture(0);
    let slashed = capture(1);
    let disabled = capture(2);

    let plain_bright = bright_pixel_count(&plain);
    assert!(
        plain_bright > 200,
        "plain zeros should draw, bright pixels={plain_bright}"
    );
    let changed = changed_pixel_count(&plain, &slashed);
    assert!(
        changed > 100,
        "\"zero\" should draw the slashed zero glyph, changed pixels={changed}"
    );
    assert_eq!(
        changed_pixel_count(&plain, &disabled),
        0,
        "\"zero\" 0 should draw exactly the plain zeros"
    );
}
