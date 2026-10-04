use super::{
    DeviceRect, MAX_SURFACE_PIXELS, Rect, rendered_surface, surface_pixels, surface_within_budget,
};

fn rect(width: f32, height: f32) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width,
        height,
    }
}

fn device(x: f32, y: f32, width: f32, height: f32) -> DeviceRect {
    DeviceRect {
        x,
        y,
        width,
        height,
    }
}

#[test]
fn a_promoted_control_renders_the_shadow_past_its_box() {
    // The receipts feed's star while held: a 76x52 dp box at 2.36 px per
    // dp whose shadow reaches 20 dp out, on a page that shows all of it.
    let page = device(0.0, 0.0, 1800.0, 1400.0);
    let whole = device(1812.0, 1183.0, 368.0, 312.0);
    assert_eq!(rendered_surface(whole, page, 9.0), whole);
}

#[test]
fn a_card_wider_than_the_page_costs_the_page_and_the_glass_reach() {
    let page = device(0.0, 0.0, 1800.0, 1400.0);
    let card = device(-400.0, 100.0, 3000.0, 400.0);
    assert_eq!(
        rendered_surface(card, page, 9.0),
        device(-9.0, 100.0, 1818.0, 400.0)
    );
}

#[test]
fn a_surface_inside_the_budget_keeps_every_pixel_its_content_draws() {
    let content = rect(1446.0, 3157.6);
    let own_box = rect(1412.0, 1480.0);
    assert!(surface_pixels(content, 1.0) <= MAX_SURFACE_PIXELS);
    assert_eq!(surface_within_budget(content, own_box, 1.0), content);
}

#[test]
fn content_past_the_budget_falls_back_to_the_layer_own_box() {
    let content = rect(5403.0, 3314.4);
    let own_box = rect(1412.0, 1480.0);
    assert!(
        surface_pixels(content, 1.0) > MAX_SURFACE_PIXELS,
        "the LeetCodeDaily draft that blanked the window"
    );
    assert_eq!(surface_within_budget(content, own_box, 1.0), own_box);
    assert!(surface_pixels(own_box, 1.0) <= MAX_SURFACE_PIXELS);
}

#[test]
fn the_scale_decides_the_budget_not_the_logical_size() {
    let content = rect(3000.0, 2000.0);
    let own_box = rect(1000.0, 800.0);
    assert_eq!(surface_within_budget(content, own_box, 1.0), content);
    assert_eq!(surface_within_budget(content, own_box, 3.0), own_box);
}

#[test]
fn a_box_no_smaller_than_its_content_is_not_worth_swapping_in() {
    let content = rect(6000.0, 6000.0);
    let own_box = rect(6000.0, 6000.0);
    assert_eq!(surface_within_budget(content, own_box, 1.0), content);
}

#[test]
fn ensuring_z_order_sorts_changed_keys_and_preserves_ties() {
    let mut values: Vec<_> = (0..96).map(|index| (index % 3, index)).collect();
    let expected: Vec<_> = (0..3)
        .flat_map(|z| (z..96).step_by(3).map(move |index| (z, index)))
        .collect();
    ensure_sorted_by_key(&mut values, |value| value.0);
    assert_eq!(values, expected);
    ensure_sorted_by_key(&mut values, |value| value.0);
    assert_eq!(values, expected);
    values[95].0 = 0;
    ensure_sorted_by_key(&mut values, |value| value.0);
    assert_eq!(values[32], (0, 95));
    assert_eq!(&values[..32], &expected[..32]);
    assert_eq!(&values[33..], &expected[32..95]);
    values.clear();
    ensure_sorted_by_key(&mut values, |value| value.0);
    assert!(values.is_empty());
}

