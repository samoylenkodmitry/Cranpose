use compose_animation::animateFloatAsState;
use compose_core::{
    self, compositionLocalOf, CompositionLocal, CompositionLocalProvider, DisposableEffect,
    DisposableEffectResult, LaunchedEffect, LaunchedEffectAsync, MutableState,
};
use compose_foundation::{PointerEvent, PointerEventKind};
use compose_ui::{
    composable, BoxSpec, Brush, Button, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    IntrinsicSize, LinearArrangement, Modifier, Point, PointerInputScope, RoundedCornerShape, Row,
    RowSpec, Size, Spacer, Text, VerticalAlignment,
};
use std::cell::RefCell;

thread_local! {
    pub static TEST_COMPOSITION_LOCAL_COUNTER: RefCell<Option<MutableState<i32>>> = RefCell::new(None);
    pub static TEST_ACTIVE_TAB_STATE: RefCell<Option<MutableState<DemoTab>>> = RefCell::new(None);
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DemoTab {
    Counter,
    CompositionLocal,
    Async,
    Layout,
}

impl DemoTab {
    pub fn label(self) -> &'static str {
        match self {
            DemoTab::Counter => "Counter App",
            DemoTab::CompositionLocal => "CompositionLocal Test",
            DemoTab::Async => "Async Runtime",
            DemoTab::Layout => "Recursive Layout",
        }
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
struct Holder {
    count: i32,
}

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

fn local_holder() -> CompositionLocal<Holder> {
    thread_local! {
        static LOCAL_HOLDER: RefCell<Option<CompositionLocal<Holder>>> = RefCell::new(None);
    }
    LOCAL_HOLDER.with(|cell| {
        let mut opt = cell.borrow_mut();
        if opt.is_none() {
            *opt = Some(compositionLocalOf(|| Holder { count: 0 }));
        }
        opt.as_ref().unwrap().clone()
    })
}

fn random() -> i32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    (nanos % 10000) as i32
}

#[composable]
pub fn combined_app() {
    let active_tab = compose_core::useState(|| DemoTab::Counter);
    TEST_ACTIVE_TAB_STATE.with(|cell| {
        *cell.borrow_mut() = Some(active_tab.clone());
    });

    Column(Modifier::empty().padding(20.0), ColumnSpec::default(), move || {
        let tab_state_for_row = active_tab.clone();
        let tab_state_for_content = active_tab.clone();
        Row(
            Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
            RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
            move || {
                let tab_state = tab_state_for_row.clone();
                let render_tab_button = move |tab: DemoTab| {
                    let tab_state = tab_state.clone();
                    let is_active = tab_state.get() == tab;
                    Button(
                        Modifier::empty().rounded_corners(12.0)
                            .then(Modifier::empty().draw_behind(move |scope| {
                                scope.draw_round_rect(
                                    Brush::solid(if is_active {
                                        Color(0.2, 0.45, 0.9, 1.0)
                                    } else {
                                        Color(0.3, 0.3, 0.3, 0.5)
                                    }),
                                    CornerRadii::uniform(12.0),
                                );
                            }))
                            .then(Modifier::empty().padding(10.0)),
                        {
                            let tab_state = tab_state.clone();
                            move || {
                                if tab_state.get() != tab {
                                    println!("{} button clicked", tab.label());
                                    tab_state.set(tab);
                                }
                            }
                        },
                        {
                            let label = tab.label();
                            move || {
                                Text(label, Modifier::empty().padding(4.0));
                            }
                        },
                    );
                };

                render_tab_button(DemoTab::Counter);
                render_tab_button(DemoTab::CompositionLocal);
                render_tab_button(DemoTab::Async);
                render_tab_button(DemoTab::Layout);
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        let active = tab_state_for_content.get();
        compose_core::with_key(&active, || match active {
            DemoTab::Counter => counter_app(),
            DemoTab::CompositionLocal => composition_local_example(),
            DemoTab::Async => async_runtime_example(),
            DemoTab::Layout => recursive_layout_example(),
        });
    });
}

#[composable]
fn recursive_layout_example() {
    let depth_state = compose_core::useState(|| 3usize);

    Column(
        Modifier::empty().padding(32.0)
            .then(Modifier::empty().background(Color(0.08, 0.10, 0.18, 1.0)))
            .then(Modifier::empty().rounded_corners(24.0))
            .then(Modifier::empty().padding(20.0)),
        ColumnSpec::default(),
        move || {
            Text(
                "Recursive Layout Playground",
                Modifier::empty().padding(12.0)
                    .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.08)))
                    .then(Modifier::empty().rounded_corners(16.0)),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Row(
                Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    let depth_state = depth_state.clone();
                    move || {
                        let depth = depth_state.get();
                        Button(
                            Modifier::empty().rounded_corners(16.0)
                                .then(Modifier::empty().draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.35, 0.45, 0.85, 1.0)),
                                        CornerRadii::uniform(16.0),
                                    );
                                }))
                                .then(Modifier::empty().padding(10.0)),
                            {
                                let depth_state = depth_state.clone();
                                move || {
                                    let next = (depth_state.get() + 1).min(96);
                                    if next != depth_state.get() {
                                        depth_state.set(next);
                                    }
                                }
                            },
                            || {
                                Text("Increase depth", Modifier::empty().padding(6.0));
                            },
                        );

                        Button(
                            Modifier::empty().rounded_corners(16.0)
                                .then(Modifier::empty().draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.65, 0.35, 0.35, 1.0)),
                                        CornerRadii::uniform(16.0),
                                    );
                                }))
                                .then(Modifier::empty().padding(10.0)),
                            {
                                let depth_state = depth_state.clone();
                                move || {
                                    let next = depth_state.get().saturating_sub(1).max(1);
                                    if next != depth_state.get() {
                                        depth_state.set(next);
                                    }
                                }
                            },
                            || {
                                Text("Decrease depth", Modifier::empty().padding(6.0));
                            },
                        );

                        Text(
                            format!("Current depth: {}", depth.max(1)),
                            Modifier::empty().padding(8.0)
                                .then(Modifier::empty().background(Color(0.12, 0.16, 0.28, 0.8)))
                                .then(Modifier::empty().rounded_corners(12.0)),
                        );
                    }
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            let depth = depth_state.get().max(1);
            Column(
                Modifier::empty().fill_max_width()
                    .then(Modifier::empty().padding(8.0))
                    .then(Modifier::empty().background(Color(0.06, 0.08, 0.16, 0.9)))
                    .then(Modifier::empty().rounded_corners(20.0))
                    .then(Modifier::empty().padding(12.0)),
                ColumnSpec::default(),
                move || {
                    recursive_layout_node(depth, true, 0);
                },
            );
        },
    );
}

