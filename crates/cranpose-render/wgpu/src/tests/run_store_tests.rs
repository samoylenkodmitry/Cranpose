use cranpose_ui_graphics::{BlendMode, ColorFilter, Point};

use super::*;

/// A shape segment of one record: its kinds' bits, blend and interior flags.
fn shape_segment(
    kinds: u8,
    blend: BlendMode,
    (interiors, occluders, bare_interiors): (bool, bool, bool),
) -> RecordSegment {
    RecordSegment {
        lane: RecordLane::Shapes,
        start: 0,
        count: 1,
        blend,
        gradient: false,
        vertex_gradient: false,
        brushes: 1,
        kinds,
        band_class: 0,
        interiors,
        occluders,
        bare_interiors,
    }
}

/// The arena pipeline `segment` draws with in a pass without depth.
fn arena_key(segment: &RecordSegment) -> crate::render::ShapePipelineKey {
    crate::render::ShapePipelineKey {
        blend_mode: segment.blend,
        tier: crate::render::RunTier::Arena,
        variant: crate::render::ShapeVariant::of_segment(
            segment,
            false,
            Default::default(),
            false,
            true,
        ),
        turns: crate::render::ShapeTurns::None,
        depth: crate::render::ShapeDepth::Off,
    }
}

#[test]
fn shared_pipelines_preserve_each_draws_strip_index_count() {
    let key = arena_key(&shape_segment(4, BlendMode::SrcOver, (false, false, false)));
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

/// The arena key of a one-record gradient fill segment: only a gradient
/// batch tests fill interiors.
fn fill_key(interiors: bool, blend: BlendMode) -> crate::render::ShapePipelineKey {
    arena_key(&RecordSegment {
        gradient: true,
        ..shape_segment(1, blend, (interiors, false, false))
    })
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

fn solid_fill_key(vertex_gradient: bool) -> crate::render::ShapePipelineKey {
    arena_key(&RecordSegment {
        vertex_gradient,
        ..shape_segment(1, BlendMode::SrcOver, (false, false, false))
    })
}

#[test]
fn a_vertex_gradient_and_its_small_solid_shapes_share_one_dithered_draw() {
    let chart = solid_fill_key(true);
    let bar = solid_fill_key(false);
    assert_eq!(staged_draws([bar, chart, bar, bar, bar]), [(0..5, chart)]);
    let many = JOINED_PLAIN_RECORDS + 1;
    let draws = staged_draws(std::iter::once(chart).chain(std::iter::repeat_n(bar, many as usize)));
    assert_eq!(draws, [(0..many, chart), (many..many + 1, bar)]);
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
fn a_card_s_plain_draws_ride_along_with_the_interiors_around_them() {
    use crate::render::{ShapeDepth, ShapePipelineKey, interior_run_draws};
    let tested = |key: ShapePipelineKey| ShapePipelineKey {
        depth: ShapeDepth::Tested,
        ..key
    };
    let card = tested(fill_key(true, BlendMode::SrcOver));
    let bars = tested(fill_key(false, BlendMode::SrcOver));
    let interior = card.interior().expect("a source-over fill has interiors");
    let draws = [
        RunDrawCall::new(bars, 0, 0..4, false),
        RunDrawCall::new(card, 0, 4..5, true),
        RunDrawCall::new(bars, 3, 5..9, false),
        RunDrawCall::new(card, 0, 9..10, true),
        RunDrawCall::new(bars, 0, 10..14, false),
    ];
    let planned: Vec<_> = interior_run_draws(&draws)
        .into_iter()
        .map(|draw| (draw.key, draw.records))
        .collect();
    assert_eq!(
        planned,
        [(interior, 4..10)],
        "one call from the first card to the last, the plain draws past them left out"
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
    assert!(!staging.draws[0].occluders);
    staging.push_draw(key, 0, 1, true);
    staging.push_draw(key, 0, 2, false);
    assert_eq!(staging.draws.len(), 1);
    assert!(staging.draws[0].occluders);
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
    let data = PlacementData::of(&placement, 2.0, crate::geometry::SegmentTransform::IDENTITY);
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
    let plain = PlacementData::of(
        &Placement::at(Point::default(), None, None),
        1.0,
        crate::geometry::SegmentTransform::IDENTITY,
    );
    assert_eq!(plain.flags, 0);
    assert_eq!(plain.color_matrix[2][2], 1.0);
    assert_eq!(plain.transform, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(plain.translation, [0.0, 0.0]);
    let turn = crate::geometry::SegmentTransform::affine([0.0, -1.0, 1.0, 0.0], [5.0, 7.0])
        .expect("a quarter turn");
    let turned = PlacementData::of(&Placement::at(Point::default(), None, None), 1.0, turn);
    assert_eq!(
        (turned.transform, turned.translation),
        ([0.0, -1.0, 1.0, 0.0], [5.0, 7.0]),
        "the records of a layer drawn in place carry its turn"
    );
    assert_eq!(
        turned.flags, PLACEMENT_TURNED,
        "and say so, so the one pipeline draws them turned"
    );
    assert_eq!(std::mem::size_of::<PlacementData>(), 160);
}

#[test]
fn shape_turns_follow_the_placement_unless_the_pass_mixes_flat_and_turned_records() {
    use crate::{geometry::SegmentTransform, render::ShapeTurns};
    let Some(turned) = SegmentTransform::affine([0.0, -1.0, 1.0, 0.0], [4.0, 2.0]) else {
        panic!("a quarter turn is invertible");
    };
    assert_eq!(
        ShapeTurns::of(SegmentTransform::IDENTITY, false),
        ShapeTurns::None
    );
    assert_eq!(ShapeTurns::of(turned, false), ShapeTurns::All);
    assert_eq!(
        ShapeTurns::of(SegmentTransform::IDENTITY, true),
        ShapeTurns::Mixed
    );
    assert_eq!(ShapeTurns::of(turned, true), ShapeTurns::Mixed);
}

#[test]
fn a_solid_segment_never_tests_its_interiors() {
    use crate::render::ShapeVariant;
    for clipped in [false, true] {
        let variant = |interiors: bool| {
            let segment = shape_segment(1, BlendMode::SrcOver, (interiors, false, false));
            ShapeVariant::of_segment(&segment, clipped, Default::default(), false, true)
        };
        assert_eq!(variant(true), variant(false), "clipped: {clipped}");
    }
}

#[test]
fn a_segment_whose_interiors_a_pre_pass_lays_down_skips_the_interior_fast_path() {
    use crate::render::ShapeVariant;
    let segment = |occluders: bool, bare_interiors: bool| RecordSegment {
        gradient: true,
        ..shape_segment(1, BlendMode::SrcOver, (true, occluders, bare_interiors))
    };
    // Whether a pre-pass laying interiors down changes the variant: only by
    // dropping the interior's fast path, which the test cannot read apart.
    let drops_fast_path = |segment: &RecordSegment| {
        ShapeVariant::of_segment(segment, false, Default::default(), true, true)
            != ShapeVariant::of_segment(segment, false, Default::default(), false, true)
    };
    assert!(drops_fast_path(&segment(true, false)), "laid down ahead");
    assert!(
        !drops_fast_path(&segment(true, true)),
        "a translucent fill beside"
    );
    assert!(
        !drops_fast_path(&segment(false, false)),
        "no occluder, no span"
    );
}