#[test]
fn a_backdrop_captures_no_further_than_the_clip_it_is_drawn_in() {
    let mut shader = RuntimeShader::new("fn glass_fs() {}");
    shader.set_input_padding(30.0);
    let rect = Rect {
        x: 20.0,
        y: 96.0,
        width: 160.0,
        height: 52.0,
    };
    let list = Rect {
        x: 20.0,
        y: 96.0,
        width: 160.0,
        height: 300.0,
    };
    let target = DeviceRect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 800.0,
    };
    let layer = BackdropLayer {
        node_id: None,
        alpha: 1.0,
        rect,
        clip: Some(rect),
        reach: Some(list),
        rounded_clip: None,
        snap_anchor: None,
        effect: RenderEffect::runtime_shader(shader),
        z_index: 0,
    };
    let planned =
        plan_backdrop(&layer, 0, 2.0, target, Some(target)).expect("the backdrop is on the target");
    assert_eq!(
        planned.capture_rect,
        DeviceRect::from_logical(
            Rect {
                x: 20.0,
                y: 96.0,
                width: 160.0,
                height: 82.0,
            },
            2.0,
        ),
        "the capture stops at the list's top and sides and reads the padding below, \
         where the list goes on"
    );
}

#[test]
fn a_backdrop_keeps_its_capture_and_records_the_part_of_it_inside_the_effects_output_support() {
    let mut shader = RuntimeShader::new("fn glass_fs() {}");
    shader.set_input_padding(2.0);
    shader.set_output_padding(3.0);
    let rect = Rect {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
    };
    let target = DeviceRect {
        x: 0.0,
        y: 0.0,
        width: 400.0,
        height: 400.0,
    };
    let plan = |shader: RuntimeShader| {
        let layer = BackdropLayer {
            node_id: None,
            alpha: 1.0,
            rect,
            clip: None,
            reach: None,
            rounded_clip: None,
            snap_anchor: None,
            effect: RenderEffect::runtime_shader(shader),
            z_index: 0,
        };
        let planned = plan_backdrop(&layer, 0, 2.0, target, Some(target))
            .expect("the backdrop is on the target");
        (planned.visible, planned.capture_rect, planned.support)
    };
    let (whole_visible, whole_capture, whole_support) = plan(shader.clone());
    assert_eq!(whole_visible, DeviceRect::from_logical(rect, 2.0));
    assert_eq!(whole_capture, whole_visible.expand(10.0).snap_out());
    assert_eq!(whole_support, None);

    shader.set_output_support(Some(Rect {
        x: 30.0,
        y: 5.0,
        width: 20.0,
        height: 10.0,
    }));
    let (visible, capture_rect, support) = plan(shader);
    assert_eq!(visible, whole_visible);
    assert_eq!(capture_rect, whole_capture);
    assert_eq!(
        support,
        Some(DeviceRect::from_logical(
            Rect {
                x: 40.0,
                y: 25.0,
                width: 20.0,
                height: 10.0,
            },
            2.0,
        ))
    );
}

#[test]
fn a_gate_admits_a_key_that_held_for_more_than_its_patience() {
    let key = gate_key(1);
    let mut gate = AdmissionGate::copied(key);
    assert!(!gate.admits(), "a key seen once is only remembered");
    gate.observe(key);
    assert!(gate.admits(), "the second frame of a key admits it");
    gate.admitted();
    gate.hit(key);
    assert_eq!(patience(&gate), 1);
    assert!(gate.end_frame(), "a gate seen this frame stays");
    assert!(!gate.end_frame(), "a gate not seen since goes");
}

#[test]
fn a_cached_key_between_misses_breaks_the_other_keys_consecutive_run() {
    let first = gate_key(1);
    let other = gate_key(2);
    let mut gate = AdmissionGate::copied(first);
    gate.observe(first);
    assert!(gate.admits());
    gate.admitted();
    gate.observe(other);
    assert!(!gate.admits());
    gate.observe(other);
    assert!(!gate.admits());
    gate.hit(first);
    gate.observe(other);
    assert!(
        !gate.admits(),
        "the other key has held for only one frame since the cache hit"
    );
}
/// One frame of a node's surface: read from the cache while its kept copy
/// is current, otherwise offered to the gate.
fn gate_frame(gate: &mut AdmissionGate, key: LayerRasterCacheKey) -> bool {
    if gate.admitted && gate.key == key {
        gate.hit(key);
        return false;
    }
    gate.observe(key);
    if gate.admits() {
        gate.admitted();
        return true;
    }
    false
}

