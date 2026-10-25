// A dashboard that fills a 320 by 480 portrait screen and changes most of
// it every frame: a ring gauge with its percentage, three level bars, an
// activity chart of bars under a line, two switches, a slider and a list
// that scrolls by itself, below a header with a clock and three readings.
//
// Every value on it is a function of the milliseconds since its first
// frame, the `timeline` below. The ESP32 benchmarks build the same screen in
// LVGL and Slint from the same functions, so the three show the same picture
// at the same time. The ESP32 demo's `dashboard` feature shows it, and the
// framebuffer bench draws it with `SCREEN=dashboard`.

use std::f32::consts::TAU;

use cranpose::{prelude::*, text::TextUnit};
use cranpose_animation::{Easing, animateFloatAsState, tween};
use cranpose_core::{LaunchedEffectAsync, MutableState, derivedStateOf, rememberMutableStateOf};

// The values the screen shows at `t` milliseconds after its first frame.
pub(crate) mod timeline {
    use super::TAU;

    // The bars of the activity chart.
    pub(crate) const BARS: usize = 24;
    // The points of the line over them.
    pub(crate) const POINTS: usize = 32;
    // The rows of the list, each `ROW_HEIGHT` tall.
    pub(crate) const ROWS: usize = 40;
    pub(crate) const ROW_HEIGHT: u32 = 28;

    // A sine of period `period` milliseconds, started at `phase` radians.
    fn wave(t: u64, period: u64, phase: f32) -> f32 {
        (TAU * (t % period) as f32 / period as f32 + phase).sin()
    }

    // The gauge's fill, from 0.05 to 0.95.
    pub(crate) fn gauge(t: u64) -> f32 {
        0.5 + 0.45 * wave(t, 4000, 0.0)
    }

    // The gauge's fill as the whole percent its label shows.
    pub(crate) fn gauge_percent(t: u64) -> u32 {
        (gauge(t) * 100.0 + 0.5) as u32
    }

    // The fill of level bar `index`, from 0.05 to 0.95.
    pub(crate) fn level(t: u64, index: usize) -> f32 {
        0.5 + 0.45 * wave(t, 2000 + 700 * index as u64, index as f32)
    }

    // The height of chart bar `index`, as a fraction of the chart's.
    pub(crate) fn bar(t: u64, index: usize) -> f32 {
        0.55 + 0.4 * wave(t, 1600, index as f32 * 0.5)
    }

    // The height of the line's point `index`, as a fraction of the chart's.
    pub(crate) fn line(t: u64, index: usize) -> f32 {
        0.5 + 0.35 * wave(t, 2500, index as f32 * 0.35)
    }

    // Where the slider's thumb is, from 0 to 1.
    pub(crate) fn slider(t: u64) -> f32 {
        0.5 + 0.5 * wave(t, 3000, 0.0)
    }

    // Whether the switch that flips every `period` milliseconds is on.
    pub(crate) fn switch_on(t: u64, period: u64) -> bool {
        (t / period).is_multiple_of(2)
    }

    // How far the list has scrolled, in pixels: 60 a second, starting over
    // after 1000.
    pub(crate) fn scroll(t: u64) -> u32 {
        (t * 60 / 1000 % 1000) as u32
    }

    // The clock: minutes and seconds.
    pub(crate) fn clock(t: u64) -> (u64, u64) {
        let seconds = t / 1000;
        (seconds / 60 % 60, seconds % 60)
    }

    // The CPU reading, in percent: changes every 200 ms.
    pub(crate) fn cpu(t: u64) -> u64 {
        20 + (t / 200 * 37) % 61
    }

    // The temperature reading, in tenths of a degree: changes every 330 ms.
    pub(crate) fn temperature(t: u64) -> u64 {
        360 + (t / 330 * 7) % 30
    }

    // The network reading, in tenths of a MB/s: changes every 500 ms.
    pub(crate) fn network(t: u64) -> u64 {
        5 + (t / 500 * 13) % 90
    }

    // The value list row `index` shows.
    pub(crate) fn row_value(index: usize) -> usize {
        (index * 37 + 11) % 100
    }
}

