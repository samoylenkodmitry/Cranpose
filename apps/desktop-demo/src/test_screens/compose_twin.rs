//! Desktop-demo scenes, and chains the demos alone do not cover, rendered on
//! a fixed frame for a pixel comparison with their Compose Desktop twins in
//! `tools/compose-twin` (issue #905). A scene's name is its screenshot's on
//! both sides.

use std::cell::RefCell;

use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_foundation::text::TextFieldState;
use cranpose_ui::{
    composable, text::SpanStyle, BasicTextField, BasicTextFieldDecorated, BasicTextFieldOptions,
    BoxSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Text, TextStyle,
    VerticalAlignment,
};

use crate::app::{
    complex_chain_showcase, item_list_showcase, positioned_boxes_showcase, simple_card_showcase,
};

/// The frame's size in logical pixels, the Compose twin's `SCENE_WIDTH` and
/// `SCENE_HEIGHT`.
pub const TWIN_FRAME_WIDTH: u32 = 520;
pub const TWIN_FRAME_HEIGHT: u32 = 420;

/// The frame's color, the Compose twin's `SceneFrame` background.
const TWIN_FRAME_COLOR: Color = Color(0.07, 0.07, 0.09, 1.0);

/// Every twin scene, by name, in the Compose twin's `SCENES` order.
pub const TWIN_SCENES: [(&str, fn()); 6] = [
    ("simple-card", simple_card_showcase),
    ("positioned-boxes", positioned_boxes_showcase),
    ("item-list", item_list_showcase),
    ("complex-chain", complex_chain_showcase),
    ("modifier-order", modifier_order_probes),
    ("text-fields", text_field_probes),
];

/// A pixel strays when its largest channel differs from the Compose frame's
/// by more than this.
pub const TWIN_STRAY_DELTA: u8 = 32;

/// The stray pixels a scene may have. Glyph and corner antialiasing differ
/// between the frameworks only on edges, which leaves a scene that matches
/// with a handful; an element moved or resized by a pixel leaves over a
/// hundred beside its edges.
pub const TWIN_STRAY_LIMIT: usize = 32;

/// Neighbouring pixels of the Compose frame whose luma differs by more than
/// this form an edge.
const TWIN_EDGE_CONTRAST: i32 = 24;

thread_local! {
    /// The scene shown, set by the robot that captures them.
    pub static TWIN_SCENE_STATE: RefCell<Option<MutableState<usize>>> = const { RefCell::new(None) };
}

/// Shows the scene [`TWIN_SCENE_STATE`] names at the top left of the frame.
#[composable]
pub fn ComposeTwinScreen() {
    let scene = rememberMutableStateOf(|| 0usize);
    TWIN_SCENE_STATE.with(|cell| *cell.borrow_mut() = Some(scene));
    cranpose_ui::Box(
        Modifier::empty()
            .fill_max_size()
            .background(TWIN_FRAME_COLOR),
        BoxSpec::default(),
        move || {
            let index = scene.get();
            cranpose_core::with_key(&index, || (TWIN_SCENES[index].1)());
        },
    );
}

/// The pixels of `actual` that stray from `reference`, both RGBA frames
/// `width` pixels wide: those differing by more than [`TWIN_STRAY_DELTA`]
/// where the reference has no edge, since only edges may differ by
/// antialiasing. `None` when the frames differ in size.
pub fn twin_stray_pixels(reference: &[u8], actual: &[u8], width: usize) -> Option<usize> {
    let (reference, reference_rest) = reference.as_chunks::<4>();
    let (actual, actual_rest) = actual.as_chunks::<4>();
    if reference.len() != actual.len()
        || !reference_rest.is_empty()
        || !actual_rest.is_empty()
        || width == 0
        || !reference.len().is_multiple_of(width)
    {
        return None;
    }
    let luma: Vec<i32> = reference
        .iter()
        .map(|&[r, g, b, _]| (i32::from(r) * 54 + i32::from(g) * 183 + i32::from(b) * 19) >> 8)
        .collect();
    let height = luma.len() / width;
    let is_edge = |x: usize, y: usize| {
        let at = luma[y * width + x];
        let differs =
            |nx: usize, ny: usize| (luma[ny * width + nx] - at).abs() > TWIN_EDGE_CONTRAST;
        (x > 0 && differs(x - 1, y))
            || (x + 1 < width && differs(x + 1, y))
            || (y > 0 && differs(x, y - 1))
            || (y + 1 < height && differs(x, y + 1))
    };
    let stray = reference
        .iter()
        .zip(actual)
        .enumerate()
        .filter(|(index, (expected, got))| {
            let delta = expected[..3]
                .iter()
                .zip(&got[..3])
                .fold(0, |max, (a, b)| max.max(a.abs_diff(*b)));
            delta > TWIN_STRAY_DELTA && !is_edge(index % width, index / width)
        })
        .count();
    Some(stray)
}

