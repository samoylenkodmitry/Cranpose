// A glTF model in a window, where no headset draws it: the volume shows the model in a box of the
// modifier's size, seen from the front, with its texture.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use cranpose::{
    AppLauncher,
    embedded_view::EmbeddedView,
    space::{Light, Vec3},
};
use cranpose_xr::{
    SpatialGltfModel, SpatialGltfModelSource, SpatialGltfModelState, SpatialGltfModelStatus,
    Subspace, SubspaceModifier, rememberSpatialGltfModelState,
};

const WINDOW: u32 = 400;
// The volume's box is 300 dp wide, high and deep, at the window's top left
const BOX: f32 = 300.0;
const MIDDLE: u32 = 150;

fn model(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/models")
        .join(name)
}

// A window that shows a model in a box of 300 dp. A light with no shade keeps the texture's own
// colors. The state of the model goes into `held`.
fn window(
    source: SpatialGltfModelSource,
) -> (EmbeddedView, Rc<RefCell<Option<SpatialGltfModelState>>>) {
    let held = Rc::new(RefCell::new(None));
    let keep = Rc::clone(&held);
    let view = AppLauncher::new()
        .create_embedded_view(WINDOW, WINDOW, 1.0, move || {
            let source = source.clone();
            let keep = Rc::clone(&keep);
            Subspace(move || {
                let state = rememberSpatialGltfModelState(source.clone());
                keep.replace(Some(state.clone()));
                SpatialGltfModel(
                    state,
                    SubspaceModifier::new().size(BOX, BOX).depth(BOX),
                    |scope| {
                        scope.light(Light {
                            direction: Vec3::new(0.0, 0.0, 1.0),
                            ambient: 1.0,
                            shadows: false,
                            size: 0.0,
                        });
                    },
                );
            });
        })
        .expect("GPU embedded component");
    (view, held)
}

// Draws as a host does that honors the view's frame schedule, until `ready` says that the pixels
// show what the test waits for, or the deadline passes. The model loads on a worker thread, so the
// first frames show nothing.
#[expect(
    clippy::disallowed_methods,
    reason = "a test waits for a worker thread and for the pipelines that a frame left out"
)]
fn draw_until(view: &mut EmbeddedView, pixels: &mut Vec<u8>, ready: impl Fn(&[u8]) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        view.draw(pixels).expect("frame");
        if !view.shell().needs_redraw() && ready(pixels) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the view never showed what the test waits for"
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

fn rgb_at(pixels: &[u8], x: u32, y: u32) -> [u8; 3] {
    let at = ((y * WINDOW + x) * 4) as usize;
    [pixels[at], pixels[at + 1], pixels[at + 2]]
}

// A pixel that is mostly `channel`: at least 200 of it, and under 60 of each of the others.
fn is_pure(pixel: [u8; 3], strong: [bool; 3]) -> bool {
    pixel
        .iter()
        .zip(strong)
        .all(|(&value, strong)| if strong { value > 200 } else { value < 60 })
}

#[test]
fn a_model_with_a_texture_shows_its_four_quadrants_upright_in_its_box() {
    let (mut view, _state) = window(SpatialGltfModelSource::from_path(model(
        "checker_quad.gltf",
    )));
    let mut pixels = Vec::new();
    // The quad is a meter wide and fills the box's 300 dp, in the plane through the box's middle,
    // which is farther from the camera than the front face: it shows 225 px wide, with each
    // quadrant 56 px from the middle
    let offset = 56;
    draw_until(&mut view, &mut pixels, |pixels| {
        is_pure(
            rgb_at(pixels, MIDDLE - offset, MIDDLE - offset),
            [true, false, false],
        )
    });
    assert!(
        is_pure(
            rgb_at(&pixels, MIDDLE - offset, MIDDLE - offset),
            [true, false, false]
        ),
        "top left is red, not {:?}",
        rgb_at(&pixels, MIDDLE - offset, MIDDLE - offset)
    );
    assert!(
        is_pure(
            rgb_at(&pixels, MIDDLE + offset, MIDDLE - offset),
            [false, true, false]
        ),
        "top right is green, not {:?}",
        rgb_at(&pixels, MIDDLE + offset, MIDDLE - offset)
    );
    assert!(
        is_pure(
            rgb_at(&pixels, MIDDLE - offset, MIDDLE + offset),
            [false, false, true]
        ),
        "bottom left is blue, not {:?}",
        rgb_at(&pixels, MIDDLE - offset, MIDDLE + offset)
    );
    assert!(
        is_pure(
            rgb_at(&pixels, MIDDLE + offset, MIDDLE + offset),
            [true, true, false]
        ),
        "bottom right is yellow, not {:?}",
        rgb_at(&pixels, MIDDLE + offset, MIDDLE + offset)
    );
    // Outside the quad the window's own background shows
    let corner = rgb_at(&pixels, 10, 10);
    assert!(
        corner.iter().all(|&value| value < 60),
        "the corner is {corner:?}"
    );
}

#[test]
fn a_model_that_does_not_load_shows_nothing_and_says_why() {
    let (mut view, state) = window(SpatialGltfModelSource::from_path(model(
        "missing_buffer.gltf",
    )));
    let mut pixels = Vec::new();
    let failed = |state: &Rc<RefCell<Option<SpatialGltfModelState>>>| {
        state.borrow().as_ref().map(SpatialGltfModelState::status)
    };
    draw_until(&mut view, &mut pixels, |_| {
        matches!(failed(&state), Some(SpatialGltfModelStatus::Failed(_)))
    });
    let Some(SpatialGltfModelStatus::Failed(error)) = failed(&state) else {
        panic!("the model should have failed");
    };
    assert!(error.to_string().contains("absent.bin"), "{error}");
    assert!(
        rgb_at(&pixels, MIDDLE, MIDDLE)
            .iter()
            .all(|&value| value < 60),
        "nothing shows"
    );
}
