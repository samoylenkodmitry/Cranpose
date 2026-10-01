use cranpose_ui_graphics::{Point, Rect};

use super::*;

fn placement(clip: Option<Rect>) -> crate::scene::Placement {
    crate::scene::Placement {
        offset: Point::new(0.0, 0.0),
        snap_anchor: None,
        clip,
        clip_radius: 0.0,
        alpha: 1.0,
        color_filter: None,
    }
}

fn viewport(offset: [f32; 2], transform: SegmentTransform) -> ViewportUniformParams {
    ViewportUniformParams {
        width: 64,
        height: 48,
        offset,
        transform,
        origin: [0.0, 0.0],
        depth_base: 0.0,
    }
}

/// The pixels along one axis the clipped shape variant keeps: those whose
/// centre, moved by `offset`, lies within the clip, as the shader compares.
fn shader_kept(low: f32, width: f32, offset: f32, extent: u32) -> Vec<u32> {
    let high = low + width;
    (0..extent)
        .filter(|&pixel| {
            let centre = pixel as f32 + 0.5 + offset;
            centre >= low && centre <= high
        })
        .collect()
}

#[test]
fn an_unturned_clip_keeps_exactly_the_pixels_the_clip_test_would() {
    let edges = [-3.0, 0.0, 0.25, 0.5, 1.0, 2.5, 7.49, 7.5, 7.51, 30.0, 70.0];
    let widths = [0.0, 0.2, 1.0, 5.5, 12.25, 40.0, 100.0];
    let offsets = [0.0, 0.5, -2.0, 3.25, 17.0];
    for x in edges {
        for width in widths {
            for offset in offsets {
                let clip = Rect {
                    x,
                    y: x,
                    width,
                    height: width,
                };
                let Some((left, top, columns, rows)) = clip_scissor(
                    &placement(Some(clip)),
                    1.0,
                    viewport([offset, offset], SegmentTransform::IDENTITY),
                ) else {
                    panic!("an unturned clip takes a scissor");
                };
                let kept_columns: Vec<u32> = (left..left + columns).collect();
                let kept_rows: Vec<u32> = (top..top + rows).collect();
                assert_eq!(
                    kept_columns,
                    shader_kept(x, width, offset, 64),
                    "columns of clip x={x} width={width} under offset {offset}"
                );
                assert_eq!(
                    kept_rows,
                    shader_kept(x, width, offset, 48),
                    "rows of clip y={x} height={width} under offset {offset}"
                );
            }
        }
    }
}

#[test]
fn a_clip_takes_the_root_scale_before_its_scissor() {
    let clip = Rect {
        x: 2.25,
        y: 1.0,
        width: 4.0,
        height: 3.0,
    };
    assert_eq!(
        clip_scissor(
            &placement(Some(clip)),
            2.0,
            viewport([0.0, 0.0], SegmentTransform::IDENTITY)
        ),
        Some((4, 2, 9, 6)),
        "device clip 4.5..12.5 × 2..8 keeps centres 4.5 through 12.5 and 2.5 through 7.5"
    );
}

#[test]
fn a_turned_or_unclipped_run_takes_no_scissor() {
    let clip = Rect {
        x: 0.0,
        y: 0.0,
        width: 10.0,
        height: 10.0,
    };
    let Some(turn) = SegmentTransform::affine([0.0, -1.0, 1.0, 0.0], [0.0, 0.0]) else {
        panic!("a quarter turn is invertible");
    };
    assert_eq!(
        clip_scissor(&placement(Some(clip)), 1.0, viewport([0.0, 0.0], turn)),
        None,
        "a turned run tests its clip in the shader"
    );
    assert_eq!(
        clip_scissor(
            &placement(None),
            1.0,
            viewport([0.0, 0.0], SegmentTransform::IDENTITY)
        ),
        None
    );
}

#[test]
fn target_rects_intersect_to_the_pixels_both_keep() {
    assert_eq!(
        intersect_target_rects((0, 0, 10, 10), (5, 2, 10, 4)),
        Some((5, 2, 5, 4))
    );
    assert_eq!(intersect_target_rects((0, 0, 4, 4), (4, 0, 4, 4)), None);
}