const BACKGROUND: Color = Color::from_rgb_u8(15, 18, 32);
const CARD: Color = Color::from_rgb_u8(27, 31, 51);
const TRACK: Color = Color::from_rgb_u8(42, 48, 80);
const MUTED: Color = Color::from_rgb_u8(140, 147, 176);
const BLUE: Color = Color::from_rgb_u8(67, 97, 238);
const VIOLET: Color = Color::from_rgb_u8(123, 47, 247);
const CYAN: Color = Color::from_rgb_u8(76, 201, 240);
const PINK: Color = Color::from_rgb_u8(247, 37, 133);
const PURPLE: Color = Color::from_rgb_u8(114, 9, 183);
const YELLOW: Color = Color::from_rgb_u8(255, 209, 102);
const ORANGE: Color = Color::from_rgb_u8(247, 127, 0);
const RED: Color = Color::from_rgb_u8(255, 107, 107);
const TEAL: Color = Color::from_rgb_u8(78, 205, 196);
const AVATARS: [Color; 5] = [BLUE, PINK, TEAL, ORANGE, VIOLET];

fn style(color: Color, size: f32) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            ..SpanStyle::default()
        },
        ..TextStyle::default()
    }
}

fn card() -> Modifier {
    Modifier::empty().background(CARD).rounded_corners(12.0)
}

fn centered_row() -> RowSpec {
    RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically)
}

// The milliseconds since the first frame, read where a value changes.
type Time = MutableState<u64>;

#[composable]
fn Header(title: &'static str, time: Time) {
    let clock = derivedStateOf(move || timeline::clock(time.get()));
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(40.0)
            .draw_behind(|scope| {
                scope.draw_round_rect(
                    Brush::horizontal_gradient(vec![BLUE, VIOLET], 0.0, f32::INFINITY),
                    CornerRadii::uniform(12.0),
                );
            })
            .padding_horizontal(12.0),
        centered_row(),
        move || {
            Text(
                title,
                Modifier::empty().weight(1.0),
                style(Color::WHITE, 18.0),
            );
            let (minutes, seconds) = clock.value();
            Text(
                format!("{minutes:02}:{seconds:02}"),
                Modifier::empty(),
                style(Color::WHITE, 18.0),
            );
        },
    );
}

#[composable]
fn Stat(label: &'static str, dot: Color, value: State<String>) {
    Column(
        card().weight(1.0).padding(8.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(2.0)),
        move || {
            Row(Modifier::empty(), centered_row(), move || {
                Box(
                    Modifier::empty()
                        .size(Size::new(8.0, 8.0))
                        .background(dot)
                        .rounded_corners(4.0),
                    BoxSpec::default(),
                    || {},
                );
                Text(
                    label,
                    Modifier::empty().padding_each(6.0, 0.0, 0.0, 0.0),
                    style(MUTED, 12.0),
                );
            });
            Text(value.value(), Modifier::empty(), style(Color::WHITE, 16.0));
        },
    );
}

#[composable]
fn Stats(time: Time) {
    let cpu = derivedStateOf(move || format!("{}%", timeline::cpu(time.get())));
    let temperature = derivedStateOf(move || {
        let tenths = timeline::temperature(time.get());
        format!("{}.{} C", tenths / 10, tenths % 10)
    });
    let network = derivedStateOf(move || {
        let tenths = timeline::network(time.get());
        format!("{}.{} MB/s", tenths / 10, tenths % 10)
    });
    Row(
        Modifier::empty().fill_max_width().height(52.0),
        RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            Stat("CPU", RED, cpu);
            Stat("TEMP", TEAL, temperature);
            Stat("NET", YELLOW, network);
        },
    );
}

#[composable]
fn GaugeLabel(time: Time) {
    let percent = derivedStateOf(move || timeline::gauge_percent(time.get()));
    Column(
        Modifier::empty(),
        ColumnSpec::default().horizontal_alignment(HorizontalAlignment::CenterHorizontally),
        move || {
            Text(
                format!("{}%", percent.value()),
                Modifier::empty(),
                style(Color::WHITE, 20.0),
            );
            Text("load", Modifier::empty(), style(MUTED, 12.0));
        },
    );
}

