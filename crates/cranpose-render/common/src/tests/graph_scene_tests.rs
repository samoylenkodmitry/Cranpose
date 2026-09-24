use std::cell::Cell;

use super::*;

fn rect_to_quad(rect: Rect) -> [[f32; 2]; 4] {
    [
        [rect.x, rect.y],
        [rect.x + rect.width, rect.y],
        [rect.x, rect.y + rect.height],
        [rect.x + rect.width, rect.y + rect.height],
    ]
}

fn translated_world_to_local(rect: Rect) -> ProjectiveTransform {
    ProjectiveTransform::translation(-rect.x, -rect.y)
}

fn local_bounds_for_rect(rect: Rect) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: rect.width,
        height: rect.height,
    }
}

fn hit_geometry_for_rect(rect: Rect) -> HitGeometry<'static> {
    HitGeometry {
        rect,
        quad: rect_to_quad(rect),
        local_bounds: local_bounds_for_rect(rect),
        world_to_local: translated_world_to_local(rect),
        hit_clip_bounds: None,
        hit_clips: &[],
    }
}

fn test_diagnostics() -> Rc<RenderDiagnostics> {
    Rc::new(RenderDiagnostics::new())
}

fn make_handler(counter: Rc<Cell<u32>>, consume: bool) -> Rc<dyn Fn(PointerEvent)> {
    Rc::new(move |event: PointerEvent| {
        counter.set(counter.get() + 1);
        if consume {
            event.consume();
        }
    })
}

#[test]
fn rebuilding_hits_reuses_buffers_releases_handlers_and_preserves_captured_targets() {
    let mut scene = Scene::new();
    let first_count = Rc::new(Cell::new(0));
    let second_count = Rc::new(Cell::new(0));
    let first = make_handler(Rc::clone(&first_count), false);
    let second = make_handler(Rc::clone(&second_count), false);
    let first_click: Rc<dyn Fn(Point)> = Rc::new(|_| {});
    let second_click: Rc<dyn Fn(Point)> = Rc::new(|_| {});
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 30.0,
        height: 30.0,
    };
    scene.push_hit(
        1,
        &[1, 9],
        hit_geometry_for_rect(rect),
        HitTargetSpec {
            shape: None,
            click_actions: [ClickAction::WithPoint(Rc::clone(&first_click))],
            pointer_inputs: &[Rc::clone(&first)],
            pointer_icon: None,
        },
    );
    let captured = scene.find_target(1).unwrap();
    let path_storage = scene.hits[0].capture_path.as_ptr();
    let input_storage = scene.hits[0].pointer_inputs.as_ptr();
    scene.clear_hits();
    assert!(scene.hits.is_empty());
    assert!(scene.find_target(1).is_none());
    assert_eq!(scene.next_hit_z, 0);
    assert_eq!(Rc::strong_count(&first), 2);
    assert_eq!(Rc::strong_count(&first_click), 2);
    scene.push_hit(
        2,
        &[2],
        hit_geometry_for_rect(rect),
        HitTargetSpec {
            shape: None,
            click_actions: [ClickAction::WithPoint(Rc::clone(&second_click))],
            pointer_inputs: &[Rc::clone(&second)],
            pointer_icon: None,
        },
    );
    assert_eq!(scene.hits[0].capture_path.as_ptr(), path_storage);
    assert_eq!(scene.hits[0].pointer_inputs.as_ptr(), input_storage);
    assert_eq!(scene.hits[0].capture_path, [2]);
    assert_eq!(scene.hits[0].z_index, 0);
    assert_eq!(scene.hits[0].click_actions.len(), 1);
    assert!(
        matches!(&scene.hits[0].click_actions[0], ClickAction::WithPoint(handler) if Rc::ptr_eq(handler, &second_click))
    );
    let event = PointerEvent::new(
        PointerEventKind::Down,
        Point::new(5.0, 5.0),
        Point::new(5.0, 5.0),
    );
    scene.find_target(2).unwrap().dispatch(event.clone());
    assert_eq!(first_count.get(), 0);
    assert_eq!(second_count.get(), 1);
    captured.dispatch(event);
    assert_eq!(captured.capture_path, [1, 9]);
    assert_eq!(first_count.get(), 1);
    scene.clear_hits();
    assert_eq!(Rc::strong_count(&second), 1);
    assert_eq!(Rc::strong_count(&second_click), 1);
    scene.push_hit(
        3,
        &[3],
        hit_geometry_for_rect(rect),
        HitTargetSpec {
            shape: None,
            click_actions: [],
            pointer_inputs: &[],
            pointer_icon: None,
        },
    );
    assert!(scene.hits.is_empty());
    assert_eq!(scene.next_hit_z, 0);
}