#[composable]
fn recursive_layout_node(depth: usize, horizontal: bool, index: usize) {
    let palette = [
        Color(0.25, 0.32, 0.58, 0.75),
        Color(0.30, 0.20, 0.45, 0.75),
        Color(0.20, 0.40, 0.32, 0.75),
        Color(0.45, 0.28, 0.24, 0.75),
    ];
    let accent = palette[index % palette.len()];

    Column(
        Modifier::empty().rounded_corners(18.0)
            .then(Modifier::empty().draw_behind({
                move |scope| {
                    scope.draw_round_rect(Brush::solid(accent), CornerRadii::uniform(18.0));
                }
            }))
            .then(Modifier::empty().padding(12.0)),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                format!("Depth {}", depth),
                Modifier::empty().padding(6.0)
                    .then(Modifier::empty().background(Color(0.0, 0.0, 0.0, 0.25)))
                    .then(Modifier::empty().rounded_corners(10.0)),
            );

            if depth <= 1 {
                Text(
                    format!("Leaf node #{index}"),
                    Modifier::empty().padding(6.0)
                        .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.12)))
                        .then(Modifier::empty().rounded_corners(10.0)),
                );
            } else if horizontal {
                Row(
                    Modifier::empty().fill_max_width(),
                    RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(depth - 1, false, index * 2 + child_idx + 1);
                        }
                    },
                );
            } else {
                Column(
                    Modifier::empty().fill_max_width(),
                    ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                    move || {
                        for child_idx in 0..2 {
                            recursive_layout_node(depth - 1, true, index * 2 + child_idx + 1);
                        }
                    },
                );
            }
        },
    );
}