/// Chains whose draws and text sit where the layout modifiers before them
/// put their content, not only inside their padding: a link widened to the
/// minimum touch target next to its label, a background before and after an
/// offset, and one after a modifier that centres the content.
#[composable]
pub fn modifier_order_probes() {
    Column(
        Modifier::empty().padding(16.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        || {
            Row(
                Modifier::empty().fill_max_width().padding(4.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                || {
                    Text(
                        "API Endpoint:",
                        Modifier::empty().padding(2.0),
                        TextStyle::default(),
                    );
                    Text(
                        "https://api.ipify.org",
                        Modifier::empty()
                            .padding(2.0)
                            .minimum_interactive_component_size(),
                        TextStyle::default(),
                    );
                },
            );
            cranpose_ui::Box(
                Modifier::empty()
                    .background(Color(0.8, 0.3, 0.3, 0.9))
                    .offset(12.0, 6.0)
                    .size_points(80.0, 40.0),
                BoxSpec::default(),
                || {},
            );
            cranpose_ui::Box(
                Modifier::empty()
                    .offset(12.0, 6.0)
                    .background(Color(0.3, 0.6, 0.9, 0.9))
                    .size_points(80.0, 40.0),
                BoxSpec::default(),
                || {},
            );
            cranpose_ui::Box(
                Modifier::empty()
                    .padding(6.0)
                    .minimum_interactive_component_size()
                    .background(Color(0.3, 0.8, 0.4, 0.9)),
                BoxSpec::default(),
                || {
                    cranpose_ui::Box(
                        Modifier::empty().size_points(16.0, 16.0),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
        },
    );
}

/// The fill behind each probe field.
const FIELD_FILL: Color = Color(0.25, 0.27, 0.33, 1.0);

/// Fields sized as Compose sizes them: an empty one and a short one a line
/// of ten 'H's, a long one its text, and a decorated one whose padding and
/// hint sit in its decoration box.
#[composable]
pub fn text_field_probes() {
    let empty = cranpose_core::remember(|| TextFieldState::new("")).with(|state| *state);
    let short = cranpose_core::remember(|| TextFieldState::new("Hi")).with(|state| *state);
    let long = cranpose_core::remember(|| TextFieldState::new("A field as wide as its text"))
        .with(|state| *state);
    let search = cranpose_core::remember(|| TextFieldState::new("")).with(|state| *state);
    Column(
        Modifier::empty().padding(16.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(12.0)),
        move || {
            BasicTextField(
                empty,
                Modifier::empty().background(FIELD_FILL),
                TextStyle::default(),
            );
            BasicTextField(
                short,
                Modifier::empty().background(FIELD_FILL).padding(4.0),
                TextStyle::default(),
            );
            BasicTextField(
                long,
                Modifier::empty().background(FIELD_FILL),
                TextStyle::default(),
            );
            BasicTextFieldDecorated(
                search,
                Modifier::empty()
                    .background(Color(0.2, 0.35, 0.6, 1.0))
                    .padding(9.0),
                BasicTextFieldOptions::default(),
                |inner| {
                    cranpose_ui::Box(Modifier::empty(), BoxSpec::default(), move || {
                        Text(
                            "Search",
                            Modifier::empty(),
                            TextStyle {
                                span_style: SpanStyle {
                                    color: Some(Color(1.0, 1.0, 1.0, 0.5)),
                                    ..Default::default()
                                },
                                ..Default::default()
                            },
                        );
                        inner.inner_text_field();
                    });
                },
            );
        },
    );
}
