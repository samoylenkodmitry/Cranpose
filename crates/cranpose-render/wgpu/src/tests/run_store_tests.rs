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
            crate::render::SegmentClip::Untested,
            Default::default(),
            false,
            (true, true),
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
        clip_radius: 0.0,
        alpha: 0.5,
        color_filter: Some(Arc::new(ColorFilter::modulate(Color(0.5, 0.25, 1.0, 1.0)))),
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
    use crate::render::{SegmentClip, ShapeVariant};
    for clip in [SegmentClip::Untested, SegmentClip::Tested] {
        let variant = |interiors: bool| {
            let segment = shape_segment(1, BlendMode::SrcOver, (interiors, false, false));
            ShapeVariant::of_segment(&segment, clip, Default::default(), false, (true, true))
        };
        assert_eq!(variant(true), variant(false), "clip: {clip:?}");
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
        let clip = crate::render::SegmentClip::Untested;
        ShapeVariant::of_segment(segment, clip, Default::default(), true, (true, true))
            != ShapeVariant::of_segment(segment, clip, Default::default(), false, (true, true))
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

fn fill_run(solid: usize, gradient: usize) -> RunDraw {
    use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect, ShapeRecorder};
    let mut recorder = ShapeRecorder::default();
    let mut push = |index: usize, brush: Brush| {
        recorder.push_primitive(DrawPrimitive::Rect {
            rect: Rect {
                x: (index % 16) as f32 * 4.0,
                y: (index / 16) as f32 * 4.0,
                width: 3.0,
                height: 3.0,
            },
            brush,
            stroke: None,
        });
    };
    for index in 0..solid {
        push(index, Brush::solid(Color::WHITE));
    }
    for index in solid..solid + gradient {
        push(
            index,
            Brush::linear_gradient(vec![Color::WHITE, Color(0.0, 0.0, 1.0, 1.0)]),
        );
    }
    RunDraw::whole(
        std::sync::Arc::new(recorder),
        crate::scene::Placement::at(Point::default(), None, None),
    )
    .expect("recorded run")
}

/// The vertices each pipeline key draws for `window` of `run`, appended
/// into a fresh arena chunk.
fn arena_wants(
    store: &mut RunStore,
    run: &RunDraw,
    window: std::ops::Range<u32>,
) -> (u32, Vec<(crate::render::ShapePipelineKey, u64)>) {
    let chunk = store.open_arena();
    let mut wanted = crate::shape_pipelines::PipelineDemand::default();
    let taken = store.append_arena(
        chunk,
        run,
        window,
        (1.0, crate::geometry::SegmentTransform::IDENTITY),
        &mut |segment| arena_key(segment),
        &mut wanted,
    );
    (taken, wanted.drain().collect())
}

#[test]
fn an_arena_append_wants_pipelines_for_the_records_its_window_takes() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut store = RunStore::new(
        &device,
        RunBufferMode {
            storage: true,
            trig_fill: false,
        },
        UploadMode::Copied,
    );
    let run = fill_run(3, 2);
    let keys: Vec<_> = run.segment_records().map(arena_key).collect();
    assert_eq!(keys.len(), 2, "solid and gradient fills take two segments");
    let (taken, wanted) = arena_wants(&mut store, &run, 2..5);
    assert_eq!(taken, 3);
    assert_eq!(wanted, [(keys[0], 4), (keys[1], 8)]);
    let (taken, wanted) = arena_wants(&mut store, &run, 0..2);
    assert_eq!(taken, 2);
    assert_eq!(
        wanted,
        [(keys[0], 8)],
        "a window that ends inside the first segment wants nothing of the second"
    );
}

