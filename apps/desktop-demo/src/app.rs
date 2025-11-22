use anyhow::{anyhow, Context};
use compose_animation::animateFloatAsState;
use compose_core::{
    self, compositionLocalOf, CompositionLocal, CompositionLocalProvider, DisposableEffect,
    DisposableEffectResult, LaunchedEffect, LaunchedEffectAsync, MutableState,
};
use compose_foundation::PointerEventKind;
use compose_ui::{
    composable, BoxSpec, Brush, Button, Color, Column, ColumnSpec, CornerRadii, GraphicsLayer,
    IntrinsicSize, LinearArrangement, Modifier, Point, PointerInputScope, RoundedCornerShape, Row,
    RowSpec, Size, Spacer, Text, VerticalAlignment,
};
use std::cell::RefCell;

mod mineswapper2;

thread_local! {
    pub static TEST_COMPOSITION_LOCAL_COUNTER: RefCell<Option<MutableState<i32>>> = const { RefCell::new(None) };
    pub static TEST_ACTIVE_TAB_STATE: RefCell<Option<MutableState<DemoTab>>> = const { RefCell::new(None) };
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum DemoTab {
    Counter,
    CompositionLocal,
    Async,
    WebFetch,
    Layout,
    ModifierShowcase,
    Mineswapper2,
}

impl DemoTab {
    pub fn label(self) -> &'static str {
        match self {
            DemoTab::Counter => "Counter App",
            DemoTab::CompositionLocal => "CompositionLocal Test",
            DemoTab::Async => "Async Runtime",
            DemoTab::WebFetch => "Web Fetch",
            DemoTab::Layout => "Recursive Layout",
            DemoTab::ModifierShowcase => "Modifiers Showcase",
            DemoTab::Mineswapper2 => "Mineswapper2",
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

#[derive(Clone, Debug, PartialEq, Eq)]
enum FetchStatus {
    Idle,
    Loading,
    Success(String),
    Error(String),
}

fn local_holder() -> CompositionLocal<Holder> {
    thread_local! {
        static LOCAL_HOLDER: RefCell<Option<CompositionLocal<Holder>>> = const { RefCell::new(None) };
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
        *cell.borrow_mut() = Some(active_tab);
    });

    Column(
        Modifier::empty().padding(20.0),
        ColumnSpec::default(),
        move || {
            let tab_state_for_row = active_tab;
            let tab_state_for_content = active_tab;
            Row(
                Modifier::empty().fill_max_width().padding(8.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::SpacedBy(8.0)),
                move || {
                    let render_tab_button = {
                        move |tab: DemoTab| {
                            let tab_state_for_tab = tab_state_for_row;
                            let is_active = tab_state_for_tab.get() == tab;

                            Button(
                                Modifier::empty()
                                    .rounded_corners(12.0)
                                    .draw_behind(move |scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(if is_active {
                                                Color(0.2, 0.45, 0.9, 1.0)
                                            } else {
                                                Color(0.3, 0.3, 0.3, 0.5)
                                            }),
                                            CornerRadii::uniform(12.0),
                                        );
                                    })
                                    .padding(10.0),
                                {
                                    let tab_state = tab_state_for_tab;
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
                        }
                    };

                    render_tab_button(DemoTab::Counter);
                    render_tab_button(DemoTab::CompositionLocal);
                    render_tab_button(DemoTab::Async);
                    render_tab_button(DemoTab::WebFetch);
                    render_tab_button(DemoTab::Layout);
                    render_tab_button(DemoTab::ModifierShowcase);
                    render_tab_button(DemoTab::Mineswapper2);
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
                DemoTab::WebFetch => web_fetch_example(),
                DemoTab::Layout => recursive_layout_example(),
                DemoTab::ModifierShowcase => modifier_showcase_tab(),
                DemoTab::Mineswapper2 => mineswapper2::mineswapper2_tab(),
            });
        },
    );
}

#[composable]
fn recursive_layout_example() {
    let depth_state = compose_core::useState(|| 3usize);

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.10, 0.18, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "Recursive Layout Playground",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.08))
                    .rounded_corners(16.0),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Row(
                Modifier::empty().fill_max_width().padding(8.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                {
                    move || {
                        let depth = depth_state.get();
                        Button(
                            Modifier::empty()
                                .rounded_corners(16.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.35, 0.45, 0.85, 1.0)),
                                        CornerRadii::uniform(16.0),
                                    );
                                })
                                .padding(10.0),
                            {
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
                            Modifier::empty()
                                .rounded_corners(16.0)
                                .draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.65, 0.35, 0.35, 1.0)),
                                        CornerRadii::uniform(16.0),
                                    );
                                })
                                .padding(10.0),
                            {
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
                            Modifier::empty()
                                .padding(8.0)
                                .background(Color(0.12, 0.16, 0.28, 0.8))
                                .rounded_corners(12.0),
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
                Modifier::empty()
                    .fill_max_width()
                    .padding(8.0)
                    .background(Color(0.06, 0.08, 0.16, 0.9))
                    .rounded_corners(20.0)
                    .padding(12.0),
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
        Modifier::empty()
            .rounded_corners(18.0)
            .draw_behind({
                move |scope| {
                    scope.draw_round_rect(Brush::solid(accent), CornerRadii::uniform(18.0));
                }
            })
            .padding(12.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Text(
                format!("Depth {}", depth),
                Modifier::empty()
                    .padding(6.0)
                    .background(Color(0.0, 0.0, 0.0, 0.25))
                    .rounded_corners(10.0),
            );

            if depth <= 1 {
                Text(
                    format!("Leaf node #{index}"),
                    Modifier::empty()
                        .padding(6.0)
                        .background(Color(1.0, 1.0, 1.0, 0.12))
                        .rounded_corners(10.0),
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
        *cell.borrow_mut() = Some(counter);
    });

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.12, 0.10, 0.24, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        move || {
            Text(
                "CompositionLocal Subscription Test",
                Modifier::empty()
                    .padding(12.0)
                    .background(Color(1.0, 1.0, 1.0, 0.1))
                    .rounded_corners(16.0),
            );

            Spacer(Size {
                width: 0.0,
                height: 16.0,
            });

            Text(
                format!("Counter: {}", counter.get()),
                Modifier::empty()
                    .padding(8.0)
                    .background(Color(0.2, 0.3, 0.4, 0.7))
                    .rounded_corners(12.0),
            );

            Spacer(Size {
                width: 0.0,
                height: 12.0,
            });

            Button(
                Modifier::empty()
                    .rounded_corners(16.0)
                    .draw_behind(|scope| {
                        scope.draw_round_rect(
                            Brush::solid(Color(0.2, 0.45, 0.9, 1.0)),
                            CornerRadii::uniform(16.0),
                        );
                    })
                    .padding(12.0),
                {
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
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.3, 0.3, 0.3, 0.5))
            .rounded_corners(12.0),
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
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.9, 0.6, 0.4, 0.5))
            .rounded_corners(12.0),
    );
}

