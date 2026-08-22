mod support;

use cranpose_core::location_key;
use cranpose_foundation::lazy::{rememberLazyListState, LazyListScope};
use cranpose_foundation::text::TextFieldState;
use cranpose_render_wgpu::CapturedFrame;
use cranpose_ui::text::{SpanStyle, TextUnit};
use cranpose_ui::text_field_focus::{clear_focus, dispatch_ime_preedit};
use cranpose_ui::{
    composable, Color, Column, ColumnSpec, LayoutBox, LazyColumn, LazyColumnSpec, Modifier,
    TextStyle,
};

const FRAME_WIDTH: u32 = 400;
const FRAME_HEIGHT: u32 = 500;

fn newline_preedit() -> String {
    // Explicit newlines so the field's line count (and measured height) grows.
    (0..8)
        .map(|i| format!("composed line {i}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn wrapping_transcript() -> String {
    // ~260 chars, no newlines: must wrap to many lines inside the field width.
    "the quick brown fox jumps over the lazy dog ".repeat(6)
}

fn field_style() -> TextStyle {
    TextStyle::from_span_style(SpanStyle {
        color: Some(Color(1.0, 0.85, 0.4, 1.0)),
        font_size: TextUnit::Sp(13.0),
        ..Default::default()
    })
}

fn deepest_text_field_height(root: &LayoutBox) -> Option<f32> {
    fn walk(node: &LayoutBox, found: &mut Option<f32>) {
        if node.node_data.modifier_slices().text_content().is_some() {
            *found = Some(node.rect.height);
        }
        for child in &node.children {
            walk(child, found);
        }
    }
    let mut found = None;
    walk(root, &mut found);
    found
}

/// Bright glyph pixels (the field text is a bright warm color on a dark field).
fn bright_pixels_below(frame: &CapturedFrame, min_y: u32) -> usize {
    let width = frame.width as usize;
    frame
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .enumerate()
        .filter(|(idx, pixel)| {
            let y = (*idx / width) as u32;
            y >= min_y && pixel[0] > 120 && pixel[1] > 90 && pixel[2] > 40
        })
        .count()
}

#[composable]
#[allow(non_snake_case)]
fn Field(state: TextFieldState, style: TextStyle) {
    cranpose_ui::BasicTextField(
        state,
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(Color(0.06, 0.08, 0.11, 1.0))
            .rounded_corners(10.0),
        style,
    );
}

type Shell = cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>;

/// Holds the GPU test lock alongside the shell so the serialized GPU access
/// stays live for the whole test.
struct TestApp {
    _lock: std::sync::MutexGuard<'static, ()>,
    shell: Shell,
}

fn build_shell(lazy: bool, seed: String) -> Option<TestApp> {
    let (lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return None;
        }
    };

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = cranpose_app_shell::AppShell::new(renderer, root_key, move || {
        let seed = seed.clone();
        let state = cranpose_core::remember(move || TextFieldState::new(&seed)).with(|s| *s);
        let style = field_style();
        let root_mod = Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.02, 0.03, 0.05, 1.0));
        if lazy {
            let list_state = rememberLazyListState();
            LazyColumn(root_mod, list_state, LazyColumnSpec::new(), move |scope| {
                let style = style.clone();
                scope.item(move || Field(state, style.clone()));
            });
        } else {
            Column(root_mod, ColumnSpec::default(), move || {
                Field(state, style.clone());
            });
        }
    });
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();
    Some(TestApp { _lock: lock, shell })
}

fn focus_field(shell: &mut Shell) {
    assert!(shell.set_cursor(40.0, 20.0), "click should hit the field");
    shell.update();
    assert!(shell.pointer_pressed(), "pointer down should hit the field");
    shell.update();
    let _ = shell.pointer_released();
    shell.update();
}

/// Regression: composing text into a focused multi-line field (via the IME
/// path used by both desktop winit and the Android InputConnection) must grow
/// and relayout the field on the next frame, not only after the field blurs.
fn assert_typing_grows_field(lazy: bool) {
    let Some(mut app) = build_shell(lazy, String::new()) else {
        return;
    };
    let shell = &mut app.shell;
    focus_field(shell);

    let before = shell
        .layout_tree()
        .and_then(|tree| deepest_text_field_height(tree.root()))
        .expect("field height before");

    shell.debug_enter_app_context(|| {
        assert!(
            dispatch_ime_preedit(&newline_preedit(), None),
            "preedit routed"
        );
    });
    shell.update();

    let after = shell
        .layout_tree()
        .and_then(|tree| deepest_text_field_height(tree.root()))
        .expect("field height after");
    shell.debug_enter_app_context(clear_focus);

    assert!(
        after > before + 60.0,
        "lazy={lazy}: composed multi-line text must grow the field (before={before:.1}, after={after:.1})"
    );
}

#[test]
fn typing_grows_multiline_text_field_in_plain_column() {
    assert_typing_grows_field(false);
}

#[test]
fn typing_grows_multiline_text_field_in_lazy_item() {
    assert_typing_grows_field(true);
}

/// Regression: a long single-paragraph transcript (no newlines) seeded into a
/// multi-line field must wrap to many lines and render them all, not clip
/// everything past the first line. Before the fix the field measured a single
/// line tall and the wrapped glyphs were clipped away.
#[test]
fn seeded_wrapping_transcript_renders_all_lines() {
    let Some(mut app) = build_shell(false, wrapping_transcript()) else {
        return;
    };
    let shell = &mut app.shell;

    let height = shell
        .layout_tree()
        .and_then(|tree| deepest_text_field_height(tree.root()))
        .expect("field height");
    assert!(
        height > 90.0,
        "wrapping transcript must measure several lines tall, got {height:.1}"
    );

    // Text must actually be rasterized well below the first line.
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture");
    let bright_below_first_line = bright_pixels_below(&frame, 60);
    assert!(
        bright_below_first_line > 200,
        "wrapped lines below the first must render, bright pixels below y=60 = {bright_below_first_line}"
    );
}