fn admissions_over(gate: &mut AdmissionGate, holds: impl IntoIterator<Item = u32>) -> u32 {
    let mut admissions = 0;
    for (step, hold) in holds.into_iter().enumerate() {
        for _ in 0..hold {
            admissions += u32::from(gate_frame(gate, gate_key(step as u64 + 1)));
        }
    }
    admissions
}

#[test]
fn a_gate_waits_twice_as_long_after_an_admission_nothing_read_back() {
    let mut gate = AdmissionGate::copied(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(2, 40)),
        1,
        "a key that never holds a third frame is admitted once"
    );
    assert_eq!(patience(&gate), 2);
    let mut gate = AdmissionGate::copied(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(4, 12)),
        12,
        "a key that holds a fourth frame is read back twice per admission"
    );
    assert_eq!(
        patience(&gate),
        1,
        "an admission read back twice does not double the patience"
    );
}

/// A grid resizing a pixel at a time holds each cell's size two or three
/// frames. A copy kept on its second frame is read once on its third and
/// replaced: it costs a copy and a texture for one read, so the gate waits
/// longer instead of keeping the next one.
#[test]
fn a_copied_gate_stops_keeping_what_is_read_only_once() {
    let mut gate = AdmissionGate::copied(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(3, 40)),
        2,
        "a key held three frames is kept until reading it once proves too little"
    );
    assert_eq!(patience(&gate), 4);
    assert_eq!(
        admissions_over(&mut gate, [40]),
        1,
        "content that holds still is still kept"
    );
    assert_eq!(
        patience(&gate),
        2,
        "a copy read back twice halves the wait, so the churn after a pause stays out"
    );
}

#[test]
fn a_pinned_gate_admits_every_uncached_frame_and_counts_the_hold() {
    let mut gate = AdmissionGate::pinned(gate_key(0));
    assert!(gate.admits(), "a pin costs no pass, so first sight admits");
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(2, 40)),
        40,
        "every two-frame hold is pinned on its first frame and replayed on its second"
    );
    assert_eq!(
        gate.run, 2,
        "the replay counted as a second frame of the hold"
    );
    let mut gate = AdmissionGate::pinned(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(1, 40)),
        40,
        "an unread pin costs nothing to repeat, so a key changing every frame is pinned \
         every frame"
    );
    assert_eq!(gate.run, 1);
    for _ in 0..4 {
        gate.observe(gate_key(99));
    }
    assert_eq!(
        gate.run, 4,
        "a held key's run is what the admission budget ranks by"
    );
}

#[test]
fn a_pin_lives_exactly_as_long_as_its_key_and_a_copy_only_dies_unread() {
    let mut gate = AdmissionGate::pinned(gate_key(1));
    assert_eq!(
        gate.dead_entry(),
        None,
        "nothing admitted, nothing to hand back"
    );
    gate.admitted();
    assert_eq!(gate.dead_entry(), Some(gate_key(1)));
    assert_eq!(
        gate.observe(gate_key(1)),
        None,
        "the same key holds the pin"
    );
    assert_eq!(
        gate.observe(gate_key(2)),
        Some(gate_key(1)),
        "a pin nothing read back dies with its key"
    );
    assert_eq!(gate.dead_entry(), None);
    gate.admitted();
    gate.hit(gate_key(2));
    assert_eq!(
        gate.observe(gate_key(3)),
        Some(gate_key(2)),
        "a pin that was read back dies with its key too: a re-pin costs nothing"
    );
    let mut gate = AdmissionGate::copied(gate_key(1));
    gate.observe(gate_key(1));
    gate.admitted();
    gate.hit(gate_key(1));
    assert_eq!(
        gate.observe(gate_key(2)),
        None,
        "a copy that was read back stays for the cache to keep or evict"
    );
    gate.observe(gate_key(2));
    gate.admitted();
    assert_eq!(
        gate.observe(gate_key(3)),
        Some(gate_key(2)),
        "a copy nothing read back is handed back"
    );
}

