use super::*;
use cranpose_core::{location_key, Composition, MemoryApplier, MutableState, NodeError};

mod conditional_text_test;

#[composable]
fn async_runtime_test_content(
    animation: MutableState<AnimationState>,
    stats: MutableState<FrameStats>,
    is_running: MutableState<bool>,
    reset_signal: MutableState<u64>,
) {
    {
        let animation_state = animation;
        let stats_state = stats;
        let running_state = is_running;
        let reset_state = reset_signal;
        LaunchedEffectAsync!((), move |scope| {
            let animation = animation_state;
            let stats = stats_state;
            let running = running_state;
            let reset = reset_state;
            Box::pin(async move {
                let clock = scope.runtime().frame_clock();
                let mut last_time: Option<u64> = None;
                let mut last_reset = reset.get();
                animation.set(AnimationState::default());
                stats.set(FrameStats::default());
                while scope.is_active() {
                    let nanos = clock.next_frame().await;
                    if !scope.is_active() {
                        break;
                    }
                    let current_reset = reset.get();
                    if current_reset != last_reset {
                        last_reset = current_reset;
                        animation.set(AnimationState::default());
                        stats.set(FrameStats::default());
                        last_time = None;
                        continue;
                    }
                    let running_now = running.get();
                    if !running_now {
                        last_time = Some(nanos);
                        continue;
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
        });
    }

    Column(
        Modifier::empty()
            .padding(32.0)
            .then(Modifier::empty().background(Color(0.10, 0.14, 0.28, 1.0)))
            .then(Modifier::empty().rounded_corners(24.0))
            .then(Modifier::empty().padding(20.0)),
        ColumnSpec::default(),
        {
            move || {
                let animation_snapshot = animation.get();
                let stats_snapshot = stats.get();
                let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);

                Column(
                    Modifier::empty()
                        .fill_max_width()
                        .then(Modifier::empty().padding(8.0))
                        .then(Modifier::empty().background(Color(0.06, 0.10, 0.22, 0.8)))
                        .then(Modifier::empty().rounded_corners(18.0))
                        .then(Modifier::empty().padding(12.0)),
                    ColumnSpec::default(),
                    {
                        move || {
                            Row(
                                Modifier::empty()
                                    .fill_max_width()
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
                                    let progress_fraction = progress_value;
                                    move || {
                                        if progress_fraction > 0.0 {
                                            Row(
                                                Modifier::empty()
                                                    .fill_max_width_fraction(progress_fraction)
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
                        }
                    },
                );

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
                    Modifier::empty()
                        .padding(8.0)
                        .then(Modifier::empty().background(Color(0.18, 0.22, 0.36, 0.6)))
                        .then(Modifier::empty().rounded_corners(14.0)),
                );
            }
        },
    );
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
    let is_running = MutableState::with_runtime(true, runtime.clone());
    let reset_signal = MutableState::with_runtime(0u64, runtime.clone());

    let mut render = move || async_runtime_test_content(animation, stats, is_running, reset_signal);

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    let mut last_direction = animation.value().direction;
    let mut forward_flip = false;
    let mut frames_before = None;
    let mut frames_after = None;
    let mut time = 0u64;

    for _ in 0..800 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain after frame");

        let anim = animation.value();
        if last_direction < 0.0 && anim.direction > 0.0 {
            forward_flip = true;
            frames_before = Some(stats.value().frames);

            for _ in 0..16 {
                time += 16_666_667;
                runtime.drain_frame_callbacks(time);
                drain_all(&mut composition).expect("drain after flip");
            }

            frames_after = Some(stats.value().frames);
            break;
        }

        last_direction = anim.direction;
    }

    assert!(forward_flip, "did not observe backward->forward transition");
    let before = frames_before.expect("frames before flip");
    let after = frames_after.expect("frames after flip");

    assert!(
        after > before,
        "frames should continue increasing after forward flip without manual with_key workaround (before {before}, after {after})"
    );
}

#[test]
fn async_runtime_tab_content_renders_static_states() {
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    let animation_state = MutableState::with_runtime(
        AnimationState {
            progress: 1.0,
            direction: 1.0,
        },
        runtime.clone(),
    );
    let stats_state = MutableState::with_runtime(
        FrameStats {
            frames: 42,
            last_frame_ms: 16.0,
        },
        runtime.clone(),
    );
    let is_running_state = MutableState::with_runtime(false, runtime.clone());
    let reset_signal_state = MutableState::with_runtime(0u64, runtime);

    let animation_for_render = animation_state.clone();
    let stats_for_render = stats_state.clone();
    let is_running_for_render = is_running_state.clone();
    let reset_for_render = reset_signal_state.clone();

    let mut render = move || {
        AsyncRuntimeTabContent(
            animation_for_render,
            stats_for_render,
            is_running_for_render,
            reset_for_render,
        )
    };

    composition
        .render(location_key(file!(), line!(), column!()), &mut render)
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    animation_state.update(|state| state.progress = 0.0);
    drain_all(&mut composition).expect("drain after progress 0");
    animation_state.update(|state| state.progress = 1.0);
    drain_all(&mut composition).expect("drain after progress 1");
}