#[test]
fn an_arena_chunk_that_fills_up_wants_only_the_records_it_took() {
    let (_lock, device, _queue) = crate::frame_graph::upload_test_device();
    let mut store = RunStore::new(
        &device,
        RunBufferMode {
            storage: false,
            trig_fill: false,
        },
        UploadMode::Copied,
    );
    let records = RECORD_CHUNK as u32 + 40;
    let run = fill_run(records as usize, 0);
    let key = arena_key(run.segment_records().next().expect("one segment"));
    let (first, wanted) = arena_wants(&mut store, &run, 0..u32::MAX);
    assert_eq!(first, RECORD_CHUNK as u32);
    assert_eq!(wanted, [(key, u64::from(first) * 4)]);
    let (rest, wanted) = arena_wants(&mut store, &run, first..u32::MAX);
    assert_eq!(rest, 40);
    assert_eq!(wanted, [(key, 160)]);
}

/// The modes the test device offers: the copied one always, the mapped
/// one when the device maps its primary buffers.
fn upload_modes(device: &wgpu::Device) -> Vec<UploadMode> {
    let mut modes = vec![UploadMode::Copied];
    if UploadMode::for_device(device) == UploadMode::Mapped {
        modes.push(UploadMode::Mapped);
    }
    modes
}

fn storage_store(device: &wgpu::Device, upload: UploadMode) -> RunStore {
    RunStore::new(
        device,
        RunBufferMode {
            storage: true,
            trig_fill: false,
        },
        upload,
    )
}

/// A stored run of `records` gradient rects as one command, painted at
/// `alpha`; record `marked` takes a colour of its own.
fn stored_gradient_run(records: usize, marked: usize, alpha: f32) -> RunDraw {
    use cranpose_ui_graphics::{Brush, Color, DrawPrimitive, Rect, ShapeRecorder};
    let mut recorder = ShapeRecorder::default();
    for index in 0..records {
        let first = if index == marked {
            Color(1.0, 0.0, 0.0, 1.0)
        } else {
            Color(0.0, 1.0, 0.0, 1.0)
        };
        recorder.push_primitive(DrawPrimitive::Rect {
            rect: Rect {
                x: (index % 16) as f32 * 4.0,
                y: (index / 16) as f32 * 4.0,
                width: 3.0,
                height: 3.0,
            },
            brush: Brush::linear_gradient(vec![first, Color(0.0, 0.0, 1.0, 1.0)]),
            stroke: None,
        });
    }
    let mut placement = crate::scene::Placement::at(Point::default(), None, None);
    placement.alpha = alpha;
    let mut run = RunDraw::whole(std::sync::Arc::new(recorder), placement).expect("recorded run");
    run.command = Some(DrawCommandId {
        node_id: 7,
        command_index: 0,
        placement: cranpose_render_common::style_shared::DrawPlacement::Behind,
    });
    run
}

/// A readback buffer the size of `tables`' buffer, with a copy of it
/// recorded into the pass where the copy stands in for a draw.
fn copy_tables(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    tables: &StoredTables,
) -> wgpu::Buffer {
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: tables.buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&tables.buffer, 0, &readback, 0, tables.buffer.size());
    readback
}

fn holds(bytes: &[u8], table: &[u8]) -> bool {
    bytes.windows(table.len()).any(|window| window == table)
}

fn painted_stop_bytes(run: &RunDraw) -> Vec<u8> {
    let mut painted = Vec::new();
    painted_stops(
        &run.tables().stops,
        &paint_layer(&run.placement),
        &mut painted,
    );
    bytemuck::cast_slice(&painted).to_vec()
}

/// One frame that uploads `runs` in order, each read by a copy recorded
/// right after its upload as the pass that draws it would read it; returns
/// the copies and the frame's submission.
fn upload_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    store: &mut RunStore,
    runs: &[&RunDraw],
) -> (Vec<wgpu::Buffer>, wgpu::SubmissionIndex) {
    store.begin_frame(false);
    let mut readbacks = Vec::new();
    let mut executor = crate::frame_graph::WgpuFrameGraphExecutor::new();
    let mut graph = crate::frame_graph::WgpuFrameGraph::new(None);
    graph.add_fallible_command_pass(Some("stored run uploads"), &[], &[], |context| {
        for run in runs {
            let (_, _, tables) =
                store.upload_stored(device, context, run, 1.0, &(0..u32::MAX), &[]);
            readbacks.push(copy_tables(device, context.encoder, &tables));
        }
        Ok(())
    });
    let execution = executor
        .execute_recorded_graph(device, queue, graph)
        .expect("the frame submits");
    store.finish_frame();
    store.recall();
    (readbacks, execution.submission)
}