fn patience(gate: &AdmissionGate) -> u32 {
    match gate.cost {
        AdmissionCost::Pin => 0,
        AdmissionCost::Copy { patience, .. } => patience,
    }
}

#[test]
fn a_rendered_gate_admits_a_first_sight_and_waits_after_an_unread_admission() {
    let mut gate = AdmissionGate::rendered(gate_key(0));
    assert!(
        gate.admits(),
        "a surface seen for the first time is kept, as still content reads it back next frame"
    );
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(1, 40)),
        1,
        "a surface that changes every frame is kept once, then drawn without being stored"
    );
    assert_eq!(patience(&gate), 1);
    assert_eq!(
        admissions_over(&mut gate, [4]),
        1,
        "a surface that settles is kept on its second frame"
    );
    assert_eq!(
        patience(&gate),
        0,
        "a kept surface read back twice restores first-sight admission"
    );
    assert!(gate_frame(&mut gate, gate_key(100)));
}

#[test]
fn a_gate_never_waits_longer_than_the_cap() {
    let mut gate = AdmissionGate::copied(gate_key(0));
    let admissions = admissions_over(&mut gate, [2, 3, 5, 9, 17, 17, 17]);
    assert_eq!(
        admissions, 7,
        "each hold one frame past the patience is admitted on its last frame"
    );
    assert_eq!(patience(&gate), MAX_ADMISSION_PATIENCE);
}

#[test]
fn an_in_place_gate_waits_longer_after_the_surface_it_kept_went_unread() {
    let mut gate = AdmissionGate::drawn_in_place(gate_key(0));
    assert_eq!(
        admissions_over(&mut gate, std::iter::repeat_n(IN_PLACE_PATIENCE + 1, 6)),
        1,
        "a relayout pausing one frame past the patience at every turn of its motion keeps \
         its surface once, and not again after nothing read it"
    );
    assert!(patience(&gate) > IN_PLACE_PATIENCE);
    // Held one frame past the patience to be kept, then read twice.
    let settles = patience(&gate) + 3;
    assert_eq!(
        admissions_over(&mut gate, [settles]),
        1,
        "content that settles is still kept"
    );
    assert_eq!(
        patience(&gate),
        IN_PLACE_PATIENCE,
        "a kept surface read back twice restores the patience"
    );
}

#[test]
fn an_in_place_gate_never_waits_longer_than_its_cap() {
    let mut gate = AdmissionGate::drawn_in_place(gate_key(0));
    let holds = [17, 33, 65, 129, 257, 257, 257];
    assert_eq!(
        admissions_over(&mut gate, holds),
        7,
        "each hold one frame past the patience is admitted on its last frame"
    );
    assert_eq!(patience(&gate), IN_PLACE_MAX_PATIENCE);
}

fn gate_key(content: u64) -> LayerRasterCacheKey {
    LayerRasterCacheKey::backdrop_effect(
        None,
        content,
        0,
        Rect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
        (1, 1),
        RasterScale::from_scale(1.0),
    )
}

use super::*;
use crate::scene::DrawOpKind;

const CHILD_BOUNDS: Rect = Rect {
    x: 0.0,
    y: 0.0,
    width: 40.0,
    height: 40.0,
};

fn child_layer(transform: ProjectiveTransform, content: LayerScene) -> ChildLayer {
    ChildLayer {
        z_index: 0,
        node_id: None,
        local_bounds: CHILD_BOUNDS,
        transform,
        clip: None,
        rounded_clip: None,
        alpha: 1.0,
        blend_mode: BlendMode::SrcOver,
        effect: None,
        backdrop: None,
        backdrop_alpha: 1.0,
        snap_anchor: None,
        surface_scale: 1.0,
        content_hash: 0,
        cache_policy: CachePolicy::None,
        in_place: false,
        content,
    }
}

fn scene_of(ops: &[usize], children: Vec<ChildLayer>) -> LayerScene {
    let mut scene = CompositorScene::new();
    scene.draw_ops = ops.iter().map(|&z| op(z)).collect();
    LayerScene::new(scene, children)
}

