use crate::{Brush, Color, Column, ColumnSpec, CornerRadii, Modifier, Row, RowSpec, Text};
use compose_core::{
    location_key, Composition, MemoryApplier, MutableState, Node, NodeError,
    __launched_effect_async_impl as launched_effect_async_impl,
};
use compose_macros::composable;

#[derive(Clone, Copy, Debug)]
struct AnimationState {
    progress: f32,
    direction: f32,
}

impl Default for AnimationState {
    fn default() -> Self {
        Self {
            progress: 0.0,
            direction: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct FrameStats {
    frames: u32,
    last_frame_ms: f32,
}

impl Default for FrameStats {
    fn default() -> Self {
        Self {
            frames: 0,
            last_frame_ms: 0.0,
        }
    }
}

#[derive(Default)]
struct DummyNode;

impl Node for DummyNode {}

#[composable]
fn async_runtime_demo(animation: MutableState<AnimationState>, stats: MutableState<FrameStats>) {
    {
        let animation_state = animation.clone();
        let stats_state = stats.clone();
        launched_effect_async_impl(
            location_key(file!(), line!(), column!()),
            (),
            move |scope| {
                let animation = animation_state.clone();
                let stats = stats_state.clone();
                Box::pin(async move {
                    let clock = scope.runtime().frame_clock();
                    let mut last_time: Option<u64> = None;
                    while scope.is_active() {
                        let nanos = clock.next_frame().await;
                        if !scope.is_active() {
                            break;
                        }
                        if let Some(previous) = last_time {
                            let mut delta_nanos = nanos.saturating_sub(previous);
                            if delta_nanos == 0 {
                                delta_nanos = 16_666_667;
                            }
                            let dt_ms = delta_nanos as f32 / 1_000_000.0;
                            stats.update(|state| {
                                state.frames = state.frames.wrapping_add(1);
                                state.last_frame_ms = dt_ms;
                            });
                            animation.update(|anim| {
                                let next = anim.progress + 0.1 * anim.direction * (dt_ms / 600.0);
                                if next >= 1.0 {
                                    anim.progress = 1.0;
                                    anim.direction = -1.0;
                                } else if next <= 0.0 {
                                    anim.progress = 0.0;
                                    anim.direction = 1.0;
                                } else {
                                    anim.progress = next;
                                }
                            });
                        }
                        last_time = Some(nanos);
                    }
                })
            },
        );
    }

    Column(Modifier::empty(), ColumnSpec::default(), {
        let animation_snapshot = animation.value();
        let stats_snapshot = stats.value();
        let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);
        let fill_width = 320.0 * progress_value;
        move || {
            let progress_width = fill_width;
            compose_core::with_current_composer(|composer| {
                composer.with_group(location_key(file!(), line!(), column!()), |composer| {
                    if progress_width > 0.0 {
                        composer.with_group(
                            location_key(file!(), line!(), column!()),
                            |composer| {
                                composer.emit_node(|| DummyNode::default());
                            },
                        );
                    }
                });
            });

            Text(
                format!(
                    "Frames advanced: {} (last frame {:.2} ms, direction: {})",
                    stats_snapshot.frames,
                    stats_snapshot.last_frame_ms,
                    if animation_snapshot.direction >= 0.0 {
                        "forward"
                    } else {
                        "reverse"
                    }
                ),
                Modifier::empty(),
            );
        }
    });
}

fn drain_all(composition: &mut Composition<MemoryApplier>) -> Result<(), NodeError> {
    loop {
        if !composition.process_invalid_scopes()? {
            break;
        }
    }
    Ok(())
}

#[test]
fn async_runtime_freezes_without_conditional_key() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());

    let mut render = {
        let animation = animation.clone();
        let stats = stats.clone();
        move || async_runtime_demo(animation.clone(), stats.clone())
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    let mut last_direction = animation.value().direction;
    let mut frames_before = None;
    let mut frames_after = None;
    let mut forward_flip = false;
    let mut time = 0u64;

    for _ in 0..800 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");

        let anim = animation.value();
        if last_direction < 0.0 && anim.direction > 0.0 {
            forward_flip = true;
            frames_before = Some(stats.value().frames);

            for _ in 0..8 {
                time += 16_666_667;
                runtime.drain_frame_callbacks(time);
                drain_all(&mut composition).expect("post-flip drain");
            }

            frames_after = Some(stats.value().frames);
            break;
        }

        last_direction = anim.direction;
    }

    assert!(forward_flip, "no backward->forward transition observed");
    let before = frames_before.expect("frames before flip recorded");
    let after = frames_after.expect("frames after flip recorded");

    assert!(
        after > before,
        "frames should increase after forward flip without manual with_key workaround (before {before}, after {after})"
    );
}

#[composable]
fn progress_demo(animation: MutableState<AnimationState>, stats: MutableState<FrameStats>) {
    Column(Modifier::empty().padding(12.0), ColumnSpec::default(), {
        move || {
            let animation_snapshot = animation.value();
            let stats_snapshot = stats.value();
            let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);
            let fill_width = 320.0 * progress_value;

            Row(
                Modifier::empty().fill_max_width()
                    .then(Modifier::empty().height(26.0))
                    .then(Modifier::empty().rounded_corners(13.0))
                    .then(Modifier::empty().draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.12, 0.16, 0.30, 1.0)),
                            CornerRadii::uniform(13.0),
                        );
                    })),
                RowSpec::default(),
                {
                    let progress_width = fill_width;
                    move || {
                        if progress_width > 0.0 {
                            Row(
                                Modifier::empty().width(progress_width.min(360.0))
                                    .then(Modifier::empty().height(26.0))
                                    .then(Modifier::empty().rounded_corners(13.0))
                                    .then(Modifier::empty().draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::linear_gradient(vec![
                                                Color(0.25, 0.55, 0.95, 1.0),
                                                Color(0.15, 0.35, 0.80, 1.0),
                                            ]),
                                            CornerRadii::uniform(13.0),
                                        );
                                    })),
                                RowSpec::default(),
                                || {},
                            );
                        }
                    }
                },
            );

            Text(
                format!(
                    "Frames advanced: {} (dir: {})",
                    stats_snapshot.frames,
                    if animation_snapshot.direction >= 0.0 {
                        "forward"
                    } else {
                        "reverse"
                    }
                ),
                Modifier::empty().padding(8.0),
            );
        }
    });
}

#[test]
fn stats_state_invalidates_after_direction_flip() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());

    animation.update(|anim| anim.progress = 0.5);

    let mut render = {
        let animation = animation.clone();
        let stats = stats.clone();
        move || progress_demo(animation.clone(), stats.clone())
    };

    let key = location_key(file!(), line!(), column!());
    composition
        .render(key, &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");
    assert!(
        !composition.should_render(),
        "initial render should leave composition idle"
    );

    animation.update(|anim| {
        anim.progress = 1.0;
        anim.direction = -1.0;
    });
    composition
        .render(key, &mut render)
        .expect("render descending");
    drain_all(&mut composition).expect("drain descending");

    animation.update(|anim| {
        anim.progress = 0.0;
        anim.direction = 1.0;
    });
    composition
        .render(key, &mut render)
        .expect("render ascending");
    drain_all(&mut composition).expect("drain ascending");

    stats.update(|state| state.frames = state.frames.wrapping_add(1));
    assert!(
        composition.should_render(),
        "stats update should still schedule render without manual with_key workaround"
    );
}