#[composable]
fn composition_local_content_inner() {
    let local = local_holder();
    let holder = local.current();
    Text(
        format!("READING local: count={}, rand={}", holder.count, random()),
        Modifier::empty()
            .padding(8.0)
            .background(Color(0.6, 0.9, 0.4, 0.7))
            .rounded_corners(12.0),
    );
}

#[composable]
fn async_runtime_example() {
    let is_running = compose_core::useState(|| true);
    let animation = compose_core::useState(AnimationState::default);
    let stats = compose_core::useState(FrameStats::default);
    let reset_signal = compose_core::useState(|| 0u64);

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
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.10, 0.14, 0.28, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        {
            let is_running_state = is_running;
            move || {
                Text(
                    "Async Runtime Demo",
                    Modifier::empty()
                        .padding(12.0)
                        .background(Color(1.0, 1.0, 1.0, 0.08))
                        .rounded_corners(16.0),
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
                    Modifier::empty()
                        .fill_max_width()
                        .padding(8.0)
                        .background(Color(0.06, 0.10, 0.22, 0.8))
                        .rounded_corners(18.0)
                        .padding(12.0),
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
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(26.0)
                                    .rounded_corners(13.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.12, 0.16, 0.30, 1.0)),
                                            CornerRadii::uniform(13.0),
                                        );
                                    }),
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
                                                    Modifier::empty()
                                                        .width(progress_width.min(360.0))
                                                        .height(26.0)
                                                        .then(
                                                            Modifier::empty().rounded_corners(13.0),
                                                        )
                                                        .draw_behind(|scope| {
                                                            scope.draw_round_rect(
                                                                Brush::linear_gradient(vec![
                                                                    Color(0.25, 0.55, 0.95, 1.0),
                                                                    Color(0.15, 0.35, 0.80, 1.0),
                                                                ]),
                                                                CornerRadii::uniform(13.0),
                                                            );
                                                        }),
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
                    Modifier::empty()
                        .padding(8.0)
                        .background(Color(0.18, 0.22, 0.36, 0.6))
                        .rounded_corners(14.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Row(
                    Modifier::empty().fill_max_width().padding(4.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let toggle_state = is_running_state;
                        let animation_state = animation;
                        let stats_state = stats;
                        let reset_state = reset_signal;
                        move || {
                            let running = toggle_state.get();
                            let button_color = if running {
                                Color(0.50, 0.20, 0.35, 1.0)
                            } else {
                                Color(0.2, 0.45, 0.9, 1.0)
                            };
                            Button(
                                Modifier::empty()
                                    .rounded_corners(16.0)
                                    .draw_behind({
                                        let color = button_color;
                                        move |scope| {
                                            scope.draw_round_rect(
                                                Brush::solid(color),
                                                CornerRadii::uniform(16.0),
                                            );
                                        }
                                    })
                                    .padding(12.0),
                                move || toggle_state.set(!toggle_state.get()),
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

                            let reset_animation = animation_state;
                            let reset_stats = stats_state;
                            let reset_tick_state = reset_state;
                            Button(
                                Modifier::empty()
                                    .rounded_corners(16.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.16, 0.36, 0.82, 1.0)),
                                            CornerRadii::uniform(16.0),
                                        );
                                    })
                                    .padding(12.0),
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
fn web_fetch_example() {
    let fetch_status = compose_core::useState(|| FetchStatus::Idle);
    let request_counter = compose_core::useState(|| 0u64);

    {
        let status_state = fetch_status;
        let request_state = request_counter;
        let request_key = request_state.get();
        LaunchedEffect!(request_key, move |scope| {
            let status = status_state;
            status.set(FetchStatus::Loading);

            scope.launch_background(
                move |token| {
                    if token.is_cancelled() {
                        return Err(anyhow!("request cancelled"));
                    }

                    let client = reqwest::blocking::Client::builder()
                        .user_agent("compose-rs-desktop-demo/0.1")
                        .build()
                        .context("building HTTP client")?;

                    let response = client
                        .get("https://api.github.com/zen")
                        .send()
                        .context("sending request")?;

                    let status = response.status();
                    let body = response.text().context("reading response body")?;

                    if status.is_success() {
                        Ok(body.trim().to_string())
                    } else {
                        Err(anyhow!("Request failed with status {}: {}", status, body))
                    }
                },
                move |fetch_result| match fetch_result {
                    Ok(text) => status.set(FetchStatus::Success(text)),
                    Err(error) => status.set(FetchStatus::Error(error.to_string())),
                },
            );
        });
    }

    Column(
        Modifier::empty()
            .padding(32.0)
            .background(Color(0.08, 0.12, 0.22, 1.0))
            .rounded_corners(24.0)
            .padding(20.0),
        ColumnSpec::default(),
        {
            let status_state = fetch_status;
            let request_state = request_counter;
            move || {
                Text(
                    "Fetch data from the web",
                    Modifier::empty()
                        .padding(12.0)
                        .background(Color(1.0, 1.0, 1.0, 0.08))
                        .rounded_corners(16.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                Text(
                    concat!(
                        "This tab uses LaunchedEffect with a background worker to ",
                        "request a short motto from api.github.com/zen. Each click ",
                        "spawns a request and updates the UI when the response ",
                        "arrives.",
                    ),
                    Modifier::empty()
                        .padding(12.0)
                        .background(Color(0.12, 0.16, 0.28, 0.7))
                        .rounded_corners(14.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Row(
                    Modifier::empty().fill_max_width().padding(4.0),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let status_for_button = status_state;
                        let request_for_button = request_state;
                        move || {
                            Button(
                                Modifier::empty()
                                    .rounded_corners(14.0)
                                    .draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::linear_gradient(vec![
                                                Color(0.22, 0.52, 0.92, 1.0),
                                                Color(0.14, 0.42, 0.78, 1.0),
                                            ]),
                                            CornerRadii::uniform(14.0),
                                        );
                                    })
                                    .padding(10.0),
                                move || {
                                    status_for_button.set(FetchStatus::Loading);
                                    request_for_button.update(|tick| *tick = tick.wrapping_add(1));
                                },
                                || {
                                    Text(
                                        "Fetch motto",
                                        Modifier::empty()
                                            .padding(6.0)
                                            .background(Color(1.0, 1.0, 1.0, 0.05))
                                            .rounded_corners(10.0),
                                    );
                                },
                            );
                        }
                    },
                );

                Spacer(Size {
                    width: 0.0,
                    height: 12.0,
                });

                let status_snapshot = status_state.get();
                let (status_label, banner_color) = match &status_snapshot {
                    FetchStatus::Idle => (
                        "Click the button to start an HTTP request",
                        Color(0.14, 0.24, 0.36, 0.8),
                    ),
                    FetchStatus::Loading => {
                        ("Contacting api.github.com...", Color(0.20, 0.30, 0.48, 0.9))
                    }
                    FetchStatus::Success(_) => {
                        ("Success: received response", Color(0.16, 0.42, 0.26, 0.85))
                    }
                    FetchStatus::Error(_) => ("Request failed", Color(0.45, 0.18, 0.18, 0.85)),
                };

                Text(
                    status_label,
                    Modifier::empty()
                        .padding(10.0)
                        .background(banner_color)
                        .rounded_corners(12.0),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 8.0,
                });

                match status_snapshot {
                    FetchStatus::Idle => {
                        Text(
                            "No request has been made yet.",
                            Modifier::empty()
                                .padding(10.0)
                                .background(Color(0.10, 0.16, 0.28, 0.7))
                                .rounded_corners(12.0),
                        );
                    }
                    FetchStatus::Loading => {
                        Text(
                            "Hang tight while the response arrives...",
                            Modifier::empty()
                                .padding(10.0)
                                .background(Color(0.12, 0.18, 0.32, 0.9))
                                .rounded_corners(12.0),
                        );
                    }
                    FetchStatus::Success(message) => {
                        Text(
                            format!("\"{}\"", message),
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.14, 0.34, 0.26, 0.9))
                                .rounded_corners(14.0),
                        );
                    }
                    FetchStatus::Error(error) => {
                        Text(
                            format!("Error: {}", error),
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.40, 0.18, 0.18, 0.9))
                                .rounded_corners(14.0),
                        );
                    }
                }
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
        let async_message_state = async_message;
        LaunchedEffect!(fetch_key, move |scope| {
            if fetch_key == 0 {
                return;
            }
            let message_for_ui = async_message_state;
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
                    Modifier::empty()
                        .padding(12.0)
                        .then(
                            Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                                16.0, 24.0, 16.0, 24.0,
                            )),
                        )
                        .draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                                CornerRadii::uniform(20.0),
                            );
                        }),
                );
            } else {
                println!("Rendering odd branch");
                Text(
                    "if counter % 2 != 0",
                    Modifier::empty()
                        .padding(12.0)
                        .then(
                            Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                                16.0, 24.0, 16.0, 24.0,
                            )),
                        )
                        .draw_with_content(|scope| {
                            scope.draw_round_rect(
                                Brush::solid(Color(1.0, 1.0, 1.0, 0.5)),
                                CornerRadii::uniform(20.0),
                            );
                        }),
                );
            }
        });
    });

    compose_ui::Box(Modifier::empty(), BoxSpec::default(), move || {
        Column(
            Modifier::empty()
                .padding(32.0)
                .rounded_corners(24.0)
                .draw_behind({
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
                })
                .padding(20.0),
            ColumnSpec::default(),
            {
                let counter_main = counter;
                let pointer_position_main = pointer_position;
                let pointer_down_main = pointer_down;
                let wave_main = wave_state;
                move || {
                    let counter = counter_main;
                    let pointer_position = pointer_position_main;
                    let pointer_down = pointer_down_main;
                    let wave = wave_main;
                    Text(
                        "Compose-RS Playground",
                        Modifier::empty()
                            .padding(12.0)
                            .then(
                                Modifier::empty().rounded_corner_shape(RoundedCornerShape::new(
                                    16.0, 24.0, 16.0, 24.0,
                                )),
                            )
                            .draw_with_content(|scope| {
                                scope.draw_round_rect(
                                    Brush::solid(Color(1.0, 1.0, 1.0, 0.1)),
                                    CornerRadii::uniform(20.0),
                                );
                            }),
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 12.0,
                    });

                    Row(
                        Modifier::empty().fill_max_width().padding(8.0),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        {
                            let counter_display = counter;
                            let wave_value = wave;
                            move || {
                                let wave_value = wave_value.value();
                                Text(
                                    format!("Counter: {}", counter_display.get()),
                                    Modifier::empty()
                                        .padding(8.0)
                                        .then(
                                            Modifier::empty()
                                                .background(Color(0.0, 0.0, 0.0, 0.35)),
                                        )
                                        .rounded_corners(12.0),
                                );
                                Text(
                                    format!("Wave {:.2}", wave_value),
                                    Modifier::empty()
                                        .padding(8.0)
                                        .then(
                                            Modifier::empty()
                                                .background(Color(0.35, 0.55, 0.9, 0.5)),
                                        )
                                        .rounded_corners(12.0)
                                        .graphics_layer(GraphicsLayer {
                                            alpha: 0.7 + wave_value * 0.3,
                                            scale: 0.85 + wave_value * 0.3,
                                            translation_x: 0.0,
                                            translation_y: (wave_value - 0.5) * 12.0,
                                        }),
                                );
                            }
                        },
                    );

                    Spacer(Size {
                        width: 0.0,
                        height: 16.0,
                    });

                    let async_message_state = async_message;
                    let fetch_request_state = fetch_request;
                    Column(
                        Modifier::empty()
                            .rounded_corners(20.0)
                            .draw_with_cache(|cache| {
                                cache.on_draw_behind(|scope| {
                                    scope.draw_round_rect(
                                        Brush::solid(Color(0.16, 0.18, 0.26, 0.95)),
                                        CornerRadii::uniform(20.0),
                                    );
                                });
                            })
                            .draw_with_content({
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
                            })
                            .pointer_input((), {
                                let pointer_position_state = pointer_position;
                                let pointer_down_state = pointer_down;
                                move |scope: PointerInputScope| async move {
                                    scope
                                        .await_pointer_event_scope(|await_scope| async move {
                                            loop {
                                                let event = await_scope.await_pointer_event().await;
                                                println!(
                                                    "Pointer event: kind={:?} pos=({:.1}, {:.1})",
                                                    event.kind, event.position.x, event.position.y
                                                );
                                                match event.kind {
                                                    PointerEventKind::Down => {
                                                        pointer_down_state.set(true)
                                                    }
                                                    PointerEventKind::Up => {
                                                        pointer_down_state.set(false)
                                                    }
                                                    PointerEventKind::Move => {
                                                        pointer_position_state.set(Point {
                                                            x: event.position.x,
                                                            y: event.position.y,
                                                        });
                                                    }
                                                    PointerEventKind::Cancel => {
                                                        pointer_down_state.set(false)
                                                    }
                                                }
                                            }
                                        })
                                        .await;
                                }
                            })
                            .padding(16.0),
                        ColumnSpec::default(),
                        move || {
                            Text(
                                format!("Pointer: ({:.1}, {:.1})", pointer.x, pointer.y),
                                Modifier::empty()
                                    .padding(8.0)
                                    .background(Color(0.1, 0.1, 0.15, 0.6))
                                    .rounded_corners(12.0)
                                    .padding(8.0),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 16.0,
                            });

                            Row(
                                Modifier::empty()
                                    .padding(8.0)
                                    .rounded_corners(12.0)
                                    .background(Color(0.1, 0.1, 0.15, 0.6))
                                    .padding(8.0),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                                    .vertical_alignment(VerticalAlignment::CenterVertically),
                                || {
                                    Button(
                                        Modifier::empty()
                                            .width_intrinsic(IntrinsicSize::Max)
                                            .rounded_corners(12.0)
                                            .draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.3, 0.5, 0.2, 1.0)),
                                                    CornerRadii::uniform(12.0),
                                                );
                                            })
                                            .padding(10.0),
                                        || {},
                                        || {
                                            Text(
                                                "OK",
                                                Modifier::empty().padding(4.0).then(
                                                    Modifier::empty().size(Size {
                                                        width: 50.0,
                                                        height: 50.0,
                                                    }),
                                                ),
                                            );
                                        },
                                    );
                                    Button(
                                        Modifier::empty()
                                            .width_intrinsic(IntrinsicSize::Max)
                                            .rounded_corners(12.0)
                                            .draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.5, 0.3, 0.2, 1.0)),
                                                    CornerRadii::uniform(12.0),
                                                );
                                            })
                                            .padding(10.0),
                                        || {},
                                        || {
                                            Text("Cancel", Modifier::empty().padding(4.0));
                                        },
                                    );
                                    Button(
                                        Modifier::empty()
                                            .width_intrinsic(IntrinsicSize::Max)
                                            .rounded_corners(12.0)
                                            .draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.2, 0.3, 0.5, 1.0)),
                                                    CornerRadii::uniform(12.0),
                                                );
                                            })
                                            .padding(10.0),
                                        || {},
                                        || {
                                            Text(
                                                "Long Button Text",
                                                Modifier::empty().padding(4.0),
                                            );
                                        },
                                    );
                                },
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 16.0,
                            });

                            let counter_inc = counter;
                            let counter_dec = counter;
                            Row(
                                Modifier::empty().fill_max_width().padding(8.0),
                                RowSpec::new()
                                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                                move || {
                                    Button(
                                        Modifier::empty()
                                            .rounded_corners(16.0)
                                            .draw_with_cache(|cache| {
                                                cache.on_draw_behind(|scope| {
                                                    scope.draw_round_rect(
                                                        Brush::linear_gradient(vec![
                                                            Color(0.2, 0.45, 0.9, 1.0),
                                                            Color(0.15, 0.3, 0.65, 1.0),
                                                        ]),
                                                        CornerRadii::uniform(16.0),
                                                    );
                                                });
                                            })
                                            .padding(12.0),
                                        {
                                            let counter = counter_inc;
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
                                        Modifier::empty()
                                            .rounded_corners(16.0)
                                            .draw_behind(|scope| {
                                                scope.draw_round_rect(
                                                    Brush::solid(Color(0.4, 0.18, 0.3, 1.0)),
                                                    CornerRadii::uniform(16.0),
                                                );
                                            })
                                            .padding(12.0),
                                        {
                                            let counter = counter_dec;
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

                            let async_message_text = async_message_state;
                            Text(
                                async_message_text.get(),
                                Modifier::empty()
                                    .padding(10.0)
                                    .background(Color(0.1, 0.18, 0.32, 0.6))
                                    .rounded_corners(14.0),
                            );

                            Spacer(Size {
                                width: 0.0,
                                height: 12.0,
                            });

                            let async_message_button = async_message_state;
                            let fetch_request_button = fetch_request_state;
                            Button(
                                Modifier::empty()
                                    .rounded_corners(16.0)
                                    .draw_with_cache(|cache| {
                                        cache.on_draw_behind(|scope| {
                                            scope.draw_round_rect(
                                                Brush::linear_gradient(vec![
                                                    Color(0.15, 0.35, 0.85, 1.0),
                                                    Color(0.08, 0.2, 0.55, 1.0),
                                                ]),
                                                CornerRadii::uniform(16.0),
                                            );
                                        });
                                    })
                                    .padding(12.0),
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

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum ShowcaseType {
    SimpleCard,
    PositionedBoxes,
    ItemList,
    ComplexChain,
    DynamicModifiers,
    LongList,
}

impl ShowcaseType {
    fn label(self) -> &'static str {
        match self {
            ShowcaseType::SimpleCard => "Simple Card",
            ShowcaseType::PositionedBoxes => "Positioned Boxes",
            ShowcaseType::ItemList => "Item List (5)",
            ShowcaseType::ComplexChain => "Complex Chain",
            ShowcaseType::DynamicModifiers => "Dynamic Modifiers",
            ShowcaseType::LongList => "Long List (50)",
        }
    }
}

#[composable]
fn modifier_showcase_tab() {
    let selected_showcase = compose_core::useState(|| ShowcaseType::SimpleCard);

    Row(
        Modifier::empty().fill_max_width().padding(8.0),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
            .vertical_alignment(VerticalAlignment::Top),
        move || {
            // Left panel - showcase selector
            Column(
                Modifier::empty()
                    .width(180.0)
                    .padding(16.0)
                    .background(Color(0.08, 0.10, 0.18, 1.0))
                    .rounded_corners(20.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                {
                    let showcase_state = selected_showcase;
                    move || {
                        Text(
                            "Select Showcase",
                            Modifier::empty()
                                .padding(8.0)
                                .background(Color(1.0, 1.0, 1.0, 0.08))
                                .rounded_corners(12.0),
                        );

                        Spacer(Size {
                            width: 0.0,
                            height: 8.0,
                        });

                        let showcase_types = [
                            ShowcaseType::SimpleCard,
                            ShowcaseType::PositionedBoxes,
                            ShowcaseType::ItemList,
                            ShowcaseType::ComplexChain,
                            ShowcaseType::DynamicModifiers,
                            ShowcaseType::LongList,
                        ];

                        for showcase_type in showcase_types {
                            let is_selected = showcase_state.get() == showcase_type;
                            Button(
                                Modifier::empty()
                                    .fill_max_width()
                                    .rounded_corners(10.0)
                                    .draw_behind(move |scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(if is_selected {
                                                Color(0.25, 0.45, 0.85, 1.0)
                                            } else {
                                                Color(0.15, 0.18, 0.25, 0.8)
                                            }),
                                            CornerRadii::uniform(10.0),
                                        );
                                    })
                                    .padding(10.0),
                                {
                                    move || {
                                        if showcase_state.get() != showcase_type {
                                            showcase_state.set(showcase_type);
                                        }
                                    }
                                },
                                {
                                    let label = showcase_type.label();
                                    move || {
                                        Text(label, Modifier::empty().padding(4.0));
                                    }
                                },
                            );
                        }
                    }
                },
            );

            // Right panel - showcase content
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .padding(24.0)
                    .background(Color(0.06, 0.08, 0.16, 0.9))
                    .rounded_corners(20.0)
                    .padding(16.0),
                ColumnSpec::default(),
                {
                    let selected_showcase_inner = selected_showcase;
                    move || {
                        let showcase_to_render = selected_showcase_inner.get();
                        compose_core::with_key(&showcase_to_render, || match showcase_to_render {
                            ShowcaseType::SimpleCard => simple_card_showcase(),
                            ShowcaseType::PositionedBoxes => positioned_boxes_showcase(),
                            ShowcaseType::ItemList => item_list_showcase(),
                            ShowcaseType::ComplexChain => complex_chain_showcase(),
                            ShowcaseType::DynamicModifiers => dynamic_modifiers_showcase(),
                            ShowcaseType::LongList => long_list_showcase(),
                        });
                    }
                },
            );
        },
    );
}

#[composable]
pub fn simple_card_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Simple Card Pattern ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        // Card with border effect (outer box creates border)
        compose_ui::Box(
            Modifier::empty()
                .padding(3.0)
                .background(Color(0.4, 0.6, 0.9, 0.8))
                .rounded_corners(18.0),
            BoxSpec::default(),
            || {
                compose_ui::Box(
                    Modifier::empty()
                        .padding(16.0)
                        .size(Size {
                            width: 300.0,
                            height: 200.0,
                        })
                        .background(Color(0.15, 0.18, 0.25, 0.95))
                        .rounded_corners(16.0),
                    BoxSpec::default(),
                    || {
                        Column(
                            Modifier::empty().padding(8.0),
                            ColumnSpec::default(),
                            || {
                                Text(
                                    "Card Title",
                                    Modifier::empty()
                                        .padding(8.0)
                                        .background(Color(0.3, 0.5, 0.8, 0.6))
                                        .rounded_corners(8.0),
                                );

                                Spacer(Size {
                                    width: 0.0,
                                    height: 8.0,
                                });

                                Text(
                                    "Card content goes here with padding",
                                    Modifier::empty().padding(4.0),
                                );

                                Spacer(Size {
                                    width: 0.0,
                                    height: 12.0,
                                });

                                // Action buttons row
                                Row(Modifier::empty(), RowSpec::default(), || {
                                    Text(
                                        "Action 1",
                                        Modifier::empty()
                                            .padding(8.0)
                                            .background(Color(0.2, 0.7, 0.4, 0.7))
                                            .rounded_corners(6.0),
                                    );

                                    Spacer(Size {
                                        width: 8.0,
                                        height: 0.0,
                                    });

                                    Text(
                                        "Action 2",
                                        Modifier::empty()
                                            .padding(8.0)
                                            .background(Color(0.8, 0.3, 0.3, 0.7))
                                            .rounded_corners(6.0),
                                    );
                                });
                            },
                        );
                    },
                );
            },
        );
    });
}