#[test]
fn collecting_observation_owners_clears_an_empty_scene() {
    let scene = Scene::new();
    let mut nodes = HashSet::from_iter([13, 17]);
    let capacity = nodes.capacity();
    assert!(scene.collect_retained_visual_observation_nodes(&mut nodes));
    assert!(nodes.is_empty());
    assert_eq!(nodes.capacity(), capacity);
}

#[test]
fn rebuilding_clip_buffers_replaces_clips_without_changing_captured_targets() {
    let mut scene = Scene::new();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    let left = Rect {
        width: 50.0,
        ..rect
    };
    let right = Rect {
        x: 50.0,
        width: 50.0,
        ..rect
    };
    let clip = |bounds| HitClip {
        quad: rect_to_quad(bounds),
        bounds,
    };
    let handler = make_handler(Rc::new(Cell::new(0)), false);
    let push = |scene: &mut Scene, clips: &[HitClip]| {
        scene.push_hit(
            1,
            &[1],
            HitGeometry {
                hit_clips: clips,
                ..hit_geometry_for_rect(rect)
            },
            HitTargetSpec {
                shape: None,
                click_actions: [],
                pointer_inputs: &[Rc::clone(&handler)],
                pointer_icon: None,
            },
        );
    };
    push(&mut scene, &[clip(left)]);
    let captured = scene.find_target(1).unwrap();
    let storage = scene.hits[0].hit_clips.as_ptr();
    assert!(captured.contains(25.0, 25.0));
    assert!(!captured.contains(75.0, 25.0));
    scene.clear_hits();
    push(&mut scene, &[clip(right)]);
    assert_eq!(scene.hits[0].hit_clips.as_ptr(), storage);
    assert!(scene.hit_test(25.0, 25.0).is_empty());
    assert_eq!(scene.hit_test(75.0, 25.0).len(), 1);
    assert!(captured.contains(25.0, 25.0));
    assert!(!captured.contains(75.0, 25.0));
    scene.clear_hits();
    push(&mut scene, &[]);
    assert_eq!(scene.hit_test(25.0, 25.0).len(), 1);
    assert_eq!(scene.hit_test(75.0, 25.0).len(), 1);
}

#[test]
fn hit_test_respects_hit_clip() {
    let mut scene = Scene::new();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 100.0,
        height: 100.0,
    };
    let clip = Rect {
        x: 0.0,
        y: 0.0,
        width: 40.0,
        height: 40.0,
    };
    scene.push_hit(
        1,
        &[1],
        HitGeometry {
            hit_clip_bounds: Some(clip),
            hit_clips: &[HitClip {
                quad: rect_to_quad(clip),
                bounds: clip,
            }],
            ..hit_geometry_for_rect(rect)
        },
        HitTargetSpec {
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: &[Rc::new(|_event: PointerEvent| {})],
            pointer_icon: None,
        },
    );

    assert!(scene.hit_test(60.0, 20.0).is_empty());
    assert_eq!(scene.hit_test(20.0, 20.0).len(), 1);
}

fn push_input_target(scene: &mut Scene, node_id: NodeId, rect: Rect) {
    scene.push_hit(
        node_id,
        &[node_id],
        hit_geometry_for_rect(rect),
        HitTargetSpec {
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: &[Rc::new(|_event: PointerEvent| {})],
            pointer_icon: None,
        },
    );
}

fn small_target(x: f32) -> Rect {
    Rect {
        x,
        y: 100.0,
        width: 20.0,
        height: 20.0,
    }
}