fn rounded_child(transform: ProjectiveTransform, surface_scale: f32) -> ChildLayer {
    ChildLayer {
        rounded_clip: Some(LayerRoundedClip {
            rect: CHILD_BOUNDS,
            radii: [20.0; 4],
        }),
        surface_scale,
        ..child_layer(transform, scene_of(&[], Vec::new()))
    }
}

fn in_place_child(
    z_index: usize,
    transform: ProjectiveTransform,
    ops: &[usize],
    children: Vec<ChildLayer>,
) -> ChildLayer {
    ChildLayer {
        z_index,
        in_place: true,
        ..child_layer(transform, scene_of(ops, children))
    }
}

fn page_target() -> InPlaceTarget {
    InPlaceTarget {
        scale: 2.0,
        rect: DeviceRect {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        },
        size: (100, 100),
        offset: [0.0, 0.0],
    }
}

/// Each part of a flush: a page part by its op range, a part drawn in place
/// by its ops' depths, its translation and its scissor.
#[derive(Debug, PartialEq)]
enum Part {
    Page(Range<usize>),
    InPlace(Vec<usize>, [f32; 2], Option<(u32, u32, u32, u32)>),
}

fn described(parts: &[FlushPart<'_>]) -> Vec<Part> {
    parts
        .iter()
        .map(|part| match part {
            FlushPart::Page { ops, .. } => Part::Page(ops.clone()),
            FlushPart::InPlace {
                ops,
                transform,
                scissor,
                ..
            } => Part::InPlace(
                ops.iter().map(|op| op.z_index).collect(),
                transform.uniform_parts().1,
                *scissor,
            ),
        })
        .collect()
}

#[test]
fn a_flush_draws_its_children_in_place_between_its_ops_at_their_z() {
    let layer = scene_of(
        &[0, 2, 5],
        vec![
            in_place_child(
                1,
                ProjectiveTransform::translation(10.0, 0.0),
                &[0, 1],
                Vec::new(),
            ),
            in_place_child(
                4,
                ProjectiveTransform::translation(0.0, 3.0),
                &[0],
                Vec::new(),
            ),
        ],
    );
    let (parts, complete) = flush_parts(
        &layer,
        &layer.scene.draw_ops,
        &[],
        &[0, 1],
        &page_target(),
        0,
    );
    assert!(complete);
    assert_eq!(
        described(&parts),
        [
            Part::Page(0..1),
            Part::InPlace(vec![0, 1], [20.0, 0.0], None),
            Part::Page(1..2),
            Part::InPlace(vec![0], [0.0, 6.0], None),
            Part::Page(2..3),
        ]
    );
}

#[test]
fn a_child_drawn_in_place_draws_its_children_at_their_z_under_composed_transforms() {
    let grandchild = in_place_child(
        2,
        ProjectiveTransform::translation(0.0, 7.0),
        &[0],
        Vec::new(),
    );
    let child = in_place_child(
        1,
        ProjectiveTransform::translation(5.0, 0.0),
        &[0, 3],
        vec![grandchild],
    );
    let mut parts = Vec::new();
    assert!(push_in_place(
        &mut parts,
        &child,
        SegmentTransform::IDENTITY,
        2.0,
        Some((1, 2, 3, 4)),
        0,
    ));
    assert_eq!(
        described(&parts),
        [
            Part::InPlace(vec![0], [10.0, 0.0], Some((1, 2, 3, 4))),
            Part::InPlace(vec![0], [10.0, 14.0], Some((1, 2, 3, 4))),
            Part::InPlace(vec![3], [10.0, 0.0], Some((1, 2, 3, 4))),
        ]
    );
}

#[test]
fn a_clipped_child_draws_in_place_under_its_clip_and_not_at_all_when_clipped_away() {
    let clipped = |clip: Rect| ChildLayer {
        clip: Some(clip),
        ..in_place_child(1, ProjectiveTransform::identity(), &[0], Vec::new())
    };
    let layer = scene_of(
        &[],
        vec![
            clipped(Rect {
                x: 10.0,
                y: 10.0,
                width: 20.0,
                height: 20.0,
            }),
            clipped(Rect {
                x: 80.0,
                y: 0.0,
                width: 10.0,
                height: 10.0,
            }),
        ],
    );
    let (parts, _) = flush_parts(&layer, &[], &[], &[0, 1], &page_target(), 0);
    assert_eq!(
        described(&parts),
        [Part::InPlace(vec![0], [0.0, 0.0], Some((20, 20, 40, 40)))]
    );
}

#[test]
fn layers_drawn_in_place_past_the_resolve_depth_are_left_out() {
    let mut nested = in_place_child(1, ProjectiveTransform::identity(), &[0], Vec::new());
    for _ in 0..MAX_RESOLVE_DEPTH + 1 {
        nested = in_place_child(1, ProjectiveTransform::identity(), &[0], vec![nested]);
    }
    let mut parts = Vec::new();
    assert!(!push_in_place(
        &mut parts,
        &nested,
        SegmentTransform::IDENTITY,
        1.0,
        None,
        0,
    ));
    assert_eq!(parts.len(), MAX_RESOLVE_DEPTH);
}

#[test]
fn a_child_drawn_in_place_turns_and_moves_by_its_transform_at_the_page_scale() {
    let quarter_turn = ProjectiveTransform::from_rect_to_quad(
        CHILD_BOUNDS,
        [[40.0, 0.0], [40.0, 40.0], [0.0, 0.0], [0.0, 40.0]],
    )
    .then(ProjectiveTransform::translation(3.0, 4.0));
    let child = child_layer(quarter_turn, scene_of(&[], Vec::new()));
    let (transform, content_scale) = in_place_transform(&child, 2.0).expect("a turn is invertible");
    assert_eq!(content_scale, 2.0, "a turn draws at the page's scale");
    let (linear, translation, inverse) = transform.uniform_parts();
    let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5);
    assert!(near(linear, [0.0, -1.0, 1.0, 0.0]), "{linear:?}");
    assert!(near(inverse, [0.0, 1.0, -1.0, 0.0]), "{inverse:?}");
    assert!(
        (translation[0] - 86.0).abs() < 1e-4 && (translation[1] - 8.0).abs() < 1e-4,
        "{translation:?}"
    );
}

