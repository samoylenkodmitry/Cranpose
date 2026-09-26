use cranpose_ui_graphics::{BlendMode, ColorFilter, Point};

use super::*;

#[test]
fn shared_pipelines_preserve_each_draws_strip_index_count() {
    let segment = RecordSegment {
        lane: RecordLane::Shapes,
        start: 0,
        count: 1,
        blend: BlendMode::SrcOver,
        gradient: false,
        brushes: 1,
        kinds: 4,
        band_class: 0,
        interiors: false,
    };
    let key = crate::render::ShapePipelineKey {
        blend_mode: segment.blend,
        tier: crate::render::RunTier::Arena,
        variant: crate::render::ShapeVariant::of_segment(&segment, false, Default::default()),
        transformed: false,
    };
    let mut staging = ArenaStaging::default();
    for (record, class) in [0, 0, 3, 3, 0].into_iter().enumerate() {
        staging.push_draw(key, class, record as u32);
    }
    let draws: Vec<_> = staging
        .draws
        .iter()
        .map(|draw| (draw.records.clone(), draw.indices()))
        .collect();
    assert_eq!(draws, [(0..2, 0..6), (2..4, 0..48), (4..5, 0..6)]);
}

#[test]
fn a_placement_folds_its_snap_delta_clip_and_filter_into_the_uniform() {
    let placement = Placement {
        offset: Point::new(10.25, 20.0),
        snap_anchor: Some(crate::scene::SnapAnchor::rigid(Point::new(0.3, 0.0))),
        clip: Some(cranpose_ui_graphics::Rect {
            x: 1.0,
            y: 2.0,
            width: 3.0,
            height: 4.0,
        }),
        alpha: 0.5,
        color_filter: Some(ColorFilter::modulate(Color(0.5, 0.25, 1.0, 1.0))),
    };
    let data = PlacementData::of(&placement, 2.0);
    assert_eq!(
        data.flags,
        PLACEMENT_CANONICALIZE | PLACEMENT_CLIPPED | PLACEMENT_FILTERED | PLACEMENT_PAINTED
    );
    assert_eq!(data.root_scale, 2.0);
    assert!((data.offset[0] - 10.45).abs() < 1e-5, "{:?}", data.offset);
    assert_eq!(data.clip, [2.0, 4.0, 6.0, 8.0]);
    assert_eq!(data.alpha, 0.5);
    assert_eq!(data.color_matrix[0][0], 0.5);
    assert_eq!(data.color_matrix[1][1], 0.25);
    assert_eq!(data.color_offset, [0.0; 4]);
    let plain = PlacementData::of(&Placement::at(Point::default(), None, None), 1.0);
    assert_eq!(plain.flags, 0);
    assert_eq!(plain.color_matrix[2][2], 1.0);
    assert_eq!(std::mem::size_of::<PlacementData>(), 128);
}