#[test]
fn a_press_beside_a_small_target_reaches_it_when_nothing_else_claims_the_point() {
    let mut scene = Scene::new();
    push_input_target(&mut scene, 1, small_target(100.0));

    assert!(scene.hit_test(125.0, 110.0).is_empty());
    let near = scene
        .hit_test_near(125.0, 110.0)
        .expect("the press lands inside the target grown to 48");
    assert_eq!(near.node_id, 1);
    assert!(scene.hit_test_near(140.0, 110.0).is_none());
}

#[test]
fn the_nearest_small_target_takes_the_press() {
    let mut scene = Scene::new();
    push_input_target(&mut scene, 1, small_target(100.0));
    push_input_target(&mut scene, 2, small_target(140.0));

    assert_eq!(
        scene.hit_test_near(122.0, 110.0).map(|hit| hit.node_id),
        Some(1)
    );
    assert_eq!(
        scene.hit_test_near(138.0, 110.0).map(|hit| hit.node_id),
        Some(2)
    );
}

#[test]
fn a_target_of_the_minimum_size_has_no_reach_beyond_its_box() {
    let mut scene = Scene::new();
    push_input_target(
        &mut scene,
        1,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 48.0,
            height: 48.0,
        },
    );

    assert!(scene.hit_test_near(50.0, 10.0).is_none());
}

#[test]
fn hit_test_sorts_by_z_without_duplicating_hit_storage() {
    let mut scene = Scene::new();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 50.0,
        height: 50.0,
    };

    scene.push_hit(
        1,
        &[1],
        hit_geometry_for_rect(rect),
        HitTargetSpec {
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: &[Rc::new(|_event: PointerEvent| {})],
            pointer_icon: None,
        },
    );
    scene.push_hit(
        2,
        &[2],
        hit_geometry_for_rect(rect),
        HitTargetSpec {
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: &[Rc::new(|_event: PointerEvent| {})],
            pointer_icon: None,
        },
    );

    assert_eq!(scene.node_index.get(&1), Some(&0));
    assert_eq!(scene.node_index.get(&2), Some(&1));

    let hits = scene.hit_test(10.0, 10.0);
    assert_eq!(
        hits.iter().map(|hit| hit.node_id).collect::<Vec<_>>(),
        vec![2, 1]
    );
    assert_eq!(scene.find_target(1).map(|hit| hit.node_id), Some(1));
    assert_eq!(scene.find_target(2).map(|hit| hit.node_id), Some(2));
}

#[test]
fn hit_test_rejects_points_in_rounded_corner_cutout() {
    let mut scene = Scene::new();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 40.0,
        height: 40.0,
    };
    scene.push_hit(
        1,
        &[1],
        hit_geometry_for_rect(rect),
        HitTargetSpec {
            shape: Some(RoundedCornerShape::uniform(20.0)),
            click_actions: Vec::new(),
            pointer_inputs: &[Rc::new(|_event: PointerEvent| {})],
            pointer_icon: None,
        },
    );

    assert!(scene.hit_test(1.0, 1.0).is_empty());
    assert_eq!(scene.hit_test(20.0, 20.0).len(), 1);
}

#[test]
fn render_diagnostics_claim_each_warning_key_once() {
    let diagnostics = RenderDiagnostics::new();

    assert!(diagnostics.claim_warning_once("pixels.effect-fallback"));
    assert!(!diagnostics.claim_warning_once("pixels.effect-fallback"));
    assert!(diagnostics.claim_warning_once("pixels.blend-fallback"));
}

#[test]
fn dispatch_stops_after_event_consumed() {
    let count_first = Rc::new(Cell::new(0));
    let count_second = Rc::new(Cell::new(0));

    let hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 1,
        capture_path: vec![1],
        geometry: hit_geometry_for_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        }),
        pointer_inputs: vec![
            make_handler(count_first.clone(), true),
            make_handler(count_second.clone(), false),
        ],
        ..Default::default()
    });

    let event = PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 10.0, y: 10.0 },
        Point { x: 10.0, y: 10.0 },
    );
    hit.dispatch(event);

    assert_eq!(count_first.get(), 1);
    assert_eq!(count_second.get(), 0);
}

