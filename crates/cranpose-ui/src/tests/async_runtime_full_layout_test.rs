use cranpose_core::{
    __launched_effect_async_impl as launched_effect_async_impl, Composition, MemoryApplier,
    MutableState, Node, TaskSite, location_key,
};
use cranpose_macros::composable;

use crate::{
    Brush, Button, ButtonSpec, Color, Column, ColumnSpec, CornerRadii, Modifier, Row, RowSpec,
    Size, Spacer, Text, TextStyle,
};

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
#[allow(dead_code)]
struct DummyNode;

impl Node for DummyNode {}

#[composable]
fn async_runtime_full_layout(
    is_running: MutableState<bool>,
    animation: MutableState<AnimationState>,
    stats: MutableState<FrameStats>,
) {
    {
        let animation_state = animation;
        let stats_state = stats;
        let running_state = is_running;
        launched_effect_async_impl(
            location_key(file!(), line!(), column!()),
            TaskSite::new(file!(), line!()),
            (),
            move |scope| {
                let animation = animation_state;
                let stats = stats_state;
                let running = running_state;
                Box::pin(async move {
                    let clock = scope.runtime().frame_clock();
                    let mut last_time: Option<u64> = None;

                    while scope.is_active() {
                        let nanos = clock.next_frame().await;
                        if !scope.is_active() {
                            break;
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
            },
        );
    }

    Column(
        Modifier::empty().padding(32.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Async Runtime Demo",
                Modifier::empty().padding(12.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            let animation_snapshot = animation.get();
            let stats_snapshot = stats.get();
            let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);

            Column(
                Modifier::empty().padding(8.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        format!("Progress: {:>3}%", (progress_value * 100.0) as i32),
                        Modifier::empty().padding(6.0),
                        TextStyle::default(),
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 8.0,
                    });

                    Row(
                        Modifier::empty()
                            .height(26.0)
                            .then(Modifier::empty().rounded_corners(13.0)),
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
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Text(
                format!(
                    "Frames advanced: {} (direction: {})",
                    stats_snapshot.frames,
                    if animation_snapshot.direction >= 0.0 {
                        "forward"
                    } else {
                        "reverse"
                    }
                ),
                Modifier::empty().padding(8.0),
                TextStyle::default(),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            {
                let is_running_for_button = is_running;
                Row(
                    Modifier::empty().padding(4.0),
                    RowSpec::default(),
                    move || {
                        let running = is_running_for_button.get();

                        let button_label = if running {
                            "Pause animation"
                        } else {
                            "Resume animation"
                        };
                        Button(
                            Modifier::empty().padding(12.0),
                            ButtonSpec::default(),
                            {
                                let toggle_state = is_running_for_button;
                                move || toggle_state.set(!toggle_state.get())
                            },
                            move || {
                                Text(
                                    button_label,
                                    Modifier::empty().padding(6.0),
                                    TextStyle::default(),
                                );
                            },
                        );
                    },
                );
            }
        },
    );
}

fn drain_all<A: cranpose_core::Applier + 'static>(
    composition: &mut Composition<A>,
) -> Result<(), cranpose_core::NodeError> {
    let mut iterations = 0;
    loop {
        if !composition.process_invalid_scopes()? {
            if iterations > 100 {
                println!("drain_all: Took {} iterations to stabilize", iterations);
            }
            return Ok(());
        }
        iterations += 1;
        if iterations > 1000 {
            eprintln!(
                "drain_all: Exceeded 1000 iterations, giving up. This indicates an infinite recomposition loop. {iterations}"
            );
            return Err(cranpose_core::NodeError::MissingContext {
                id: 0,
                reason: "drain_all: Exceeded 1000 iterations",
            });
        }
    }
}

#[test]
fn async_runtime_full_layout_freezes_after_forward_flip() {
    let _app_context = crate::render_state::app_context_test_scope();
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    let is_running = MutableState::with_runtime(true, runtime.clone());
    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());

    composition
        .render(location_key(file!(), line!(), column!()), &mut || {
            async_runtime_full_layout(is_running, animation, stats);
        })
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    println!("Starting animation loop, looking for forward flip...");

    let mut time = 0u64;
    let mut last_direction = animation.get().direction;
    let mut forward_flip_frame: Option<u32> = None;

    for frame_num in 0..2000 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain loop");
        {
            use crate::layout::LayoutEngine;
            let root = composition.root().expect("composition root");
            let compute_result = {
                let mut applier = composition.applier_mut();
                applier.compute_layout(
                    root,
                    crate::modifier::Size {
                        width: 1280.0,
                        height: 720.0,
                    },
                )
            };
            if let Err(err) = compute_result {
                let tree = {
                    let applier = composition.applier_mut();
                    applier.dump_tree(Some(root))
                };
                panic!("layout compute failed: {err:?}\nTree:\n{tree}");
            }
        }

        let anim = animation.get();
        let current_direction = anim.direction;

        if last_direction < 0.0 && current_direction > 0.0 && anim.progress < 0.1 {
            forward_flip_frame = Some(frame_num);
            println!(
                "Forward flip detected at frame {}: progress={:.3}, direction={:.1}",
                frame_num, anim.progress, current_direction
            );
            break;
        }

        last_direction = current_direction;
    }

    assert!(
        forward_flip_frame.is_some(),
        "Should detect forward flip within 2000 frames"
    );

    let frames_at_flip = stats.get().frames;
    println!("Stats frames at flip: {}", frames_at_flip);

    println!("Advancing 100 frames after flip...");
    for _ in 0..100 {
        time += 16_666_667;
        runtime.drain_frame_callbacks(time);
        drain_all(&mut composition).expect("drain post-flip");
        {
            use crate::layout::LayoutEngine;
            let root = composition.root().expect("composition root");
            let compute_result = {
                let mut applier = composition.applier_mut();
                applier.compute_layout(
                    root,
                    crate::modifier::Size {
                        width: 1280.0,
                        height: 720.0,
                    },
                )
            };
            if let Err(err) = compute_result {
                let tree = {
                    let applier = composition.applier_mut();
                    applier.dump_tree(Some(root))
                };
                panic!("layout compute failed: {err:?}\nTree:\n{tree}");
            }
        }
    }

    let frames_after_flip = stats.get().frames;
    let anim_after = animation.get();

    println!("Stats frames after flip: {}", frames_after_flip);
    println!(
        "Animation after flip: progress={:.3}, direction={:.1}",
        anim_after.progress, anim_after.direction
    );

    assert!(
        frames_after_flip > frames_at_flip,
        "BUG REPRODUCED: Frames stopped incrementing after forward flip! \
         Before flip: {}, After flip: {} (should be ~{} if working). \
         Animation: progress={:.3}, direction={:.1}. \
         The LaunchedEffect continues but stats updates don't trigger UI recomposition.",
        frames_at_flip,
        frames_after_flip,
        frames_at_flip + 100,
        anim_after.progress,
        anim_after.direction
    );

    assert!(
        composition.should_render(),
        "BUG: Composition should schedule rerender when stats change, but doesn't"
    );
}
