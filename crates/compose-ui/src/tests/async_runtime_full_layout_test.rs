// Comprehensive test reproducing the async runtime freeze bug with full layout structure
//
// This test mimics the actual desktop demo app's async_runtime_example() function
// to reproduce the exact freeze bug where:
// - Animation continues (LaunchedEffectAsync keeps running)
// - BUT: Text "Frames advanced: N" freezes
// - BUT: Button "Pause..." appearance doesn't change
//
// Root cause: Conditional rendering breaks RecomposeScope connections for sibling components

use crate::{
    Brush, Button, Color, Column, ColumnSpec, CornerRadii, Modifier, Row, RowSpec, Size, Spacer,
    Text,
};
use compose_core::{
    location_key, Composition, MemoryApplier, MutableState, Node,
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

/// Composable function that mimics the full async_runtime_example() layout
/// This includes:
/// - LaunchedEffectAsync updating animation + stats
/// - Column with Title Text
/// - Progress Column with conditional Row (THE BUG TRIGGER)
/// - Stats Text showing frame count (FREEZES)
/// - Button row with Pause button (FREEZES)
#[composable]
fn async_runtime_full_layout(
    is_running: MutableState<bool>,
    animation: MutableState<AnimationState>,
    stats: MutableState<FrameStats>,
) {
    // LaunchedEffectAsync - exactly like the demo
    {
        let animation_state = animation.clone();
        let stats_state = stats.clone();
        let running_state = is_running.clone();
        launched_effect_async_impl(
            location_key(file!(), line!(), column!()),
            (),
            move |scope| {
                let animation = animation_state.clone();
                let stats = stats_state.clone();
                let running = running_state.clone();
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
                                delta_nanos = 16_666_667; // 60 FPS fallback
                            }
                            let dt_ms = delta_nanos as f32 / 1_000_000.0;

                            // Update stats - THIS SHOULD TRIGGER RECOMPOSITION
                            stats.update(|state| {
                                state.frames = state.frames.wrapping_add(1);
                                state.last_frame_ms = dt_ms;
                            });

                            // Update animation - this triggers conditional rendering
                            animation.update(|anim| {
                                let next = anim.progress + 0.1 * anim.direction * (dt_ms / 600.0);
                                if next >= 1.0 {
                                    anim.progress = 1.0;
                                    anim.direction = -1.0; // Flip to reverse
                                } else if next <= 0.0 {
                                    anim.progress = 0.0;
                                    anim.direction = 1.0; // Flip to forward
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

    // Full layout structure matching the demo
    Column(
        Modifier::empty().padding(32.0),
        ColumnSpec::default(),
        move || {
            // Title Text
            Text("Async Runtime Demo", Modifier::empty().padding(12.0));

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Get snapshots for this render
            let animation_snapshot = animation.get();
            let stats_snapshot = stats.get();
            let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);
            let fill_width = 320.0 * progress_value;

            // Progress Column with conditional Row
            Column(
                Modifier::empty().padding(8.0),
                ColumnSpec::default(),
                move || {
                    Text(
                        format!("Progress: {:>3}%", (progress_value * 100.0) as i32),
                        Modifier::empty().padding(6.0),
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 8.0,
                    });

                    // Outer container Row
                    Row(
                        Modifier::empty()
                            .height(26.0)
                            .then(Modifier::empty().rounded_corners(13.0)),
                        RowSpec::default(),
                        {
                            let progress_width = fill_width;
                            move || {
                                // CRITICAL: Conditional rendering that triggers the bug
                                // When progress goes from >0 to 0 and back, this changes composition structure
                                if progress_width > 0.0 {
                                    Row(
                                        Modifier::empty()
                                            .width(progress_width.min(360.0))
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

            // Stats Text - THIS SHOULD UPDATE BUT FREEZES
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
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            // Button Row
            {
                let is_running_for_button = is_running.clone();
                Row(
                    Modifier::empty().padding(4.0),
                    RowSpec::default(),
                    move || {
                        let running = is_running_for_button.get();

                        // Pause/Resume Button - APPEARANCE SHOULD CHANGE BUT FREEZES
                        let button_label = if running {
                            "Pause animation"
                        } else {
                            "Resume animation"
                        };
                        Button(
                            Modifier::empty().padding(12.0),
                            {
                                let toggle_state = is_running_for_button.clone();
                                move || toggle_state.set(!toggle_state.get())
                            },
                            move || {
                                Text(button_label, Modifier::empty().padding(6.0));
                            },
                        );
                    },
                );
            }
        },
    );
}

/// Helper to drain all pending recompositions until stable
fn drain_all<A: compose_core::Applier + 'static>(
    composition: &mut Composition<A>,
) -> Result<(), compose_core::NodeError> {
    let mut iterations = 0;
    loop {
        // Check if process_invalid_scopes did any work
        if !composition.process_invalid_scopes()? {
            if iterations > 100 {
                println!("drain_all: Took {} iterations to stabilize", iterations);
            }
            return Ok(());
        }
        iterations += 1;
        if iterations > 1000 {
            eprintln!("drain_all: Exceeded 1000 iterations, giving up. This indicates an infinite recomposition loop. {iterations}");
            return Err(compose_core::NodeError::MissingContext {
                id: 0,
                reason: "drain_all: Exceeded 1000 iterations",
            }); // Indicate error instead of panicking
        }
    }
}

#[test]
fn async_runtime_full_layout_freezes_after_forward_flip() {
    // Setup composition
    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();

    // Create states
    let is_running = MutableState::with_runtime(true, runtime.clone());
    let animation = MutableState::with_runtime(AnimationState::default(), runtime.clone());
    let stats = MutableState::with_runtime(FrameStats::default(), runtime.clone());

    // Initial render
    composition
        .render(location_key(file!(), line!(), column!()), &mut || {
            async_runtime_full_layout(is_running.clone(), animation.clone(), stats.clone());
        })
        .expect("initial render");
    drain_all(&mut composition).expect("initial drain");

    println!("Starting animation loop, looking for forward flip...");

    // Advance frames until we detect a forward flip (reverse -> forward transition)
    let mut time = 0u64;
    let mut last_direction = animation.get().direction;
    let mut forward_flip_frame: Option<u32> = None;

    for frame_num in 0..2000 {
        time += 16_666_667; // 60 FPS (16.67ms per frame)
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

        // Detect forward flip: direction was negative, now positive, progress near 0
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

    // Advance 100 more frames AFTER the flip
    // This is where the bug manifests: frames should continue incrementing but don't
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

    // BUG REPRODUCTION: This assertion will FAIL
    // Expected: frames_after_flip > frames_at_flip (e.g., 890 > 790)
    // Actual: frames_after_flip == frames_at_flip (e.g., 790 == 790)
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

    // Additional assertion: composition should recognize need to rerender
    // This will also fail because RecomposeScopes are disconnected
    assert!(
        composition.should_render(),
        "BUG: Composition should schedule rerender when stats change, but doesn't"
    );
}