#[test]
fn dispatch_delivers_terminal_events_after_consumption_for_cleanup() {
    let count_first = Rc::new(Cell::new(0));
    let count_second = Rc::new(Cell::new(0));

    let hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 1,
        capture_path: vec![1],
        geometry: hit_geometry_for_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        }),
        pointer_inputs: vec![
            make_handler(count_first.clone(), true),
            make_handler(count_second.clone(), false),
        ],
        ..Default::default()
    });

    for kind in [PointerEventKind::Up, PointerEventKind::Cancel] {
        let event = PointerEvent::new(kind, Point { x: 10.0, y: 10.0 }, Point { x: 10.0, y: 10.0 });
        hit.dispatch(event);
    }

    assert_eq!(count_first.get(), 2);
    assert_eq!(count_second.get(), 2);
}

#[test]
fn dispatch_delivers_terminal_events_to_later_captured_targets_after_consumption() {
    let child_count = Rc::new(Cell::new(0));
    let parent_count = Rc::new(Cell::new(0));

    let child_hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 2,
        capture_path: vec![2, 1],
        geometry: hit_geometry_for_rect(Rect {
            x: 8.0,
            y: 8.0,
            width: 20.0,
            height: 20.0,
        }),
        pointer_inputs: vec![make_handler(child_count.clone(), true)],
        z_index: 1,
        ..Default::default()
    });
    let parent_hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 1,
        capture_path: vec![1],
        geometry: hit_geometry_for_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        }),
        pointer_inputs: vec![make_handler(parent_count.clone(), false)],
        ..Default::default()
    });

    let event = PointerEvent::new(
        PointerEventKind::Up,
        Point { x: 12.0, y: 12.0 },
        Point { x: 12.0, y: 12.0 },
    );
    child_hit.dispatch(event.clone());
    parent_hit.dispatch(event);

    assert_eq!(child_count.get(), 1);
    assert_eq!(parent_count.get(), 1);
}

#[test]
fn dispatch_triggers_click_action_on_down() {
    let click_count = Rc::new(Cell::new(0));
    let click_count_for_handler = Rc::clone(&click_count);
    let click_action = ClickAction::Simple(Rc::new(RefCell::new(move || {
        click_count_for_handler.set(click_count_for_handler.get() + 1);
    })));

    let hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 1,
        capture_path: vec![1],
        geometry: hit_geometry_for_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        }),
        click_actions: vec![click_action],
        ..Default::default()
    });

    hit.dispatch(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 10.0, y: 10.0 },
        Point { x: 10.0, y: 10.0 },
    ));
    hit.dispatch(PointerEvent::new(
        PointerEventKind::Move,
        Point { x: 10.0, y: 10.0 },
        Point { x: 12.0, y: 12.0 },
    ));

    assert_eq!(click_count.get(), 1);
}

#[test]
fn dispatch_passes_local_position_to_click_action() {
    let local_positions = Rc::new(RefCell::new(Vec::new()));
    let local_positions_for_handler = Rc::clone(&local_positions);
    let click_action = ClickAction::WithPoint(Rc::new(move |point| {
        local_positions_for_handler.borrow_mut().push(point);
    }));

    let hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 1,
        capture_path: vec![1],
        geometry: hit_geometry_for_rect(Rect {
            x: 10.0,
            y: 12.0,
            width: 50.0,
            height: 50.0,
        }),
        click_actions: vec![click_action],
        ..Default::default()
    });

    hit.dispatch(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 15.0, y: 17.0 },
        Point { x: 15.0, y: 17.0 },
    ));

    assert_eq!(*local_positions.borrow(), vec![Point { x: 5.0, y: 5.0 }]);
}

#[test]
fn dispatch_does_not_trigger_click_action_when_consumed() {
    let click_count = Rc::new(Cell::new(0));
    let click_count_for_handler = Rc::clone(&click_count);
    let click_action = ClickAction::Simple(Rc::new(RefCell::new(move || {
        click_count_for_handler.set(click_count_for_handler.get() + 1);
    })));

    let hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 1,
        capture_path: vec![1],
        geometry: hit_geometry_for_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        }),
        click_actions: vec![click_action],
        pointer_inputs: vec![Rc::new(|event: PointerEvent| event.consume())],
        ..Default::default()
    });

    hit.dispatch(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 10.0, y: 10.0 },
        Point { x: 10.0, y: 10.0 },
    ));

    assert_eq!(click_count.get(), 0);
}

