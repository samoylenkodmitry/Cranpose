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
        occluders: false,
    };
    let key = crate::render::ShapePipelineKey {
        blend_mode: segment.blend,
        tier: crate::render::RunTier::Arena,
        variant: crate::render::ShapeVariant::of_segment(&segment, false, Default::default()),
        transformed: false,
        depth: crate::render::ShapeDepth::Off,
    };
    let mut staging = ArenaStaging::default();
    for (record, class) in [0, 0, 3, 3, 0].into_iter().enumerate() {
        staging.push_draw(key, class, record as u32, false);
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
        occluders: false,
    };
    crate::render::ShapePipelineKey {
        blend_mode: segment.blend,
        tier: crate::render::RunTier::Arena,
        variant: crate::render::ShapeVariant::of_segment(&segment, false, Default::default()),
        transformed: false,
        depth: crate::render::ShapeDepth::Off,
    }
}

fn staged_draws(
    keys: impl IntoIterator<Item = crate::render::ShapePipelineKey>,
) -> Vec<(std::ops::Range<u32>, crate::render::ShapePipelineKey)> {
    let mut staging = ArenaStaging::default();
    for (record, key) in keys.into_iter().enumerate() {
        staging.push_draw(key, 0, record as u32, false);
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
fn only_plain_source_over_draws_in_a_depth_pass_lay_interiors_down() {
    let tested = crate::render::ShapePipelineKey {
        depth: crate::render::ShapeDepth::Tested,
        ..fill_key(false, BlendMode::SrcOver)
    };
    let interior = tested.interior().expect("a source-over fill has interiors");
    assert_eq!(interior.depth, crate::render::ShapeDepth::Interior);
    assert!(
        interior.is_general(),
        "one interior pipeline serves every variant"
    );
    assert_eq!(interior.interior(), None);
    assert_eq!(fill_key(false, BlendMode::SrcOver).interior(), None);
    let multiply = crate::render::ShapePipelineKey {
        depth: crate::render::ShapeDepth::Tested,
        ..fill_key(false, BlendMode::Multiply)
    };
    assert_eq!(multiply.interior(), None);
    let turned = crate::render::ShapePipelineKey {
        transformed: true,
        ..tested
    };
    assert_eq!(turned.interior(), None);
}

#[test]
fn interiors_go_down_last_first_in_as_few_draws_as_their_pipeline_allows() {
    use crate::render::{ShapeDepth, ShapePipelineKey, interior_run_draws};
    let tested = |key: ShapePipelineKey| ShapePipelineKey {
        depth: ShapeDepth::Tested,
        ..key
    };
    let card = tested(fill_key(true, BlendMode::SrcOver));
    let chip = tested(fill_key(false, BlendMode::SrcOver));
    let multiply = tested(fill_key(false, BlendMode::Multiply));
    let interior = card.interior().expect("a source-over fill has interiors");
    let draws = [
        RunDrawCall::new(card, 0, 0..2, true),
        RunDrawCall::new(chip, 3, 2..5, true),
        RunDrawCall::new(multiply, 0, 5..6, true),
        RunDrawCall::new(card, 0, 6..9, true),
        RunDrawCall::new(chip, 0, 9..12, false),
    ];
    let planned: Vec<_> = interior_run_draws(&draws)
        .into_iter()
        .map(|draw| (draw.key, draw.records, draw.indices))
        .collect();
    assert_eq!(
        planned,
        [(interior, 6..9, 0..6), (interior, 0..5, 0..6)],
        "the multiply draw parts the interiors around it, and a draw of small \
         shapes lays nothing down"
    );
}

#[test]
fn a_draw_holds_occluders_once_any_record_it_takes_does() {
    let key = crate::render::ShapePipelineKey {
        depth: crate::render::ShapeDepth::Tested,
        ..fill_key(true, BlendMode::SrcOver)
    };
    let mut staging = ArenaStaging::default();
    staging.push_draw(key, 0, 0, false);
    assert_eq!(staging.draws[0].interior_key(), None);
    staging.push_draw(key, 0, 1, true);
    staging.push_draw(key, 0, 2, false);
    assert_eq!(staging.draws.len(), 1);
    assert_eq!(staging.draws[0].interior_key(), key.interior());
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