#[composable]
pub fn composition_local_example() {
    let counter = compose_core::useState(|| 0);

    TEST_COMPOSITION_LOCAL_COUNTER.with(|cell| {
        *cell.borrow_mut() = Some(counter.clone());
    });

    Column(
        Modifier::empty().padding(32.0)
            .then(Modifier::empty().background(Color(0.12, 0.10, 0.24, 1.0)))
            .then(Modifier::empty().rounded_corners(24.0))
            .then(Modifier::empty().padding(20.0)),
        ColumnSpec::default(),
        move || {
            Text(
                "CompositionLocal Subscription Test",
                Modifier::empty().padding(12.0)
                    .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.1)))
                    .then(Modifier::empty().rounded_corners(16.0)),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty().padding(8.0)
                    .then(Modifier::empty().background(Color(0.2, 0.3, 0.4, 0.7)))
                    .then(Modifier::empty().rounded_corners(12.0)),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Button(
                Modifier::empty().rounded_corners(16.0)
                    .then(Modifier::empty().draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.2, 0.45, 0.9, 1.0)),
                            CornerRadii::uniform(16.0),
                        );
                    }))
                    .then(Modifier::empty().padding(12.0)),
                {
                    let counter = counter.clone();
                    move || {
                        let new_val = counter.get() + 1;
                        println!("Incrementing counter to {}", new_val);
                        counter.set(new_val);
                    }
                },
                || {
                    Text("Increment", Modifier::empty().padding(6.0));
                },
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            let local = local_holder();
            let count = counter.get();

            CompositionLocalProvider(vec![local.provides(Holder { count })], || {
                composition_local_content();
            });
        },
    );
}

#[composable]
fn composition_local_content() {
    Text(
        format!("Outside provider (NOT reading): rand={}", random()),
        Modifier::empty().padding(8.0)
            .then(Modifier::empty().background(Color(0.3, 0.3, 0.3, 0.5)))
            .then(Modifier::empty().rounded_corners(12.0)),
    );

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    composition_local_content_inner();

    Spacer(Size {
        width: 0.0,
        height: 8.0,
    });

    Text(
        format!("NOT reading local: rand={}", random()),
        Modifier::empty().padding(8.0)
            .then(Modifier::empty().background(Color(0.9, 0.6, 0.4, 0.5)))
            .then(Modifier::empty().rounded_corners(12.0)),
    );
}

#[composable]
fn composition_local_content_inner() {
    let local = local_holder();
    let holder = local.current();
    Text(
        format!("READING local: count={}, rand={}", holder.count, random()),
        Modifier::empty().padding(8.0)
            .then(Modifier::empty().background(Color(0.6, 0.9, 0.4, 0.7)))
            .then(Modifier::empty().rounded_corners(12.0)),
    );
}

