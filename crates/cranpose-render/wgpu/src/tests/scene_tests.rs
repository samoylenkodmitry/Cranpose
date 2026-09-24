use cranpose_ui_graphics::Brush;

use super::*;

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn loose_rect(x: f32) -> DrawPrimitive {
    DrawPrimitive::Rect {
        rect: rect(x, 0.0, 10.0, 10.0),
        brush: Brush::solid(Color::WHITE),
        stroke: None,
    }
}

fn shadow_draw(shapes: Option<RunDraw>, post_blur_cutouts: Option<RunDraw>) -> ShadowDraw {
    ShadowDraw {
        shapes,
        post_blur_cutouts,
        texts: Vec::new(),
        blur_radius: 4.0,
        clip: None,
        rounded_clip: None,
        occluder: None,
        z_index: 0,
    }
}

#[test]
fn recycled_shadow_recorders_clear_geometry_and_preserve_retained_runs() {
    let mut scene = CompositorScene::new();
    let placement = Placement::at(Point::default(), None, None);
    let mut caster = scene.take_shadow_recorder();
    Arc::make_mut(&mut caster).push_primitive(loose_rect(0.0));
    let caster = RunDraw::whole(caster, placement).unwrap();
    let retained = caster.clone();
    let fingerprint = retained.recorder.fingerprint();
    let mut cutout = scene.take_shadow_recorder();
    Arc::make_mut(&mut cutout).push_primitive(loose_rect(20.0));
    let storage = cutout.tables().shapes.bodies().as_ptr();
    let allocation = Arc::as_ptr(&cutout);
    scene.push_shadow_draw(shadow_draw(Some(caster), RunDraw::whole(cutout, placement)));
    scene.clear();

    let mut recycled = scene.take_shadow_recorder();
    assert_eq!(Arc::as_ptr(&recycled), allocation);
    assert_eq!(recycled.tables().shapes.bodies().as_ptr(), storage);
    assert!(recycled.is_empty());
    assert_eq!(recycled.bounds(), None);
    assert!(recycled.tables().segments.is_empty());
    Arc::make_mut(&mut recycled).push_primitive(loose_rect(80.0));
    let replacement = RunDraw::whole(recycled, placement).unwrap();
    assert_eq!(replacement.record_count(), 1);
    assert_eq!(replacement.bounds, rect(80.0, 0.0, 10.0, 10.0));
    assert_eq!(retained.record_count(), 1);
    assert_eq!(retained.bounds, rect(0.0, 0.0, 10.0, 10.0));
    assert_eq!(retained.recorder.fingerprint(), fingerprint);
    scene.push_shadow_draw(shadow_draw(Some(replacement), None));
    drop(scene);

    let mut next_scene = CompositorScene::new();
    let recycled = next_scene.take_shadow_recorder();
    assert_eq!(recycled.tables().shapes.bodies().as_ptr(), storage);
    assert!(recycled.is_empty());
}

#[test]
fn externally_recorded_shadows_do_not_grow_the_recorder_pool() {
    let mut scene = CompositorScene::new();
    let placement = Placement::at(Point::default(), None, None);
    let append = |scene: &mut CompositorScene| {
        let mut recorder = ShapeRecorder::default();
        recorder.push_primitive(loose_rect(0.0));
        scene.push_shadow_draw(shadow_draw(
            RunDraw::whole(Arc::new(recorder), placement),
            None,
        ));
    };
    append(&mut scene);
    let limit = scene.shadow_draws.capacity() * 2;
    scene.clear();
    for _ in 0..=limit {
        append(&mut scene);
        scene.clear();
    }
    assert!(scene.shadow_recorders.len() <= limit);
}

#[test]
fn full_scene_pool_recycles_the_latest_shadow_storage() {
    let mut scene = CompositorScene::new();
    let mut recorder = scene.take_shadow_recorder();
    Arc::make_mut(&mut recorder).push_primitive(loose_rect(10.0));
    let storage = recorder.tables().shapes.bodies().as_ptr();
    scene.push_shadow_draw(shadow_draw(
        RunDraw::whole(recorder, Placement::at(Point::default(), None, None)),
        None,
    ));
    let earlier: Vec<_> = (0..SCENE_BUFFER_POOL_LIMIT)
        .map(|_| CompositorScene::new())
        .collect();
    drop(earlier);
    SCENE_BUFFER_POOL.with(|pool| assert_eq!(pool.borrow().len(), SCENE_BUFFER_POOL_LIMIT));
    drop(scene);
    SCENE_BUFFER_POOL.with(|pool| assert_eq!(pool.borrow().len(), SCENE_BUFFER_POOL_LIMIT));

    let mut next = CompositorScene::new();
    let recycled = next.take_shadow_recorder();
    assert!(recycled.is_empty());
    assert_eq!(recycled.tables().shapes.bodies().as_ptr(), storage);
}

#[test]
fn loose_primitives_under_one_placement_share_a_run_and_close_before_anything_else() {
    let mut scene = CompositorScene::new();
    let placement = Placement::at(Point::new(5.0, 5.0), None, None);
    scene.push_loose(loose_rect(0.0), placement);
    scene.push_loose(loose_rect(20.0), placement);
    assert!(scene.runs.is_empty());
    let z = scene.next_z();
    assert_eq!(scene.runs.len(), 1);
    assert_eq!(z, 1);
    assert_eq!(scene.runs[0].record_count(), 2);
    assert_eq!(scene.runs[0].bounds, rect(5.0, 5.0, 30.0, 10.0));
    scene.push_loose(
        loose_rect(40.0),
        Placement::at(Point::default(), None, None),
    );
    scene.push_loose(
        loose_rect(60.0),
        Placement::at(Point::default(), None, Some(rect(0.0, 0.0, 1.0, 1.0))),
    );
    scene.flush_loose();
    assert_eq!(scene.runs.len(), 3);
    assert_eq!(
        scene
            .draw_ops
            .iter()
            .map(|op| op.z_index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[test]
fn a_run_pushed_after_loose_primitives_draws_above_them() {
    let mut scene = CompositorScene::new();
    scene.push_loose(loose_rect(0.0), Placement::at(Point::default(), None, None));
    let recording = CommandRecording::from_primitives(vec![loose_rect(1.0)]);
    scene.push_run(RunDraw::of(
        &recording,
        None,
        recording.all_segments(),
        Placement::at(Point::default(), None, None),
    ));
    assert_eq!(scene.runs.len(), 2);
    assert!(matches!(scene.draw_ops[0].kind, DrawOpKind::Run(0)));
    assert!(matches!(scene.draw_ops[1].kind, DrawOpKind::Run(1)));
}

#[test]
fn scene_drop_skips_pooling_when_the_pool_is_held() {
    let scene = CompositorScene::new();
    SCENE_BUFFER_POOL.with(|pool| {
        let _held = pool.borrow_mut();
        let dropped = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(scene)));
        assert!(
            dropped.is_ok(),
            "dropping a scene under a held pool borrow must not panic"
        );
    });
}
