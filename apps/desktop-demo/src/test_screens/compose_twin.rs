//! Desktop-demo scenes rendered alone on a fixed frame, for a pixel
//! comparison with their Compose Desktop twins in `tools/compose-twin`
//! (issue #905). A scene's name is its screenshot's on both sides.

use std::cell::RefCell;

use cranpose_core::{rememberMutableStateOf, MutableState};
use cranpose_ui::{composable, BoxSpec, Color, Modifier};

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
pub const TWIN_SCENES: [(&str, fn()); 4] = [
    ("simple-card", simple_card_showcase),
    ("positioned-boxes", positioned_boxes_showcase),
    ("item-list", item_list_showcase),
    ("complex-chain", complex_chain_showcase),
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
    if reference.len() != actual.len() || width == 0 || reference.len() % (width * 4) != 0 {
        return None;
    }
    let luma: Vec<i32> = reference
        .chunks_exact(4)
        .map(|pixel| {
            (i32::from(pixel[0]) * 54 + i32::from(pixel[1]) * 183 + i32::from(pixel[2]) * 19) >> 8
        })
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
        .chunks_exact(4)
        .zip(actual.chunks_exact(4))
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
