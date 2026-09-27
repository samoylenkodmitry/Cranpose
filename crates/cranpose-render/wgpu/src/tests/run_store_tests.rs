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

fn fill_key(interiors: bool, blend: BlendMode) -> crate::render::ShapePipelineKey {
    let segment = RecordSegment {
        lane: RecordLane::Shapes,
        start: 0,
        count: 1,
        blend,
        gradient: false,
        brushes: 1,
        kinds: 1,
        band_class: 0,
        interiors,
    };
    crate::render::ShapePipelineKey {
        blend_mode: segment.blend,
        tier: crate::render::RunTier::Arena,
        variant: crate::render::ShapeVariant::of_segment(&segment, false, Default::default()),
        transformed: false,
    }
}

fn staged_draws(
    keys: impl IntoIterator<Item = crate::render::ShapePipelineKey>,
) -> Vec<(std::ops::Range<u32>, crate::render::ShapePipelineKey)> {
    let mut staging = ArenaStaging::default();
    for (record, key) in keys.into_iter().enumerate() {
        staging.push_draw(key, 0, record as u32);
    }
    staging
        .draws
        .iter()
        .map(|draw| (draw.records.clone(), draw.key))
        .collect()
}

#[test]
fn backgrounds_and_their_small_chips_share_one_draw_testing_interiors() {
    let background = fill_key(true, BlendMode::SrcOver);
    let chip = fill_key(false, BlendMode::SrcOver);
    let level = [background, chip, chip, chip, chip, chip, chip];
    let draws = staged_draws(level.iter().chain(&level).copied());
    assert_eq!(draws, [(0..14, background)]);
    let draws = staged_draws([chip, chip, background, chip]);
    assert_eq!(draws, [(0..4, background)]);
}

#[test]
fn a_run_of_shapes_without_interiors_keeps_its_own_draw() {
    let background = fill_key(true, BlendMode::SrcOver);
    let particle = fill_key(false, BlendMode::SrcOver);
    let many = JOINED_PLAIN_RECORDS as usize + 1;
    let draws =
        staged_draws(std::iter::once(background).chain(std::iter::repeat_n(particle, many + 3)));
    assert_eq!(
        draws,
        [
            (0..many as u32, background),
            (many as u32..many as u32 + 4, particle),
        ]
    );
    let draws =
        staged_draws(std::iter::repeat_n(particle, many).chain(std::iter::once(background)));
    assert_eq!(
        draws,
        [
            (0..many as u32, particle),
            (many as u32..many as u32 + 1, background),
        ]
    );
}

#[test]
fn draws_keyed_apart_by_more_than_the_interior_test_stay_apart() {
    let over = fill_key(true, BlendMode::SrcOver);
    let multiply = fill_key(false, BlendMode::Multiply);
    let draws = staged_draws([over, multiply]);
    assert_eq!(draws, [(0..1, over), (1..2, multiply)]);
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
