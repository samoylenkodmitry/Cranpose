#![cfg(feature = "gltf")]

// A glTF model in a window, where no headset draws it: the volume shows the model in a box of the
// modifier's size, seen from the front, with its texture.

use std::{cell::RefCell, path::PathBuf, rc::Rc};

use base64::Engine as _;
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

#[test]
fn a_masked_texture_shows_where_it_is_opaque_and_the_window_where_it_is_clear() {
    let (mut view, _state) = window(SpatialGltfModelSource::from_path(model("masked_quad.gltf")));
    let mut pixels = Vec::new();
    draw_until(&mut view, &mut pixels, |pixels| {
        is_pure(rgb_at(pixels, MIDDLE + 56, MIDDLE), [false, true, false])
    });
    // The quad's right half is green, and its left half is clear, so the window's own background
    // shows there although the quad covers it
    let clear = rgb_at(&pixels, MIDDLE - 56, MIDDLE);
    assert!(
        clear.iter().all(|&value| value < 60),
        "the clear half shows {clear:?}"
    );
    assert!(is_pure(
        rgb_at(&pixels, MIDDLE + 56, MIDDLE),
        [false, true, false]
    ));
}

fn uastc_solid_block(color: [u8; 4]) -> [u8; 16] {
    let mut bits: u128 = 0x17;
    for (index, &channel) in color.iter().enumerate() {
        bits |= u128::from(channel) << (5 + 8 * index);
    }
    bits.to_le_bytes()
}

fn zstd_raw_frame(payload: &[u8]) -> Vec<u8> {
    let size = u32::try_from(payload.len()).expect("zstd raw block size fits in u32");
    assert!(size < (1 << 17), "zstd raw block exceeds 128 KiB limit");
    let block_header = (size << 3) | 1;
    let mut frame = Vec::with_capacity(12 + payload.len());
    frame.extend_from_slice(&[0x28, 0xB5, 0x2F, 0xFD]);
    // Single-segment frame with a 4-byte Frame_Content_Size field and no checksum.
    frame.push(0xA0);
    frame.extend_from_slice(&size.to_le_bytes());
    frame.extend_from_slice(&block_header.to_le_bytes()[..3]);
    frame.extend_from_slice(payload);
    frame
}

fn ktx2_zstd_uastc(width: u32, height: u32, levels: &[(&[u8], u64)]) -> Vec<u8> {
    let mut descriptor = Vec::with_capacity(44);
    for word in [44_u32, 0] {
        descriptor.extend_from_slice(&word.to_le_bytes());
    }
    for half in [2_u16, 40] {
        descriptor.extend_from_slice(&half.to_le_bytes());
    }
    descriptor.extend_from_slice(&[166, 1, 2, 0, 3, 3, 0, 0, 16, 0, 0, 0, 0, 0, 0, 0]);
    descriptor.extend_from_slice(&0_u16.to_le_bytes());
    descriptor.extend_from_slice(&[127, 0, 0, 0, 0, 0]);
    descriptor.extend_from_slice(&0_u32.to_le_bytes());
    descriptor.extend_from_slice(&u32::MAX.to_le_bytes());

    let dfd_offset = 80 + 24 * u32::try_from(levels.len()).expect("level count fits in u32");
    let mut cursor = ((u64::from(dfd_offset) + descriptor.len() as u64) + 15) & !15;
    let mut offsets = Vec::with_capacity(levels.len());
    for &(compressed, _) in levels {
        offsets.push(cursor);
        cursor = (cursor + compressed.len() as u64 + 15) & !15;
    }

    let mut out = Vec::with_capacity(usize::try_from(cursor).expect("ktx2 size fits in usize"));
    out.extend_from_slice(b"\xABKTX 20\xBB\r\n\x1A\n");
    for field in [
        0_u32,
        1,
        width,
        height,
        0,
        0,
        1,
        u32::try_from(levels.len()).expect("level count fits in u32"),
        2,
    ] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    for field in [dfd_offset, 44, 0, 0] {
        out.extend_from_slice(&field.to_le_bytes());
    }
    out.extend_from_slice(&0_u64.to_le_bytes());
    out.extend_from_slice(&0_u64.to_le_bytes());
    for (&(compressed, uncompressed), &offset) in levels.iter().zip(&offsets) {
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
        out.extend_from_slice(&uncompressed.to_le_bytes());
    }
    out.extend_from_slice(&descriptor);
    for (&(compressed, _), &offset) in levels.iter().zip(&offsets) {
        let target = usize::try_from(offset).expect("offset fits in usize");
        out.resize(target, 0);
        out.extend_from_slice(compressed);
    }
    out
}