#[test]
fn hit_test_uses_exact_quad_for_transformed_region() {
    let mut scene = Scene::new();
    let rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 40.0,
        height: 20.0,
    };
    let quad = [[10.0, 10.0], [50.0, 10.0], [20.0, 30.0], [60.0, 30.0]];
    let world_to_local = ProjectiveTransform::from_rect_to_quad(rect, quad)
        .inverse()
        .expect("transformed hit region should be invertible");
    scene.push_hit(
        1,
        &[1],
        HitGeometry {
            rect: Rect {
                x: 10.0,
                y: 10.0,
                width: 50.0,
                height: 20.0,
            },
            quad,
            local_bounds: rect,
            world_to_local,
            hit_clip_bounds: None,
            hit_clips: &[],
        },
        HitTargetSpec {
            shape: None,
            click_actions: Vec::new(),
            pointer_inputs: &[Rc::new(|_event: PointerEvent| {})],
            pointer_icon: None,
        },
    );

    assert!(
        scene.hit_test(15.0, 28.0).is_empty(),
        "point inside the quad bounds but outside the transformed quad must not hit"
    );
    assert_eq!(scene.hit_test(30.0, 20.0).len(), 1);
}

#[test]
fn dispatch_uses_inverse_transform_for_local_position() {
    let local_positions = Rc::new(RefCell::new(Vec::new()));
    let local_positions_for_handler = Rc::clone(&local_positions);
    let click_action = ClickAction::WithPoint(Rc::new(move |point| {
        local_positions_for_handler.borrow_mut().push(point);
    }));
    let local_bounds = Rect {
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 10.0,
    };
    let quad = [[20.0, 10.0], [60.0, 10.0], [20.0, 30.0], [60.0, 30.0]];
    let world_to_local = ProjectiveTransform::from_rect_to_quad(local_bounds, quad)
        .inverse()
        .expect("translated quad should be invertible");
    let hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 1,
        capture_path: vec![1],
        geometry: HitGeometry {
            rect: Rect {
                x: 20.0,
                y: 10.0,
                width: 40.0,
                height: 20.0,
            },
            quad,
            local_bounds,
            world_to_local,
            hit_clip_bounds: None,
            hit_clips: &[],
        },
        click_actions: vec![click_action],
        ..Default::default()
    });

    hit.dispatch(PointerEvent::new(
        PointerEventKind::Down,
        Point { x: 25.0, y: 17.0 },
        Point { x: 25.0, y: 17.0 },
    ));

    assert_eq!(*local_positions.borrow(), vec![Point { x: 2.5, y: 3.5 }]);
}

#[test]
fn dispatch_with_applier_counts_live_modifier_slice_lookup_misses() {
    let handler_calls = Rc::new(Cell::new(0));
    let handler_calls_for_handler = Rc::clone(&handler_calls);
    let diagnostics = test_diagnostics();
    let hit = HitRegion::with_diagnostics(HitRegionInit {
        node_id: 42,
        capture_path: vec![42],
        geometry: hit_geometry_for_rect(Rect {
            x: 0.0,
            y: 0.0,
            width: 50.0,
            height: 50.0,
        }),
        pointer_inputs: vec![Rc::new(move |_event: PointerEvent| {
            handler_calls_for_handler.set(handler_calls_for_handler.get() + 1);
        })],
        diagnostics: Rc::clone(&diagnostics),
        ..Default::default()
    });
    let misses_before = diagnostics.live_modifier_slice_lookup_miss_count();
    let mut applier = MemoryApplier::new();

    hit.dispatch_with_applier(
        &mut applier,
        PointerEvent::new(
            PointerEventKind::Down,
            Point { x: 10.0, y: 10.0 },
            Point { x: 10.0, y: 10.0 },
        ),
    );

    assert_eq!(handler_calls.get(), 1);
    assert_eq!(
        diagnostics.live_modifier_slice_lookup_miss_count(),
        misses_before + 1
    );
}