#[test]
fn a_stored_run_rewritten_within_a_frame_leaves_each_read_its_own_tables() {
    let (_lock, device, queue) = crate::frame_graph::upload_test_device();
    for upload in upload_modes(&device) {
        let mut store = storage_store(&device, upload);
        let opaque = stored_gradient_run(80, 0, 1.0);
        let faded = RunDraw {
            placement: crate::scene::Placement {
                alpha: 0.25,
                ..opaque.placement.clone()
            },
            ..opaque.clone()
        };
        upload_frame(&device, &queue, &mut store, &[&opaque]);
        let (readbacks, submission) = upload_frame(&device, &queue, &mut store, &[&opaque, &faded]);
        let first =
            crate::frame_graph::read_uploaded_bytes(&device, &readbacks[0], submission.clone());
        let second = crate::frame_graph::read_uploaded_bytes(&device, &readbacks[1], submission);
        let (opaque_stops, faded_stops) = (painted_stop_bytes(&opaque), painted_stop_bytes(&faded));
        assert!(
            holds(&first, &opaque_stops) && !holds(&first, &faded_stops),
            "{upload:?}: the read before the rewrite sees the opaque stops"
        );
        assert!(
            holds(&second, &faded_stops),
            "{upload:?}: the read after the rewrite sees the faded stops"
        );
        let bodies: &[u8] = bytemuck::cast_slice(opaque.tables().shapes.bodies());
        assert!(
            holds(&first, bodies) && holds(&second, bodies),
            "{upload:?}"
        );
    }
}

/// Uploads a stored run that changes every frame in a different place, and
/// sometimes grows, and checks every frame read its own tables whole: with
/// the GPU done before each next frame, and with every frame in flight.
#[test]
fn stored_runs_changed_while_earlier_frames_are_in_flight_keep_each_frames_tables_whole() {
    const RECORDS: usize = 400;
    // A body is 64 bytes: 64 records fill an upload chunk.
    let marked = |frame: usize| (frame * 64 * 3) % RECORDS;
    let (_lock, device, queue) = crate::frame_graph::upload_test_device();
    for upload in upload_modes(&device) {
        for wait in [true, false] {
            let mut store = storage_store(&device, upload);
            let runs: Vec<RunDraw> = (0..10)
                .map(|frame| {
                    let records = if frame % 4 == 3 {
                        RECORDS + 70
                    } else {
                        RECORDS
                    };
                    stored_gradient_run(records, marked(frame), 1.0)
                })
                .collect();
            let mut frames = Vec::new();
            for run in &runs {
                frames.push(upload_frame(&device, &queue, &mut store, &[run]));
                if wait {
                    device
                        .poll(wgpu::PollType::wait_indefinitely())
                        .expect("idle device");
                }
            }
            for (frame, ((readbacks, submission), run)) in frames.into_iter().zip(&runs).enumerate()
            {
                let read =
                    crate::frame_graph::read_uploaded_bytes(&device, &readbacks[0], submission);
                let tables = run.tables();
                for (table, bytes) in [
                    (
                        "bodies",
                        bytemuck::cast_slice::<_, u8>(tables.shapes.bodies()),
                    ),
                    ("curves", bytemuck::cast_slice(tables.shapes.curves())),
                    ("brushes", bytemuck::cast_slice(&tables.brushes)),
                    ("stops", &painted_stop_bytes(run)[..]),
                ] {
                    assert!(
                        holds(&read, bytes),
                        "{upload:?}, waiting {wait}: frame {frame} reads its own {table} whole"
                    );
                }
            }
        }
    }
}