#[composable]
fn Ring(time: Time) {
    Box(
        Modifier::empty()
            .size(Size::new(84.0, 84.0))
            .draw_behind(move |scope| {
                let size = scope.size();
                let center = Point::new(size.width / 2.0, size.height / 2.0);
                let radius = size.width / 2.0 - 5.0;
                scope.draw_circle_stroked(Brush::solid(TRACK), center, radius, Stroke::new(10.0));
                scope.draw_arc(
                    Brush::solid(CYAN),
                    center,
                    radius,
                    -TAU / 4.0,
                    TAU * timeline::gauge(time.get()),
                    Stroke {
                        cap: StrokeCap::Round,
                        ..Stroke::new(10.0)
                    },
                );
            }),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || GaugeLabel(time),
    );
}

#[composable]
fn Level(label: &'static str, index: usize, colors: [Color; 2], time: Time) {
    Text(label, Modifier::empty(), style(MUTED, 12.0));
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(8.0)
            .background(TRACK)
            .rounded_corners(4.0)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect_at(
                    Rect {
                        x: 0.0,
                        y: 0.0,
                        width: size.width * timeline::level(time.get(), index),
                        height: size.height,
                    },
                    Brush::horizontal_gradient(colors.to_vec(), 0.0, f32::INFINITY),
                    CornerRadii::uniform(4.0),
                );
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn Gauge(time: Time) {
    Row(
        card().fill_max_width().height(112.0).padding(10.0),
        centered_row(),
        move || {
            Ring(time);
            Column(
                Modifier::empty()
                    .weight(1.0)
                    .padding_each(14.0, 0.0, 0.0, 0.0),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(3.0)),
                move || {
                    Level("Fan", 0, [PINK, PURPLE], time);
                    Level("Pump", 1, [CYAN, BLUE], time);
                    Level("Light", 2, [YELLOW, ORANGE], time);
                },
            );
        },
    );
}

#[composable]
fn Chart(time: Time) {
    Column(
        card().fill_max_width().height(100.0).padding(10.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            Text("Activity", Modifier::empty(), style(MUTED, 12.0));
            Canvas(
                Modifier::empty().fill_max_width().weight(1.0),
                move |scope| {
                    let t = time.get();
                    let size = scope.size();
                    let gap = 3.0;
                    let count = timeline::BARS as f32;
                    let width = (size.width - gap * (count - 1.0)) / count;
                    let brush = Brush::vertical_gradient(vec![CYAN, BLUE], 0.0, f32::INFINITY);
                    for index in 0..timeline::BARS {
                        let height = size.height * timeline::bar(t, index);
                        scope.draw_round_rect_at(
                            Rect {
                                x: index as f32 * (width + gap),
                                y: size.height - height,
                                width,
                                height,
                            },
                            brush.clone(),
                            CornerRadii::uniform(2.0),
                        );
                    }
                    let mut line = Path::new();
                    let step = size.width / (timeline::POINTS - 1) as f32;
                    for index in 0..timeline::POINTS {
                        let point = Point::new(
                            index as f32 * step,
                            size.height * (1.0 - timeline::line(t, index)),
                        );
                        if index == 0 {
                            line.move_to(point);
                        } else {
                            line.line_to(point);
                        }
                    }
                    scope.draw_path(
                        &line,
                        Brush::solid(YELLOW),
                        DrawStyle::Stroke(Stroke {
                            cap: StrokeCap::Round,
                            join: StrokeJoin::Round,
                            ..Stroke::new(2.0)
                        }),
                    );
                },
            );
        },
    );
}