#[composable]
fn async_runtime_example() {
    let is_running = compose_core::useState(|| true);
    let animation = compose_core::useState(AnimationState::default);
    let stats = compose_core::useState(FrameStats::default);
    let reset_signal = compose_core::useState(|| 0u64);

    {
        let animation_state = animation.clone();
        let stats_state = stats.clone();
        let running_state = is_running.clone();
        let reset_state = reset_signal.clone();
        LaunchedEffectAsync!((), move |scope| {
            let animation = animation_state.clone();
            let stats = stats_state.clone();
            let running = running_state.clone();
            let reset = reset_state.clone();
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
                            // Fall back to a nominal 60 FPS delta so the animation keeps
                            // advancing even if two callbacks report the same timestamp.
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
        Modifier::empty().padding(32.0)
            .then(Modifier::empty().background(Color(0.10, 0.14, 0.28, 1.0)))
            .then(Modifier::empty().rounded_corners(24.0))
            .then(Modifier::empty().padding(20.0)),
        ColumnSpec::default(),
        {
            let is_running_state = is_running.clone();
            move || {
                Text(
                    "Async Runtime Demo",
                    Modifier::empty().padding(12.0)
                        .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.08)))
                        .then(Modifier::empty().rounded_corners(16.0)),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                let animation_snapshot = animation.get();
                let stats_snapshot = stats.get();
                let progress_value = animation_snapshot.progress.clamp(0.0, 1.0);
                let fill_width = 320.0 * progress_value;
                Column(
                    Modifier::empty().fill_max_width()
                        .then(Modifier::empty().padding(8.0))
                        .then(Modifier::empty().background(Color(0.06, 0.10, 0.22, 0.8)))
                        .then(Modifier::empty().rounded_corners(18.0))
                        .then(Modifier::empty().padding(12.0)),
                    ColumnSpec::default(),
                    {
                        move || {
                            Text(
                                format!("Progress: {:>3}%", (progress_value * 100.0) as i32),
                                Modifier::empty().padding(6.0),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 8.0,
                            });

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
                                        // WORKAROUND: Use with_key to prevent slot truncation from destroying
                                        // sibling component scopes when conditional rendering changes structure.
                                        // TODO: Remove once proper "gaps" support is implemented in compose-core
                                        compose_core::with_key(&(progress_width > 0.0), || {
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
                                        });
                                    }
                                },
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
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
                    Modifier::empty().padding(8.0)
                        .then(Modifier::empty().background(Color(0.18, 0.22, 0.36, 0.6)))
                        .then(Modifier::empty().rounded_corners(14.0)),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Row(
                    Modifier::empty().fill_max_width().then(Modifier::empty().padding(4.0)),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let toggle_state = is_running_state.clone();
                        let animation_state = animation.clone();
                        let stats_state = stats.clone();
                        let reset_state = reset_signal.clone();
                        move || {
                            let running = toggle_state.get();
                            let button_color = if running {
                                Color(0.50, 0.20, 0.35, 1.0)
                            } else {
                                Color(0.2, 0.45, 0.9, 1.0)
                            };
                            Button(
                                Modifier::empty().rounded_corners(16.0)
                                    .then(Modifier::empty().draw_behind({
                                        let color = button_color;
                                        move |scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(color),
                                                CornerRadii::uniform(16.0),
                                            );
                                        }
                                    }))
                                    .then(Modifier::empty().padding(12.0)),
                                {
                                    let toggle_state = toggle_state.clone();
                                    move || toggle_state.set(!toggle_state.get())
                                },
                                {
                                    let label = if running {
                                        "Pause animation"
                                    } else {
                                        "Resume animation"
                                    };
                                    move || {
                                        Text(label, Modifier::empty().padding(6.0));
                                    }
                                },
                            );

                            let reset_animation = animation_state.clone();
                            let reset_stats = stats_state.clone();
                            let reset_tick_state = reset_state.clone();
                            let toggle_state = toggle_state.clone();
                            Button(
                                Modifier::empty().rounded_corners(16.0)
                                    .then(Modifier::empty().draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.16, 0.36, 0.82, 1.0)),
                                            CornerRadii::uniform(16.0),
                                        );
                                    }))
                                    .then(Modifier::empty().padding(12.0)),
                                move || {
                                    reset_animation.set(AnimationState::default());
                                    reset_stats.set(FrameStats::default());
                                    if !toggle_state.get() {
                                        toggle_state.set(true);
                                    }
                                    reset_tick_state.update(|tick| *tick = tick.wrapping_add(1));
                                },
                                || {
                                    Text("Reset", Modifier::empty().padding(6.0));
                                },
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn counter_app() {
    let counter = compose_core::useState(|| 0);
    let pointer_position = compose_core::useState(|| Point { x: 0.0, y: 0.0 });
    let pointer_down = compose_core::useState(|| false);
    let async_message =
        compose_core::useState(|| "Tap \"Fetch async value\" to run background work".to_string());
    let fetch_request = compose_core::useState(|| 0u64);
    let pointer = pointer_position.get();
    let pointer_wave = (pointer.x / 360.0).clamp(0.0, 1.0);
    let target_wave = if pointer_down.get() {
        0.6 + pointer_wave * 0.4
    } else {
        pointer_wave * 0.6
    };
    let wave_state = animateFloatAsState(target_wave, "wave");
    let fetch_key = fetch_request.get();
    {
        let async_message = async_message.clone();
        LaunchedEffect!(fetch_key, move |scope| {
            if fetch_key == 0 {
                return;
            }
            let message_for_ui = async_message.clone();
            scope.launch_background(
                move |token| {
                    use std::thread;
                    use std::time::{Duration, SystemTime, UNIX_EPOCH};

                    for _ in 0..5 {
                        if token.is_cancelled() {
                            return String::new();
                        }
                        thread::sleep(Duration::from_millis(80));
                    }

                    let nanos = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .subsec_nanos();
                    format!("Background fetch #{fetch_key}: {}", nanos % 1000)
                },
                move |value| {
                    if value.is_empty() {
                        return;
                    }
                    message_for_ui.set(value);
                },
            );
        });
    }
    LaunchedEffect!(counter.get(), |_| println!("effect call"));

    let is_even = counter.get() % 2 == 0;
    println!("Recomposing counter_app, counter={}", counter.get());
    Column(Modifier::empty(), ColumnSpec::default(), move || {
        compose_core::with_key(&is_even, move || {
            println!("Compose inside with_key, is_even={}", is_even);
            if is_even {
                println!("Rendering even branch");
                Text(
                    "if counter % 2 == 0",
                    Modifier::empty().padding(12.0)
                        .then(Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                            16.0, 24.0, 16.0, 24.0,
                        )))
                        .then(Modifier::empty().draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                                CornerRadii::uniform(20.0),
                            );
                        })),
                );
            } else {
                println!("Rendering odd branch");
                Text(
                    "if counter % 2 != 0",
                    Modifier::empty().padding(12.0)
                        .then(Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                            16.0, 24.0, 16.0, 24.0,
                        )))
                        .then(Modifier::empty().draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.5)),
                                CornerRadii::uniform(20.0),
                            );
                        })),
                );
            }
        });
    });

    compose_ui::Box(Modifier::empty(), BoxSpec::default(), move || {
        Column(
            Modifier::empty().padding(32.0)
                .then(Modifier::empty().rounded_corners(24.0))
                .then(Modifier::empty().draw_behind({
                    let phase = wave_state.value();
                    move |scope| {
                        scope.draw_round_rect(
                            Brush::linear_gradient(vec![
                                Color(0.12 + phase * 0.2, 0.10, 0.24 + (1.0 - phase) * 0.3, 1.0),
                                Color(0.08, 0.16 + (1.0 - phase) * 0.3, 0.26 + phase * 0.2, 1.0),
                            ]),
                            CornerRadii::uniform(24.0),
                        );
                    }
                }))
                .then(Modifier::empty().padding(20.0)),
            ColumnSpec::default(),
            {
                let counter_main = counter.clone();
                let pointer_position_main = pointer_position.clone();
                let pointer_down_main = pointer_down.clone();
                let wave_main = wave_state.clone();
                let async_message = async_message.clone();
                let fetch_request = fetch_request.clone();
                move || {
                    let counter = counter_main.clone();
                    let pointer_position = pointer_position_main.clone();
                    let pointer_down = pointer_down_main.clone();
                    let wave = wave_main.clone();
                    Text(
                        "Compose-RS Playground",
                        Modifier::empty().padding(12.0)
                            .then(Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                                16.0, 24.0, 16.0, 24.0,
                            )))
                            .then(Modifier::empty().draw_with_content(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                                    CornerRadii::uniform(20.0),
                                );
                            })),
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 12.0,
                    });

                    Row(
                        Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        {
                            let counter_display = counter.clone();
                            let wave_value = wave;
                            move || {
                                let wave_value = wave_value.value();
                                Text(
                                    format!("Counter: {}", counter_display.get()),
                                    Modifier::empty().padding(8.0)
                                        .then(Modifier::empty().background(Color(0.0, 0.0, 0.0, 0.35)))
                                        .then(Modifier::empty().rounded_corners(12.0)),
                                );
                                Text(
                                    format!("Wave {:.2}", wave_value),
                                    Modifier::empty().padding(8.0)
                                        .then(Modifier::empty().background(Color(0.35, 0.55, 0.9, 0.5)))
                                        .then(Modifier::empty().rounded_corners(12.0))
                                        .then(Modifier::empty().graphics_layer(GraphicsLayer {
                                            alpha: 0.7 + wave_value * 0.3,
                                            scale: 0.85 + wave_value * 0.3,
                                            translation_x: 0.0,
                                            translation_y: (wave_value - 0.5) * 12.0,
                                        })),
                                );
                            }
                        },
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 16.0,
                    });

                    let async_message_state = async_message.clone();
                    let fetch_request_state = fetch_request.clone();
                    Column(
                        Modifier::empty().rounded_corners(20.0)
                            .then(Modifier::empty().draw_with_cache(|cache| {
                                cache.on_draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.16, 0.18, 0.26, 0.95)),
                                        CornerRadii::uniform(20.0),
                                    );
                                });
                            }))
                            .then(Modifier::empty().draw_with_content({
                                let position = pointer_position.get();
                                let pressed = pointer_down.get();
                                move |scope| {
                                    let intensity = if pressed { 0.45 } else { 0.25 };
                                    scope.draw_round_rect(
                                        Brush::radial_gradient(
                                            vec![
                                                Color(0.4, 0.6, 1.0, intensity),
                                                Color(0.2, 0.3, 0.6, 0.0),
                                            ],
                                            position,
                                            120.0,
                                        ),
                                        CornerRadii::uniform(20.0),
                                    );
                                }
                            }))
                            .then(Modifier::empty().pointer_input((), {
                                let pointer_position = pointer_position.clone();
                                let pointer_down = pointer_down.clone();
                                move |scope: PointerInputScope| {
                                    let pointer_position = pointer_position.clone();
                                    let pointer_down = pointer_down.clone();
                                    async move {
                                        scope
                                            .await_pointer_event_scope(|await_scope| async move {
                                                loop {
                                                    let event =
                                                        await_scope.await_pointer_event().await;
                                                    println!(
                                                    "Pointer event: kind={:?} pos=({:.1}, {:.1})",
                                                    event.kind, event.position.x, event.position.y
                                                );
                                                    match event.kind {
                                                        PointerEventKind::Down => {
                                                            pointer_down.set(true)
                                                        }
                                                        PointerEventKind::Up => {
                                                            pointer_down.set(false)
                                                        }
                                                        PointerEventKind::Move => {
                                                            pointer_position.set(Point {
                                                                x: event.position.x,
                                                                y: event.position.y,
                                                            });
                                                        }
                                                        PointerEventKind::Cancel => {
                                                            pointer_down.set(false)
                                                        }
                                                    }
                                                }
                                            })
                                            .await;
                                    }
                                }
                            }))
                            .then(Modifier::empty().padding(16.0)),
                        ColumnSpec::default(),
                        move || {
                            let async_message_state = async_message_state.clone();
                            let fetch_request_state = fetch_request_state.clone();
                            Text(
                                format!("Pointer: ({:.1}, {:.1})", pointer.x, pointer.y),
                                Modifier::empty().padding(8.0)
                                    .then(Modifier::empty().background(Color(0.1, 0.1, 0.15, 0.6)))
                                    .then(Modifier::empty().rounded_corners(12.0))
                                    .then(Modifier::empty().padding(8.0)),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 16.0,
                            });

                            Row(
                                Modifier::empty().padding(8.0)
                                    .then(Modifier::empty().rounded_corners(12.0))
                                    .then(Modifier::empty().background(Color(0.1, 0.1, 0.15, 0.6)))
                                    .then(Modifier::empty().padding(8.0)),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                || {
                                    Button(
                                        Modifier::empty().width_intrinsic(IntrinsicSize::Max)
                                            .then(Modifier::empty().rounded_corners(12.0))
                                            .then(Modifier::empty().draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.3, 0.5, 0.2, 1.0)),
                                                    CornerRadii::uniform(12.0),
                                                );
                                            }))
                                            .then(Modifier::empty().padding(10.0)),
                                        || {},
                                        || {
                                            Text(
                                                "OK",
                                                Modifier::empty().padding(4.0).then(Modifier::empty().size(Size {
                                                    width: 50.0,
                                                    height: 50.0,
                                                })),
                                            );
                                        },
                                    );
                                    Button(
                                        Modifier::empty().width_intrinsic(IntrinsicSize::Max)
                                            .then(Modifier::empty().rounded_corners(12.0))
                                            .then(Modifier::empty().draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.5, 0.3, 0.2, 1.0)),
                                                    CornerRadii::uniform(12.0),
                                                );
                                            }))
                                            .then(Modifier::empty().padding(10.0)),
                                        || {},
                                        || {
                                            Text("Cancel", Modifier::empty().padding(4.0));
                                        },
                                    );
                                    Button(
                                        Modifier::empty().width_intrinsic(IntrinsicSize::Max)
                                            .then(Modifier::empty().rounded_corners(12.0))
                                            .then(Modifier::empty().draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.2, 0.3, 0.5, 1.0)),
                                                    CornerRadii::uniform(12.0),
                                                );
                                            }))
                                            .then(Modifier::empty().padding(10.0)),
                                        || {},
                                        || {
                                            Text("Long Button Text", Modifier::empty().padding(4.0));
                                        },
                                    );
                                },
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 16.0,
                            });

                            let counter_inc = counter.clone();
                            let counter_dec = counter.clone();
                            Row(
                                Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                                move || {
                                    Button(
                                        Modifier::empty().rounded_corners(16.0)
                                            .then(Modifier::empty().draw_with_cache(|cache| {
                                                cache.on_draw_behind(|scope| {
                                                    scope.draw_round_rect(
                                                        Brush::linear_gradient(vec![
                                                            Color(0.2, 0.45, 0.9, 1.0),
                                                            Color(0.15, 0.3, 0.65, 1.0),
                                                        ]),
                                                        CornerRadii::uniform(16.0),
                                                    );
                                                });
                                            }))
                                            .then(Modifier::empty().padding(12.0)),
                                        {
                                            let counter = counter_inc.clone();
                                            move || {
                                                println!(
                                                    "Incrementing counter to {}",
                                                    counter.get() + 1
                                                );
                                                counter.set(counter.get() + 1)
                                            }
                                        },
                                        || {
                                            Text("Increment", Modifier::empty().padding(6.0));
                                        },
                                    );
                                    Button(
                                        Modifier::empty().rounded_corners(16.0)
                                            .then(Modifier::empty().draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.4, 0.18, 0.3, 1.0)),
                                                    CornerRadii::uniform(16.0),
                                                );
                                            }))
                                            .then(Modifier::empty().padding(12.0)),
                                        {
                                            let counter = counter_dec.clone();
                                            move || counter.set(counter.get() - 1)
                                        },
                                        || {
                                            Text("Decrement", Modifier::empty().padding(6.0));
                                        },
                                    );
                                },
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 20.0,
                            });

                            let async_message_text = async_message_state.clone();
                            Text(
                                async_message_text.get(),
                                Modifier::empty().padding(10.0)
                                    .then(Modifier::empty().background(Color(0.1, 0.18, 0.32, 0.6)))
                                    .then(Modifier::empty().rounded_corners(14.0)),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 12.0,
                            });

                            let async_message_button = async_message_state.clone();
                            let fetch_request_button = fetch_request_state.clone();
                            Button(
                                Modifier::empty().rounded_corners(16.0)
                                    .then(Modifier::empty().draw_with_cache(|cache| {
                                        cache.on_draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::linear_gradient(vec![
                                                    Color(0.15, 0.35, 0.85, 1.0),
                                                    Color(0.08, 0.2, 0.55, 1.0),
                                                ]),
                                                CornerRadii::uniform(16.0),
                                            );
                                        });
                                    }))
                                    .then(Modifier::empty().padding(12.0)),
                                {
                                    move || {
                                        async_message_button.set(
                                            "Fetching value on background thread...".to_string(),
                                        );
                                        fetch_request_button.update(|value| *value += 1);
                                    }
                                },
                                || {
                                    Text("Fetch async value", Modifier::empty().padding(6.0));
                                },
                            );
                        },
                    );
                }
            },
        );
    });
}

#[composable]
fn composition_local_observer() {
    let state = compose_core::useState(|| 0);
    DisposableEffect!((), move |_| {
        state.set(state.get() + 1);
        DisposableEffectResult::default()
    });
}

#[cfg(test)]
#[path = "tests/main_tests.rs"]
mod tests;