#[composable]
pub fn positioned_boxes_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Positioned Boxes ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        // Wrap positioned boxes in a container with explicit size
        // This allows overlapping boxes with offset positioning
        compose_ui::Box(
            Modifier::empty()
                .size_points(360.0, 280.0)
                .background(Color(0.05, 0.05, 0.15, 0.5))
                .rounded_corners(8.0),
            BoxSpec::default(),
            || {
                // Box A - Purple, top-left
                compose_ui::Box(
                    Modifier::empty()
                        .size_points(100.0, 100.0)
                        .offset(20.0, 20.0)
                        .padding(8.0)
                        .background(Color(0.6, 0.2, 0.7, 0.85))
                        .rounded_corners(12.0),
                    BoxSpec::default(),
                    || {
                        Text("Box A", Modifier::empty().padding(6.0));
                    },
                );

                // Box B - Green, bottom-right
                compose_ui::Box(
                    Modifier::empty()
                        .size_points(100.0, 100.0)
                        .offset(220.0, 160.0)
                        .padding(8.0)
                        .background(Color(0.2, 0.7, 0.4, 0.85))
                        .rounded_corners(12.0),
                    BoxSpec::default(),
                    || {
                        Text("Box B", Modifier::empty().padding(6.0));
                    },
                );

                // Box C - Orange, center-top (smaller)
                compose_ui::Box(
                    Modifier::empty()
                        .size_points(80.0, 60.0)
                        .offset(140.0, 30.0)
                        .padding(6.0)
                        .background(Color(0.9, 0.5, 0.2, 0.85))
                        .rounded_corners(10.0),
                    BoxSpec::default(),
                    || {
                        Text("C", Modifier::empty().padding(4.0));
                    },
                );

                // Box D - Blue, center-left (larger)
                compose_ui::Box(
                    Modifier::empty()
                        .size_points(120.0, 80.0)
                        .offset(40.0, 140.0)
                        .padding(8.0)
                        .background(Color(0.2, 0.5, 0.9, 0.85))
                        .rounded_corners(14.0),
                    BoxSpec::default(),
                    || {
                        Text("Box D", Modifier::empty().padding(6.0));
                    },
                );
            },
        );
    });
}