#[composable]
fn Switch(period: u64, time: Time) {
    let on = derivedStateOf(move || timeline::switch_on(time.get(), period));
    let on = on.value();
    let knob = animateFloatAsState(
        if on { 1.0 } else { 0.0 },
        tween(200, Easing::LinearEasing),
        "knob",
    );
    Canvas(
        Modifier::empty().size(Size::new(40.0, 22.0)),
        move |scope| {
            let size = scope.size();
            let radius = size.height / 2.0;
            scope.draw_round_rect(
                Brush::solid(if on { CYAN } else { TRACK }),
                CornerRadii::uniform(radius),
            );
            let x = radius + knob.value() * (size.width - size.height);
            scope.draw_circle(
                Brush::solid(Color::WHITE),
                Point::new(x, radius),
                radius - 2.0,
            );
        },
    );
}

#[composable]
fn Slider(time: Time) {
    Canvas(Modifier::empty().weight(1.0).height(22.0), move |scope| {
        let size = scope.size();
        let x = 8.0 + timeline::slider(time.get()) * (size.width - 16.0);
        let top = size.height / 2.0 - 2.0;
        scope.draw_round_rect_at(
            Rect {
                x: 0.0,
                y: top,
                width: size.width,
                height: 4.0,
            },
            Brush::solid(TRACK),
            CornerRadii::uniform(2.0),
        );
        scope.draw_round_rect_at(
            Rect {
                x: 0.0,
                y: top,
                width: x,
                height: 4.0,
            },
            Brush::solid(BLUE),
            CornerRadii::uniform(2.0),
        );
        scope.draw_circle(
            Brush::solid(Color::WHITE),
            Point::new(x, size.height / 2.0),
            8.0,
        );
    });
}

#[composable]
fn Controls(time: Time) {
    Row(
        card()
            .fill_max_width()
            .height(44.0)
            .padding_horizontal(12.0),
        centered_row().horizontal_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            Switch(1000, time);
            Switch(1500, time);
            Slider(time);
        },
    );
}

#[composable]
fn SensorRow(index: usize) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(timeline::ROW_HEIGHT as f32)
            .padding_horizontal(10.0),
        centered_row(),
        move || {
            Box(
                Modifier::empty()
                    .size(Size::new(20.0, 20.0))
                    .background(AVATARS[index % AVATARS.len()])
                    .rounded_corners(10.0),
                BoxSpec::default().content_alignment(Alignment::CENTER),
                move || {
                    let letter = char::from(b'A' + (index % AVATARS.len()) as u8);
                    Text(
                        letter.to_string(),
                        Modifier::empty(),
                        style(Color::WHITE, 12.0),
                    );
                },
            );
            Text(
                format!("Sensor {:02}", index + 1),
                Modifier::empty()
                    .weight(1.0)
                    .padding_each(8.0, 0.0, 0.0, 0.0),
                style(Color::WHITE, 14.0),
            );
            Text(
                format!("{} kWh", timeline::row_value(index)),
                Modifier::empty(),
                style(MUTED, 12.0),
            );
        },
    );
}

#[composable]
fn SensorList(state: LazyListState) {
    Box(
        card().fill_max_width().weight(1.0).padding_vertical(6.0),
        BoxSpec::default(),
        move || {
            LazyColumn(
                Modifier::empty().fill_max_size().clip_to_bounds(),
                state,
                LazyColumnSpec::default(),
                |scope| {
                    scope.items(LazyItems::new(timeline::ROWS), SensorRow);
                },
            );
        },
    );
}

// The dashboard, titled `title`.
#[composable]
pub fn Dashboard(title: &'static str) {
    let time = rememberMutableStateOf(|| 0u64);
    let list = rememberLazyListState();
    LaunchedEffectAsync((), move |scope| {
        Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            let start = clock.next_frame().await;
            loop {
                let now = clock.next_frame().await;
                let t = now.saturating_sub(start) / 1_000_000;
                time.set(t);
                let scroll = timeline::scroll(t);
                list.scroll_to_item(
                    (scroll / timeline::ROW_HEIGHT) as usize,
                    (scroll % timeline::ROW_HEIGHT) as f32,
                );
            }
        })
    });
    Column(
        Modifier::empty()
            .fill_max_size()
            .background(BACKGROUND)
            .padding(10.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        move || {
            Header(title, time);
            Stats(time);
            Gauge(time);
            Chart(time);
            Controls(time);
            SensorList(list);
        },
    );
}
