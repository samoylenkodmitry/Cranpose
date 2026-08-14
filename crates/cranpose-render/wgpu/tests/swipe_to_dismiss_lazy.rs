mod support;

use cranpose_core::location_key;
use cranpose_foundation::lazy::{remember_lazy_list_state, LazyListScope};
use cranpose_ui::text::{SpanStyle, TextUnit};
use cranpose_ui::{
    composable, Color, LayoutBox, LazyColumn, LazyColumnSpec, Modifier, SwipeToDismiss,
    SwipeToDismissSpec, Text, TextStyle,
};

const FRAME_WIDTH: u32 = 320;
const FRAME_HEIGHT: u32 = 480;

fn bright_text_pixels(pixels: &[u8]) -> usize {
    pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| pixel[0] > 120 && pixel[1] > 90 && pixel[2] > 40)
        .count()
}

fn layout_texts(root: &LayoutBox) -> Vec<String> {
    fn walk(node: &LayoutBox, out: &mut Vec<String>) {
        if let Some(text) = node.node_data.modifier_slices().text_content() {
            out.push(text.to_string());
        }
        for child in &node.children {
            walk(child, out);
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

#[composable]
#[allow(non_snake_case)]
fn SwipeRows() {
    let style = TextStyle::from_span_style(SpanStyle {
        color: Some(Color(1.0, 0.85, 0.4, 1.0)),
        font_size: TextUnit::Sp(16.0),
        ..Default::default()
    });
    let list_state = remember_lazy_list_state();
    LazyColumn(
        Modifier::empty()
            .size_points(FRAME_WIDTH as f32, FRAME_HEIGHT as f32)
            .background(Color(0.02, 0.03, 0.05, 1.0)),
        list_state,
        LazyColumnSpec::default(),
        move |scope| {
            let style = style.clone();
            scope.items(
                5,
                None::<fn(usize) -> u64>,
                None::<fn(usize) -> u64>,
                move |index| {
                    let style = style.clone();
                    SwipeToDismiss(
                        Modifier::empty().fill_max_width().height(56.0),
                        SwipeToDismissSpec::default(),
                        || {},
                        move || {
                            Text(
                                format!("Row {index}"),
                                Modifier::empty().padding(16.0),
                                style.clone(),
                            );
                        },
                    );
                },
            );
        },
    );
}

/// Regression for issue #305: a `SwipeToDismiss` row inside a `LazyColumn` item
/// slot must both compose (layout/semantics) and rasterize its content. Before
/// the fix these rows vanished entirely inside lazy slots.
#[test]
fn swipe_to_dismiss_rows_render_inside_lazy_column() {
    let (_lock, renderer) = match support::headless_renderer_parts() {
        Ok(parts) => parts,
        Err(err) => {
            eprintln!("skipping (headless WGPU init failed): {err}");
            return;
        }
    };

    let root_key = location_key(file!(), line!(), column!());
    let mut shell = cranpose_app_shell::AppShell::new(renderer, root_key, SwipeRows);
    shell.set_viewport(FRAME_WIDTH as f32, FRAME_HEIGHT as f32);
    shell.set_buffer_size(FRAME_WIDTH, FRAME_HEIGHT);
    shell.update();

    // Composition/layout: the row content must be present in the layout tree.
    let texts = shell
        .layout_tree()
        .map(|tree| layout_texts(tree.root()))
        .unwrap_or_default();
    assert!(
        texts.iter().any(|text| text == "Row 0"),
        "SwipeToDismiss content must compose inside a LazyColumn item, got {texts:?}"
    );

    // Scene/render: the content must actually rasterize.
    let frame = shell
        .renderer()
        .capture_frame(FRAME_WIDTH, FRAME_HEIGHT)
        .expect("frame capture");
    let bright = bright_text_pixels(&frame.pixels);
    assert!(
        bright > 200,
        "SwipeToDismiss rows must rasterize glyphs inside a LazyColumn, bright pixels={bright}"
    );
}