#[test]
fn a_scaled_child_drawn_in_place_draws_at_its_parent_s_scale_times_its_own() {
    let scaled = |z_index, scale: f32, children| ChildLayer {
        surface_scale: scale,
        ..in_place_child(
            z_index,
            ProjectiveTransform::uniform_scale(scale)
                .then(ProjectiveTransform::translation(5.0, 0.0)),
            &[0],
            children,
        )
    };
    let child = scaled(1, 0.5, vec![scaled(2, 0.8, Vec::new())]);
    let mut parts = Vec::new();
    assert!(push_in_place(
        &mut parts,
        &child,
        SegmentTransform::IDENTITY,
        2.0,
        None,
        0,
    ));
    let drawn: Vec<(f32, [f32; 4], [f32; 2])> = parts
        .iter()
        .filter_map(|part| match part {
            FlushPart::InPlace {
                transform, scale, ..
            } => {
                let (linear, translation, _) = transform.uniform_parts();
                Some((*scale, linear, translation))
            }
            FlushPart::Page { .. } => None,
        })
        .collect();
    assert_eq!(
        drawn,
        [
            (1.0, [1.0, 0.0, 0.0, 1.0], [10.0, 0.0]),
            (0.8, [1.0, 0.0, 0.0, 1.0], [15.0, 0.0]),
        ],
        "each draws at its parent's content scale times its own, moved by its offset at its \
         parent's content scale"
    );
}