#[composable]
pub fn item_list_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Item List (5 items) ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        // List with alternating colors and borders
        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
            || {
                for i in 0..5 {
                    // Alternate colors: even = blue-ish, odd = purple-ish
                    let (bg_color, border_color) = if i % 2 == 0 {
                        (Color(0.12, 0.16, 0.28, 0.8), Color(0.3, 0.5, 0.8, 0.9))
                    } else {
                        (Color(0.18, 0.12, 0.28, 0.8), Color(0.5, 0.3, 0.8, 0.9))
                    };

                    // Border wrapper
                    compose_ui::Box(
                        Modifier::empty()
                            .padding(2.0)
                            .background(border_color)
                            .rounded_corners(12.0),
                        BoxSpec::default(),
                        move || {
                            Row(
                                Modifier::empty()
                                    .padding(8.0)
                                    .size_points(400.0, 50.0)
                                    .background(bg_color)
                                    .rounded_corners(10.0),
                                RowSpec::default(),
                                move || {
                                    let text = match i {
                                        0 => "Item #0",
                                        1 => "Item #1",
                                        2 => "Item #2",
                                        3 => "Item #3",
                                        4 => "Item #4",
                                        _ => "Item",
                                    };
                                    Text(text, Modifier::empty().padding_horizontal(12.0));

                                    Spacer(Size {
                                        width: 0.0,
                                        height: 0.0,
                                    });

                                    // Status indicator
                                    let status_color = if i % 3 == 0 {
                                        Color(0.2, 0.8, 0.3, 0.9) // Green
                                    } else if i % 3 == 1 {
                                        Color(0.9, 0.7, 0.2, 0.9) // Yellow
                                    } else {
                                        Color(0.8, 0.3, 0.2, 0.9) // Red
                                    };

                                    compose_ui::Box(
                                        Modifier::empty()
                                            .size_points(12.0, 12.0)
                                            .background(status_color)
                                            .rounded_corners(6.0),
                                        BoxSpec::default(),
                                        || {},
                                    );
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
pub fn complex_chain_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Complex Modifier Chain ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            "Nested: Red → Green → Blue layers",
            Modifier::empty().padding(8.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        // Nested backgrounds showcase - creates visible colored borders
        // Red outer layer
        compose_ui::Box(
            Modifier::empty()
                .padding(8.0)
                .background(Color(0.8, 0.2, 0.2, 0.9))
                .rounded_corners(16.0),
            BoxSpec::default(),
            || {
                // Green middle layer
                compose_ui::Box(
                    Modifier::empty()
                        .padding(6.0)
                        .background(Color(0.2, 0.7, 0.3, 0.9))
                        .rounded_corners(12.0),
                    BoxSpec::default(),
                    || {
                        // Blue inner layer
                        compose_ui::Box(
                            Modifier::empty()
                                .padding(12.0)
                                .background(Color(0.3, 0.5, 0.9, 0.9))
                                .rounded_corners(8.0),
                            BoxSpec::default(),
                            || {
                                Text("Nested!", Modifier::empty());
                            },
                        );
                    },
                );
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            "Chain: offset + size + multiple backgrounds",
            Modifier::empty().padding(8.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        // Complex modifier chain with offset and sizing - Orange outer, Purple inner
        compose_ui::Box(
            Modifier::empty()
                .offset(20.0, 0.0)
                .size_points(180.0, 80.0)
                .padding(6.0)
                .background(Color(0.9, 0.6, 0.2, 0.9))
                .rounded_corners(10.0),
            BoxSpec::default(),
            || {
                compose_ui::Box(
                    Modifier::empty()
                        .padding(8.0)
                        .background(Color(0.5, 0.3, 0.7, 0.9))
                        .rounded_corners(6.0),
                    BoxSpec::default(),
                    || {
                        Text("Offset + Sized", Modifier::empty());
                    },
                );
            },
        );
    });
}

#[composable]
pub fn dynamic_modifiers_showcase() {
    let frame = compose_core::useState(|| 0i32);

    Column(Modifier::empty(), ColumnSpec::default(), move || {
        Text(
            "=== Dynamic Modifiers ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        let current_frame = frame.get();
        let x = (current_frame as f32 * 10.0) % 200.0;
        let y = 50.0;

        // Wrap moving box in a container with explicit size to prevent overflow
        compose_ui::Box(
            Modifier::empty()
                .size_points(250.0, 150.0)
                .background(Color(0.05, 0.05, 0.15, 0.5))
                .rounded_corners(8.0),
            BoxSpec::default(),
            move || {
                compose_ui::Box(
                    Modifier::empty()
                        .size(Size {
                            width: 50.0,
                            height: 50.0,
                        })
                        .offset(x, y)
                        .padding(6.0)
                        .background(Color(0.3, 0.6, 0.9, 0.9))
                        .rounded_corners(10.0),
                    BoxSpec::default(),
                    || {
                        Text("Move", Modifier::empty());
                    },
                );
            },
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Text(
            format!("Frame: {}, X: {:.1}", current_frame, x),
            Modifier::empty()
                .padding(8.0)
                .background(Color(0.2, 0.2, 0.3, 0.6))
                .rounded_corners(10.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 12.0,
        });

        Button(
            Modifier::empty()
                .rounded_corners(12.0)
                .draw_behind(|scope| {
                    scope.draw_round_rect(
                        Brush::solid(Color(0.25, 0.45, 0.85, 1.0)),
                        CornerRadii::uniform(12.0),
                    );
                })
                .padding(10.0),
            {
                let frame_state = frame;
                move || {
                    frame_state.set(frame_state.get() + 1);
                }
            },
            || {
                Text("Advance Frame", Modifier::empty().padding(6.0));
            },
        );
    });
}

#[composable]
pub fn long_list_showcase() {
    Column(Modifier::empty(), ColumnSpec::default(), || {
        Text(
            "=== Long List (50 items) ===",
            Modifier::empty()
                .padding(12.0)
                .background(Color(1.0, 1.0, 1.0, 0.1))
                .rounded_corners(14.0),
        );

        Spacer(Size {
            width: 0.0,
            height: 16.0,
        });

        Column(
            Modifier::empty().padding(16.0),
            ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
            || {
                for i in 0..50 {
                    Row(
                        Modifier::empty()
                            .padding_symmetric(8.0, 4.0)
                            .size(Size {
                                width: 400.0,
                                height: 40.0,
                            })
                            .background(Color(0.12 + (i as f32 * 0.005), 0.15, 0.25, 0.7))
                            .rounded_corners(8.0),
                        RowSpec::default(),
                        move || {
                            let text = if i < 10 {
                                match i {
                                    0 => "Item 0",
                                    1 => "Item 1",
                                    2 => "Item 2",
                                    3 => "Item 3",
                                    4 => "Item 4",
                                    5 => "Item 5",
                                    6 => "Item 6",
                                    7 => "Item 7",
                                    8 => "Item 8",
                                    9 => "Item 9",
                                    _ => "Item",
                                }
                            } else {
                                "Item 10+"
                            };
                            Text(text, Modifier::empty().padding_horizontal(12.0));
                        },
                    );
                }
            },
        );
    });
}

#[cfg(test)]
#[path = "tests/main_tests.rs"]
mod tests;