fn basisu_gltf_with_ktx2(ktx2: &[u8]) -> SpatialGltfModelSource {
    let template = include_str!("models/basisu_quad.gltf");
    let original_uri = template
        .split('"')
        .find(|part| part.starts_with("data:image/ktx2;base64,"))
        .expect("basisu_quad.gltf carries a KTX2 data URI");
    let encoded = base64::engine::general_purpose::STANDARD.encode(ktx2);
    let replacement = format!("data:image/ktx2;base64,{encoded}");
    SpatialGltfModelSource::from_bytes(template.replace(original_uri, &replacement).into_bytes())
}

#[test]
fn a_zstd_ktx2_model_selects_the_first_mip_that_fits_the_texture_limit() {
    // Mip 0 is 4096x4 (exceeds the 2048 px texture limit) and red; mip 1 is 2048x2 and green.
    let level0_raw: Vec<u8> = uastc_solid_block([255, 0, 0, 255])
        .into_iter()
        .cycle()
        .take(1024 * 16)
        .collect();
    let level1_raw: Vec<u8> = uastc_solid_block([0, 255, 0, 255])
        .into_iter()
        .cycle()
        .take(512 * 16)
        .collect();
    let level0_zstd = zstd_raw_frame(&level0_raw);
    let level1_zstd = zstd_raw_frame(&level1_raw);
    let ktx2 = ktx2_zstd_uastc(
        4096,
        4,
        &[
            (&level0_zstd, level0_raw.len() as u64),
            (&level1_zstd, level1_raw.len() as u64),
        ],
    );

    let (mut view, _state) = window(basisu_gltf_with_ktx2(&ktx2));
    let mut pixels = Vec::new();
    draw_until(&mut view, &mut pixels, |pixels| {
        is_pure(rgb_at(pixels, MIDDLE, MIDDLE), [false, true, false])
    });
    assert!(
        is_pure(rgb_at(&pixels, MIDDLE, MIDDLE), [false, true, false]),
        "nonzero mip 1 renders green, not {:?}",
        rgb_at(&pixels, MIDDLE, MIDDLE)
    );
}

#[test]
fn a_zstd_ktx2_model_whose_stream_exceeds_its_declared_size_fails() {
    // The KTX2 level declares 16 uncompressed bytes (one 4x4 UASTC block),
    // while the Zstandard frame decompresses to 32 bytes.
    let oversized_zstd = zstd_raw_frame(&[0x55; 32]);
    let ktx2 = ktx2_zstd_uastc(4, 4, &[(&oversized_zstd, 16)]);

    let (mut view, state) = window(basisu_gltf_with_ktx2(&ktx2));
    let mut pixels = Vec::new();
    let status = |state: &Rc<RefCell<Option<SpatialGltfModelState>>>| {
        state.borrow().as_ref().map(SpatialGltfModelState::status)
    };
    draw_until(&mut view, &mut pixels, |_| {
        matches!(status(&state), Some(SpatialGltfModelStatus::Failed(_)))
    });
    assert!(matches!(
        status(&state),
        Some(SpatialGltfModelStatus::Failed(_))
    ));
    assert!(
        rgb_at(&pixels, MIDDLE, MIDDLE)
            .iter()
            .all(|&value| value < 60),
        "nothing shows"
    );
}