#[test]
fn a_scaled_child_masks_its_rounded_clip_scaled_with_it() {
    let scaled = rounded_child(
        ProjectiveTransform::uniform_scale(1.5)
            .then(ProjectiveTransform::translation(100.0, 200.0)),
        1.5,
    );
    let mask = grid_rounded_mask(&scaled, Point::new(0.5, 0.0), 2.0)
        .expect("a uniform scale keeps the clip axis-aligned");
    assert_eq!(mask.rect, [201.0, 400.0, 120.0, 120.0]);
    assert_eq!(mask.radii, [60.0; 4]);

    let translated = rounded_child(ProjectiveTransform::translation(10.0, 20.0), 1.0);
    let mask = grid_rounded_mask(&translated, Point::default(), 1.0)
        .expect("a translation keeps the clip axis-aligned");
    assert_eq!(mask.rect, [10.0, 20.0, 40.0, 40.0]);
    assert_eq!(mask.radii, [20.0; 4]);
}

#[test]
fn a_rotated_child_has_no_axis_aligned_rounded_mask() {
    let rotated = rounded_child(
        ProjectiveTransform::from_rect_to_quad(
            Rect {
                x: 0.0,
                y: 0.0,
                width: 40.0,
                height: 40.0,
            },
            [[20.0, 0.0], [40.0, 20.0], [20.0, 40.0], [0.0, 20.0]],
        ),
        1.0,
    );
    assert!(grid_rounded_mask(&rotated, Point::default(), 1.0).is_none());
}

fn op(z_index: usize) -> DrawOp {
    DrawOp {
        z_index,
        kind: DrawOpKind::Run(0),
    }
}

#[test]
fn pending_draw_ops_keep_deferred_content_and_respect_capture_depth() {
    let scene = [op(1), op(3), op(5), op(7)];
    let deferred = [op(0), op(2), op(4), op(6)];
    let depths = |ops: &[DrawOp]| ops.iter().map(|op| op.z_index).collect::<Vec<_>>();
    let only_deferred = pending_draw_ops(&scene, 7, 6, &[], &deferred);
    assert_eq!(depths(&only_deferred), [0, 2, 4]);
    assert!(matches!(only_deferred, Cow::Borrowed(_)));
    let only_scene = pending_draw_ops(&scene, 3, 6, &[], &[]);
    assert_eq!(depths(&only_scene), [3, 5]);
    assert!(matches!(only_scene, Cow::Borrowed(_)));
    let mixed = pending_draw_ops(&scene, 3, 6, &[(5, 6)], &deferred);
    assert_eq!(depths(&mixed), [0, 2, 3, 4]);
    let excluded_scene = pending_draw_ops(&scene, 3, 6, &[(3, 6)], &deferred);
    assert_eq!(depths(&excluded_scene), [0, 2, 4]);
    assert!(matches!(excluded_scene, Cow::Borrowed(_)));
    assert!(pending_draw_ops(&scene, 0, 0, &[], &deferred).is_empty());
    assert_eq!(
        depths(&pending_draw_ops(&scene, 0, 3, &[(0, 3)], &[])),
        [0usize; 0]
    );
}

#[test]
fn an_inverted_op_range_is_empty_even_when_an_op_sits_at_its_end() {
    let ops = [op(1), op(3), op(3), op(5)];
    assert!(filtered_ops_in_range(&ops, 4, 3, &[]).is_empty());
    assert!(filtered_ops_in_range(&ops, 3, 3, &[]).is_empty());
    assert_eq!(
        filtered_ops_in_range(&ops, 3, 4, &[])
            .iter()
            .map(|op| op.z_index)
            .collect::<Vec<_>>(),
        [3, 3]
    );
}

#[test]
fn deferred_coverage_blocks_later_draws_across_its_entire_extent() {
    let rect = |x, width| DeviceRect {
        x,
        y: 0.0,
        width,
        height: 10.0,
    };
    let mut blockers = vec![Blocker {
        z: 2,
        rect: rect(4.25, 1.5),
    }];
    assert!(!blocked_coverage(&mut blockers, 1, rect(0.0, 10.0)));
    assert!(blocked_coverage(&mut blockers, 3, rect(0.0, 10.0)));
    assert!(blocked_coverage(&mut blockers, 4, rect(8.0, 1.0)));
    assert!(!blocked_coverage(&mut blockers, 4, rect(12.0, 1.0)));
}
