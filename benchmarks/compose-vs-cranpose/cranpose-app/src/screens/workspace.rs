//! The trading workspace of gpui-fast's `gpui_perf` showcase
//! (github.com/longbridge/gpui-fast, `crates/gpui_perf/src/showcase`), the
//! most complex screen it measures, ported element for element: the
//! showcase's frame (toolbar, a sidebar of 243 pages, the page header and
//! the frame-stats bar) around a docked window of market panels that a
//! timer streams quotes into.
//!
//! - A watchlist of 200 symbols in a lazy list of 15-column rows: market
//!   tags, company icons, badges, a 52-week range bar and a sparkline.
//! - The selected symbol's quote with a grid of 24 statistics, a
//!   candlestick chart of 120 candles with moving averages and volume, an
//!   order book of ten levels a side, 80 trades in two columns, and a donut
//!   of trade statistics.
//! - Every 16 ms the stream pushes 16 quotes (8 while the watchlist scrolls
//!   or is hovered). Each updates its symbol's state, which recomposes the
//!   rows showing it; the selected symbol's quotes also update every quote
//!   panel. Sixteen panels that are not shown receive every quote too.
//!
//! The sparklines, moving averages, chart guides and sort arrows are paths
//! (`DrawScope::draw_path`), filled, stroked and dashed as GPUI draws them.
//! Watchlist rows and inactive tabs take GPUI's hover backgrounds.
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::OnceLock,
    time::Duration,
};

use cranpose::{LazyItems, prelude::*};
use cranpose_core::{MutableState, delay, key, mutableStateOf};
use cranpose_foundation::lazy::{LazyListState, rememberLazyListState};
use cranpose_ui::{
    DashPathEffect, DrawStyle, Path, collect_is_hovered_as_state, rememberMutableInteractionSource,
    text::{FontWeight, ParagraphStyle, SpanStyle, TextUnit},
    widgets::{FlowRow, FlowRowSpec},
};

const SYMBOLS: usize = 200;
const HIDDEN_PANELS: usize = 16;
const ROW_HEIGHT: f32 = 32.0;
const FIRST_SCREEN: usize = 40;
const SPARK_POINTS: usize = 40;
const TRADES: usize = 80;
const BOOK_LEVELS: usize = 10;
const CANDLES: usize = 120;
const QUOTES_PER_CANDLE: usize = 40;
const SELECTED: usize = 7;
const STREAM_EVERY: Duration = Duration::from_millis(16);
const SCROLL_STEP: f32 = 32.0;

/// Called on the main thread at the start of every frame, before the frame's
/// work, with the quote ticks streamed so far, by a harness that links this
/// crate to measure what a frame costs, as gpui_perf's `--auto` samples its
/// main thread.
pub static FRAME_OBSERVER: OnceLock<fn(u64)> = OnceLock::new();

/// What the workspace does while it is measured, as gpui_perf's workspace
/// scenarios do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorkspaceMode {
    /// Quotes stream in, 16 every 16 ms; nothing else moves.
    Quotes,
    /// The watchlist scrolls 32 px a frame, bouncing at its ends, with 8
    /// quotes every 16 ms.
    Scroll,
    /// The pointer moves over the watchlist's rows, with 8 quotes every
    /// 16 ms; an external driver moves it.
    Hover,
}

impl WorkspaceMode {
    pub fn from_name(name: &str) -> Self {
        match name {
            "scroll" => Self::Scroll,
            "hover" => Self::Hover,
            _ => Self::Quotes,
        }
    }

    fn quotes_per_tick(self) -> usize {
        match self {
            Self::Quotes => 16,
            Self::Scroll | Self::Hover => 8,
        }
    }
}

// The showcase's light theme.
const BACKGROUND: Color = Color::from_rgb_u8(0xff, 0xff, 0xff);
const FOREGROUND: Color = Color::from_rgb_u8(0x0a, 0x0a, 0x0a);
const MUTED: Color = Color::from_rgb_u8(0xf4, 0xf4, 0xf5);
const MUTED_FOREGROUND: Color = Color::from_rgb_u8(0x71, 0x71, 0x7a);
const BORDER: Color = Color::from_rgb_u8(0xe4, 0xe4, 0xe7);
const INPUT: Color = Color::from_rgb_u8(0xd4, 0xd4, 0xd8);
const SIDEBAR: Color = Color::from_rgb_u8(0xfa, 0xfa, 0xfa);
const SIDEBAR_ACCENT: Color = Color::from_rgb_u8(0xec, 0xec, 0xee);
const SECONDARY: Color = Color::from_rgb_u8(0xf4, 0xf4, 0xf5);
const PRIMARY: Color = Color::from_rgb_u8(0x18, 0x18, 0x1b);
const PRIMARY_FOREGROUND: Color = Color::from_rgb_u8(0xfa, 0xfa, 0xfa);
const STRIPE: Color = Color(0.5, 0.5, 0.5, 0.03);
const SUCCESS: Color = Color::from_rgb_u8(0x15, 0x80, 0x3d);
const DANGER: Color = Color::from_rgb_u8(0xdc, 0x26, 0x26);
const SERIES: [Color; 3] = [
    Color::from_rgb_u8(0xca, 0x8a, 0x04),
    Color::from_rgb_u8(0x93, 0x33, 0xea),
    Color::from_rgb_u8(0x02, 0x84, 0xc7),
];
const RADIUS_SM: f32 = 4.0;
const RADIUS: f32 = 6.0;

fn faded(color: Color, alpha: f32) -> Color {
    Color(color.0, color.1, color.2, color.3 * alpha)
}

fn change_color(change: f64) -> Color {
    if change >= 0.0 { SUCCESS } else { DANGER }
}

/// Text in the showcase's type scale: `size` px with Tailwind's line height
/// for it.
fn style(size: f32, color: Color, weight: Option<FontWeight>) -> TextStyle {
    let line = match size as u32 {
        0..=12 => 16.0,
        13..=14 => 20.0,
        15..=20 => 28.0,
        _ => 32.0,
    };
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size),
            font_weight: weight,
            ..Default::default()
        },
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Em(line / size),
            ..Default::default()
        },
    }
}

fn one_line() -> TextOptions {
    TextOptions {
        overflow: TextOverflow::Clip,
        soft_wrap: false,
        max_lines: Some(1),
        ..Default::default()
    }
}

/// A single line of text.
#[composable]
fn Label(text: impl Into<String>, modifier: Modifier, text_style: TextStyle) {
    TextWithOptions(text.into(), modifier, text_style, one_line());
}

fn xs(color: Color) -> TextStyle {
    style(12.0, color, None)
}

fn sm(color: Color) -> TextStyle {
    style(14.0, color, None)
}

/// Draws a hairline along each side `sides` names: left, top, right, bottom.
fn hairlines(modifier: Modifier, sides: [bool; 4]) -> Modifier {
    modifier.draw_behind(move |scope| {
        let size = scope.size();
        let [left, top, right, bottom] = sides;
        let brush = Brush::solid(BORDER);
        let line = |x: f32, y: f32, width: f32, height: f32| Rect {
            x,
            y,
            width,
            height,
        };
        if left {
            scope.draw_rect_at(line(0.0, 0.0, 1.0, size.height), brush.clone());
        }
        if top {
            scope.draw_rect_at(line(0.0, 0.0, size.width, 1.0), brush.clone());
        }
        if right {
            scope.draw_rect_at(line(size.width - 1.0, 0.0, 1.0, size.height), brush.clone());
        }
        if bottom {
            scope.draw_rect_at(line(0.0, size.height - 1.0, size.width, 1.0), brush);
        }
    })
}

fn row_spec(gap: f32) -> RowSpec {
    RowSpec::new()
        .horizontal_arrangement(LinearArrangement::spaced_by(gap))
        .vertical_alignment(VerticalAlignment::CenterVertically)
}

fn centered() -> BoxSpec {
    BoxSpec::new().content_alignment(Alignment::CENTER)
}

// Number formats, as the showcase writes them.

/// `value` to `decimals` places, its thousands grouped: `1,053.980`.
fn grouped(value: f64, decimals: usize) -> String {
    let text = format!("{:.*}", decimals, value.abs());
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    let mut out = String::with_capacity(text.len() + whole.len() / 3 + 1);
    if value < 0.0 && text.bytes().any(|b| (b'1'..=b'9').contains(&b)) {
        out.push('-');
    }
    for (ix, digit) in whole.chars().enumerate() {
        if ix > 0 && (whole.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if !fraction.is_empty() {
        out.push('.');
        out.push_str(fraction);
    }
    out
}

fn price(value: f64) -> String {
    grouped(value, 3)
}

fn signed(value: f64) -> String {
    let text = grouped(value, 3);
    if text.starts_with('-') {
        text
    } else {
        format!("+{text}")
    }
}

fn percent(value: f64) -> String {
    format!("{value:+.2}%")
}

fn amount(value: f64) -> String {
    if value >= 1e9 {
        format!("{:.2}B", value / 1e9)
    } else if value >= 1e6 {
        format!("{:.2}M", value / 1e6)
    } else if value >= 1e3 {
        format!("{:.2}K", value / 1e3)
    } else {
        format!("{value:.0}")
    }
}

// The market: every symbol's quote as a state of its own, and the state
// each quote panel draws from.

#[derive(Clone, Copy, PartialEq)]
struct Quote {
    last: f64,
    prev_close: f64,
    open: f64,
    high: f64,
    low: f64,
    volume: u64,
    turnover: f64,
    pre_market: f64,
    high_52: f64,
    low_52: f64,
    float_shares: f64,
    average_volume: f64,
}

#[derive(Clone, Copy)]
struct QuoteEvent {
    symbol: usize,
    last: f64,
    volume: u64,
}

#[derive(Clone, Copy, PartialEq)]
struct Candle {
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    volume: f64,
}

#[derive(Clone, Copy, PartialEq)]
struct Trade {
    time: u32,
    price: f64,
    size: u64,
    buy: bool,
}

const NAME_WORDS: [&str; 16] = [
    "Pacific",
    "Golden",
    "Northern",
    "Silver",
    "Eastern",
    "United",
    "Harbor",
    "Summit",
    "Crystal",
    "Pioneer",
    "Atlas",
    "Meridian",
    "Evergreen",
    "Horizon",
    "Sterling",
    "Orion",
];
const NAME_KINDS: [&str; 10] = [
    "Technology",
    "Energy",
    "Holdings",
    "Motors",
    "Semiconductor",
    "Pharmaceuticals",
    "Bank",
    "Networks",
    "Materials",
    "Logistics",
];
const MARKETS: [&str; 7] = ["US", "HK", "US", "SH", "US", "HK", "SG"];

/// Compared by identity: one market backs the whole workspace.
struct Market {
    quotes: Vec<MutableState<Quote>>,
    names: Vec<Rc<str>>,
    codes: Vec<Rc<str>>,
    /// Each symbol's day before its last point, which follows its quote.
    sparks: Vec<[f32; SPARK_POINTS]>,
    candles: RefCell<VecDeque<Candle>>,
    chart_quotes: Cell<usize>,
    chart: MutableState<u64>,
    /// The order book's mid price and how many quotes moved it.
    book: MutableState<(f64, usize)>,
    trades: RefCell<VecDeque<Trade>>,
    tape: MutableState<u64>,
    buckets: MutableState<[f64; 6]>,
    status: MutableState<usize>,
    /// Quotes each panel that is not shown has received.
    hidden: Vec<Cell<usize>>,
    ticks: Cell<usize>,
}

impl PartialEq for Market {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Market {
    fn new() -> Self {
        let quotes: Vec<Quote> = (0..SYMBOLS)
            .map(|ix| {
                let base = 5.0 + (ix * 37 % 1_400) as f64 + (ix % 7) as f64 * 0.13;
                let prev_close = base * (1.0 + ((ix % 11) as f64 - 5.0) / 300.0);
                let volume = 1_000_000 + (ix as u64 * 7_919_113) % 90_000_000;
                Quote {
                    last: base,
                    prev_close,
                    open: prev_close * 1.002,
                    high: base * 1.02,
                    low: base * 0.98,
                    volume,
                    turnover: volume as f64 * base,
                    pre_market: prev_close * (1.0 + ((ix % 9) as f64 - 4.0) / 400.0),
                    high_52: base * 1.6,
                    low_52: base * 0.55,
                    float_shares: 2e8 + (ix as f64 * 3.7e7) % 9e9,
                    average_volume: volume as f64 * (0.6 + (ix % 5) as f64 * 0.2),
                }
            })
            .collect();
        let letter = |n: usize| char::from(b'A' + (n % 26) as u8);
        let selected = quotes[SELECTED];
        let mut close = selected.prev_close;
        let candles = (0..CANDLES)
            .map(|ix| {
                let open = close;
                let step = ((ix * 7 % 13) as f64 - 6.0) / 300.0;
                close = open * (1.0 + step);
                let reach = ((ix * 5 % 7) as f64 + 1.0) / 800.0;
                Candle {
                    open,
                    high: open.max(close) * (1.0 + reach),
                    low: open.min(close) * (1.0 - reach),
                    close,
                    volume: 2e6 + ((ix * 7_919) % 5_000) as f64 * 1e3,
                }
            })
            .collect();
        let trades = (0..TRADES)
            .map(|ix| Trade {
                time: 37_800 - ix as u32 * 3,
                price: selected.last + ((ix * 7 % 11) as f64 - 5.0) * 0.01,
                size: 100 * (1 + (ix as u64 * 37) % 60),
                buy: ix % 3 != 0,
            })
            .collect();
        Self {
            sparks: quotes
                .iter()
                .enumerate()
                .map(|(ix, quote)| {
                    std::array::from_fn(|point| {
                        let wave = ((point * 7 + ix * 3) % 17) as f64 - 8.0;
                        (quote.prev_close * (1.0 + wave / 400.0)) as f32
                    })
                })
                .collect(),
            names: (0..SYMBOLS)
                .map(|ix| {
                    Rc::from(format!(
                        "{} {}",
                        NAME_WORDS[ix % NAME_WORDS.len()],
                        NAME_KINDS[ix * 7 % NAME_KINDS.len()]
                    ))
                })
                .collect(),
            codes: (0..SYMBOLS)
                .map(|ix| match MARKETS[ix % MARKETS.len()] {
                    "HK" => Rc::from(format!("{:05}", ix * 97 % 9_999)),
                    "SH" => Rc::from(format!("6{:05}", ix * 131 % 99_999)),
                    _ => {
                        let mut code: String = [
                            letter(ix * 7 + 3),
                            letter(ix * 11 + 5),
                            letter(ix / 26 + ix * 3),
                        ]
                        .into_iter()
                        .collect();
                        if ix % 3 != 0 {
                            code.push(letter(ix * 5));
                        }
                        Rc::from(code)
                    }
                })
                .collect(),
            quotes: quotes.into_iter().map(mutableStateOf).collect(),
            candles: RefCell::new(candles),
            chart_quotes: Cell::new(0),
            chart: mutableStateOf(0),
            book: mutableStateOf((selected.last, 0)),
            trades: RefCell::new(trades),
            tape: mutableStateOf(0),
            buckets: mutableStateOf([4.2e8, 2.9e8, 1.1e8, 1.3e8, 2.4e8, 3.8e8]),
            status: mutableStateOf(0),
            hidden: (0..HIDDEN_PANELS).map(|_| Cell::new(0)).collect(),
            ticks: Cell::new(0),
        }
    }

    /// One tick of the stream: each quote updates its symbol and goes out to
    /// every panel. The indices tick every thirtieth time.
    fn tick(&self, quotes_per_tick: usize) {
        let tick = self.ticks.get() + 1;
        self.ticks.set(tick);
        for i in 0..quotes_per_tick {
            let symbol = quote_symbol(tick, i);
            let event = self.quotes[symbol].update(|quote| {
                let step = ((tick * 31 + symbol * 17) % 21) as f64 - 10.0;
                quote.last = (quote.last * (1.0 + step / 2_000.0)).max(0.01);
                quote.high = quote.high.max(quote.last);
                quote.low = quote.low.min(quote.last);
                let traded = 100 + (tick as u64 % 9) * 100;
                quote.volume += traded;
                quote.turnover += traded as f64 * quote.last;
                QuoteEvent {
                    symbol,
                    last: quote.last,
                    volume: quote.volume,
                }
            });
            self.emit(event);
        }
        if tick.is_multiple_of(30) {
            self.status.update(|tick| *tick += 1);
        }
    }

    /// Hands a quote to every panel subscribed to the feed.
    fn emit(&self, event: QuoteEvent) {
        for panel in &self.hidden {
            panel.set(panel.get() + 1);
        }
        if event.symbol != SELECTED {
            return;
        }
        let quotes = self.chart_quotes.get() + 1;
        self.chart_quotes.set(quotes);
        {
            let mut candles = self.candles.borrow_mut();
            if quotes.is_multiple_of(QUOTES_PER_CANDLE)
                && let Some(close) = candles.back().map(|candle| candle.close)
            {
                candles.pop_front();
                candles.push_back(Candle {
                    open: close,
                    high: close,
                    low: close,
                    close,
                    volume: 0.0,
                });
            }
            if let Some(candle) = candles.back_mut() {
                candle.close = event.last;
                candle.high = candle.high.max(event.last);
                candle.low = candle.low.min(event.last);
                candle.volume += (event.volume % 1_000) as f64 * 100.0;
            }
        }
        self.chart.update(|version| *version += 1);
        self.book.update(|(mid, updates)| {
            *mid = event.last;
            *updates += 1;
        });
        {
            let mut trades = self.trades.borrow_mut();
            if let Some(newest) = trades.front().copied() {
                trades.push_front(Trade {
                    time: newest.time + 1,
                    price: event.last,
                    size: 100 * (1 + event.volume % 60),
                    buy: event.last >= newest.price,
                });
                trades.truncate(TRADES);
            }
        }
        self.tape.update(|version| *version += 1);
        self.buckets.update(|buckets| {
            buckets[event.volume as usize % 6] += (event.volume % 1_000) as f64 * 1e3
        });
    }
}

/// Which symbol the `i`th quote of a tick is for: the quote panels' symbol
/// first every other tick, then rows on the watchlist's first screen, then
/// the rest.
fn quote_symbol(tick: usize, i: usize) -> usize {
    match i {
        0 if tick.is_multiple_of(2) => SELECTED,
        1..=4 => (tick * 5 + i * 11) % FIRST_SCREEN,
        _ => (tick * 7 + i * 13) % SYMBOLS,
    }
}

// The showcase's frame.

/// The width and height of gpui-fast's showcase window, in points.
const WINDOW: (f32, f32) = (1280.0, 820.0);

/// The workspace at the showcase window's size in points. A screen of another
/// width draws it at the density that makes it span that width, so every
/// device lays out the same 1280 × 820 points: the rows, panels and text a
/// frame draws are the showcase's own.
#[composable]
pub fn WorkspaceFrame(mode: WorkspaceMode) {
    BoxWithConstraints(Modifier::empty().fill_max_size(), move |scope| {
        // gpui_perf's window keeps its size; a narrower screen shows all of
        // it scaled down, as the Compose app does.
        let scale = (scope.max_width().0 / WINDOW.0).min(1.0);
        let canvas = Modifier::empty()
            .wrap_content_size(Alignment::TOP_START, true)
            .size_points(WINDOW.0, WINDOW.1);
        let canvas = if scale < 1.0 {
            canvas.graphics_layer_value(GraphicsLayer {
                scale,
                transform_origin: TransformOrigin::new(0.0, 0.0),
                ..GraphicsLayer::default()
            })
        } else {
            canvas
        };
        Box(canvas, BoxSpec::default(), move || WorkspaceScreen(mode));
    });
}

#[composable]
pub fn WorkspaceScreen(mode: WorkspaceMode) {
    let market = remember(|| Rc::new(Market::new())).with(|market| market.clone());
    let watchlist = rememberLazyListState();
    let fps = rememberMutableStateOf(|| 0.0f32);

    let stream = market.clone();
    LaunchedEffectAsync(mode as u8, move |scope| {
        std::boxed::Box::pin(async move {
            let quotes = mode.quotes_per_tick();
            while scope.is_active() {
                delay(STREAM_EVERY).await;
                if !scope.is_active() {
                    break;
                }
                stream.tick(quotes);
            }
        })
    });
    let ticked = market.clone();
    LaunchedEffectAsync(mode as u8, move |scope| {
        std::boxed::Box::pin(async move {
            let clock = scope.runtime().frame_clock();
            let mut direction = 1.0f32;
            let mut window_start = clock.next_frame().await;
            let mut window_frames = 0u32;
            while scope.is_active() {
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                if let Some(observe) = FRAME_OBSERVER.get() {
                    observe(ticked.ticks.get() as u64);
                }
                window_frames += 1;
                // The frame-stats bar samples twice a second, as the
                // showcase's does.
                if now.saturating_sub(window_start) >= 500_000_000 {
                    fps.set(window_frames as f32 * 1e9 / now.saturating_sub(window_start) as f32);
                    window_start = now;
                    window_frames = 0;
                }
                if mode == WorkspaceMode::Scroll {
                    if !watchlist.can_scroll_forward_non_reactive() {
                        direction = -1.0;
                    } else if !watchlist.can_scroll_backward_non_reactive() {
                        direction = 1.0;
                    }
                    watchlist.dispatch_scroll_delta(-SCROLL_STEP * direction);
                }
            }
        })
    });

    Column(
        Modifier::empty().fill_max_size().background(BACKGROUND),
        ColumnSpec::default(),
        move || {
            ShowcaseToolbar();
            let market = market.clone();
            Row(
                Modifier::empty().fill_max_width().weight(1.0),
                RowSpec::default(),
                move || {
                    GallerySidebar();
                    let market = market.clone();
                    Column(
                        Modifier::empty().weight(1.0).fill_max_height(),
                        ColumnSpec::default(),
                        move || {
                            PageHeader();
                            Workspace(market.clone(), watchlist);
                        },
                    );
                },
            );
            FrameStats(fps);
        },
    );
}

#[composable]
fn ShowcaseToolbar() {
    Row(
        hairlines(
            Modifier::empty()
                .fill_max_width()
                .height(40.0)
                .padding_horizontal(16.0),
            [false, false, false, true],
        ),
        row_spec(16.0),
        || {
            Box(
                Modifier::empty()
                    .height(24.0)
                    .background(SUCCESS)
                    .rounded_corners(RADIUS)
                    .padding_horizontal(8.0),
                centered(),
                || {
                    Label(
                        "cranpose",
                        Modifier::empty(),
                        style(14.0, Color::WHITE, None),
                    );
                },
            );
            Spacer(Modifier::empty().weight(1.0));
            Label("Auto-scroll", Modifier::empty(), sm(MUTED_FOREGROUND));
            Row(
                Modifier::empty()
                    .background(MUTED)
                    .rounded_corners(RADIUS)
                    .padding(2.0),
                row_spec(0.0),
                || {
                    for (ix, label) in ["Off", "Sidebar", "Page", "Table", "List", "Watchlist"]
                        .into_iter()
                        .enumerate()
                    {
                        let selected = ix == 0;
                        Box(
                            Modifier::empty().height(24.0).padding_horizontal(8.0).then(
                                if selected {
                                    Modifier::empty()
                                        .background(BACKGROUND)
                                        .rounded_corners(RADIUS_SM)
                                } else {
                                    Modifier::empty()
                                },
                            ),
                            centered(),
                            move || {
                                Label(
                                    label,
                                    Modifier::empty(),
                                    sm(if selected {
                                        FOREGROUND
                                    } else {
                                        MUTED_FOREGROUND
                                    }),
                                );
                            },
                        );
                    }
                },
            );
            Box(
                Modifier::empty().size_points(1.0, 16.0).background(BORDER),
                BoxSpec::default(),
                || {},
            );
            for (label, on) in [
                ("Refresh data", false),
                ("Stream quotes", true),
                ("Retained views", true),
            ] {
                Row(Modifier::empty(), row_spec(8.0), move || {
                    Box(
                        Modifier::empty()
                            .size_points(28.0, 16.0)
                            .background(if on { PRIMARY } else { INPUT })
                            .rounded_corners(8.0),
                        BoxSpec::new().content_alignment(if on {
                            Alignment::CENTER_END
                        } else {
                            Alignment::CENTER_START
                        }),
                        || {
                            Box(
                                Modifier::empty()
                                    .padding(2.0)
                                    .size_points(12.0, 12.0)
                                    .background(BACKGROUND)
                                    .rounded_corners(6.0),
                                BoxSpec::default(),
                                || {},
                            );
                        },
                    );
                    Label(label, Modifier::empty(), sm(FOREGROUND));
                });
            }
        },
    );
}

const GROUPS: [(&str, usize); 12] = [
    ("Getting started", 6),
    ("Inputs", 32),
    ("Buttons", 18),
    ("Data display", 36),
    ("Feedback", 22),
    ("Navigation", 24),
    ("Overlays", 20),
    ("Layout", 22),
    ("Forms", 26),
    ("Charts", 16),
    ("Media", 12),
    ("Applications", 9),
];
const PAGE_WORDS: [&str; 12] = [
    "Accordion",
    "Badge",
    "Calendar",
    "Dropdown",
    "Editor",
    "Form",
    "Grid",
    "Hover card",
    "Input",
    "Kbd",
    "Label",
    "Menu",
];

#[composable]
fn GallerySidebar() {
    let scroll = cranpose_ui::rememberScrollState!();
    Column(
        hairlines(
            Modifier::empty()
                .width(256.0)
                .fill_max_height()
                .background(SIDEBAR),
            [false, false, true, false],
        ),
        ColumnSpec::default(),
        move || {
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .vertical_scroll(scroll, false)
                    .padding_each(8.0, 0.0, 8.0, 16.0),
                ColumnSpec::default(),
                || {
                    let mut page = 0usize;
                    for (title, pages) in GROUPS {
                        Label(
                            title,
                            Modifier::empty().padding_each(8.0, 16.0, 8.0, 4.0),
                            style(12.0, MUTED_FOREGROUND, None),
                        );
                        for _ in 0..pages {
                            let selected = page == 241;
                            let name = if selected {
                                "Trading workspace".to_string()
                            } else {
                                format!("{} {}", PAGE_WORDS[page % PAGE_WORDS.len()], page / 12 + 1)
                            };
                            Box(
                                Modifier::empty()
                                    .fill_max_width()
                                    .height(28.0)
                                    .padding_horizontal(8.0)
                                    .then(if selected {
                                        Modifier::empty()
                                            .background(SIDEBAR_ACCENT)
                                            .rounded_corners(RADIUS)
                                    } else {
                                        Modifier::empty()
                                    }),
                                BoxSpec::new().content_alignment(Alignment::CENTER_START),
                                move || {
                                    Label(
                                        name.clone(),
                                        Modifier::empty(),
                                        style(14.0, FOREGROUND, None),
                                    );
                                },
                            );
                            page += 1;
                        }
                    }
                },
            );
            Label(
                "3 unread",
                hairlines(
                    Modifier::empty()
                        .fill_max_width()
                        .padding_symmetric(16.0, 8.0),
                    [false, true, false, false],
                ),
                xs(MUTED_FOREGROUND),
            );
        },
    );
}

#[composable]
fn PageHeader() {
    Column(
        hairlines(
            Modifier::empty()
                .fill_max_width()
                .padding_symmetric(24.0, 16.0),
            [false, false, false, true],
        ),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        || {
            Label(
                "Trading workspace",
                Modifier::empty(),
                style(20.0, FOREGROUND, Some(FontWeight::BOLD)),
            );
            Label(
                "Docked market panels, every one subscribed to a feed of streaming quotes.",
                Modifier::empty(),
                sm(MUTED_FOREGROUND),
            );
        },
    );
}

#[composable]
fn FrameStats(fps: MutableState<f32>) {
    Row(
        hairlines(
            Modifier::empty()
                .fill_max_width()
                .height(24.0)
                .padding_horizontal(16.0),
            [false, true, false, false],
        ),
        row_spec(16.0),
        move || {
            Label(
                format!("{:.0} fps", fps.get()),
                Modifier::empty(),
                xs(MUTED_FOREGROUND),
            );
            Label(
                "frames drawn by cranpose",
                Modifier::empty(),
                xs(MUTED_FOREGROUND),
            );
        },
    );
}

// The workspace.

#[composable]
fn LetterIcon(letter: &'static str, side: f32, fill: Color, color: Color, round: bool) {
    Box(
        Modifier::empty()
            .size_points(side, side)
            .background(fill)
            .rounded_corners(if round { side / 2.0 } else { RADIUS_SM }),
        centered(),
        move || {
            Label(
                letter,
                Modifier::empty(),
                style(side * 0.6, color, Some(FontWeight::BOLD)),
            );
        },
    );
}

#[composable]
fn Chip(label: &'static str, selected: bool, vertical: f32, text_size: f32) {
    Box(
        Modifier::empty()
            .padding_symmetric(8.0, vertical)
            .then(if selected {
                Modifier::empty()
                    .background(SECONDARY)
                    .rounded_corners(RADIUS_SM)
            } else {
                Modifier::empty()
            }),
        BoxSpec::default(),
        move || {
            Label(
                label,
                Modifier::empty(),
                if selected {
                    style(text_size, FOREGROUND, None)
                } else {
                    style(text_size, MUTED_FOREGROUND, None)
                },
            );
        },
    );
}

#[composable]
fn Workspace(market: Rc<Market>, watchlist: LazyListState) {
    Column(
        Modifier::empty().fill_max_width().weight(1.0),
        ColumnSpec::default(),
        move || {
            WorkspaceToolbar();
            let dock = market.clone();
            Row(
                Modifier::empty().fill_max_width().weight(1.0),
                RowSpec::default(),
                move || {
                    IconSidebar();
                    Dock(dock.clone(), watchlist);
                },
            );
            StatusBar(market.status);
        },
    );
}

#[composable]
fn WorkspaceToolbar() {
    Row(
        hairlines(
            Modifier::empty()
                .fill_max_width()
                .height(40.0)
                .padding_horizontal(12.0),
            [false, false, false, true],
        ),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        || {
            Row(Modifier::empty(), row_spec(4.0), || {
                Box(
                    Modifier::empty().padding_each(0.0, 0.0, 8.0, 0.0),
                    BoxSpec::default(),
                    || LetterIcon("T", 24.0, PRIMARY, PRIMARY_FOREGROUND, false),
                );
                for (ix, title) in [
                    "Watchlist",
                    "Markets",
                    "Portfolio",
                    "Trade",
                    "Screener",
                    "News",
                    "Community",
                ]
                .into_iter()
                .enumerate()
                {
                    Chip(title, ix == 0, 4.0, 14.0);
                }
            });
            Row(
                Modifier::empty()
                    .width(288.0)
                    .height(28.0)
                    .border(1.0, FOREGROUND, RoundedCornerShape::uniform(RADIUS))
                    .padding_horizontal(8.0),
                row_spec(0.0),
                || Label("Search symbols", Modifier::empty(), sm(MUTED_FOREGROUND)),
            );
            Row(Modifier::empty(), row_spec(12.0), || {
                Row(Modifier::empty(), row_spec(4.0), || {
                    Box(
                        Modifier::empty()
                            .size_points(8.0, 8.0)
                            .background(SUCCESS)
                            .rounded_corners(4.0),
                        BoxSpec::default(),
                        || {},
                    );
                    Label("US Market Open", Modifier::empty(), xs(FOREGROUND));
                });
                LetterIcon("!", 20.0, SECONDARY, MUTED_FOREGROUND, false);
                LetterIcon("*", 20.0, SECONDARY, MUTED_FOREGROUND, false);
                Label("Account A/C(1637)", Modifier::empty(), xs(MUTED_FOREGROUND));
                LetterIcon("J", 24.0, SERIES[1], Color::WHITE, true);
            });
        },
    );
}

#[composable]
fn IconSidebar() {
    Column(
        hairlines(
            Modifier::empty()
                .width(48.0)
                .fill_max_height()
                .background(SIDEBAR)
                .padding_vertical(8.0),
            [false, false, true, false],
        ),
        ColumnSpec::new()
            .vertical_arrangement(LinearArrangement::spaced_by(4.0))
            .horizontal_alignment(HorizontalAlignment::CenterHorizontally),
        || {
            for (ix, letter) in ["W", "M", "P", "T", "S", "N", "C", "A", "L", "O"]
                .into_iter()
                .enumerate()
            {
                Box(
                    Modifier::empty().size_points(36.0, 36.0).then(if ix == 0 {
                        Modifier::empty()
                            .background(SIDEBAR_ACCENT)
                            .rounded_corners(RADIUS)
                    } else {
                        Modifier::empty()
                    }),
                    centered(),
                    move || {
                        LetterIcon(
                            letter,
                            18.0,
                            Color::TRANSPARENT,
                            if ix == 0 {
                                FOREGROUND
                            } else {
                                MUTED_FOREGROUND
                            },
                            false,
                        )
                    },
                );
            }
            Spacer(Modifier::empty().weight(1.0));
            LetterIcon("?", 18.0, Color::TRANSPARENT, MUTED_FOREGROUND, false);
        },
    );
}

/// The dock: the watchlist, then a column of the quote and its chart, then
/// a column of the order book, time and sales and trade statistics.
#[composable]
fn Dock(market: Rc<Market>, watchlist: LazyListState) {
    Row(
        Modifier::empty().weight(1.0).fill_max_height(),
        RowSpec::default(),
        move || {
            let m = market.clone();
            Column(
                Modifier::empty().weight(1.0).fill_max_height(),
                ColumnSpec::default(),
                move || {
                    let m = m.clone();
                    Panel(
                        1.0,
                        [true, false],
                        vec!["Watchlist", "Positions"],
                        PanelContent(Rc::new(move || Watchlist(m.clone(), watchlist))),
                    );
                },
            );
            let m = market.clone();
            Column(
                Modifier::empty().width(460.0).fill_max_height(),
                ColumnSpec::default(),
                move || {
                    let detail = m.clone();
                    Panel(
                        1.0,
                        [true, true],
                        vec!["Quote", "Profile"],
                        PanelContent(Rc::new(move || QuoteDetail(detail.clone()))),
                    );
                    let chart = m.clone();
                    Panel(
                        1.2,
                        [true, false],
                        vec!["Candlestick", "Intraday"],
                        PanelContent(Rc::new(move || Chart(chart.clone()))),
                    );
                },
            );
            let m = market.clone();
            Column(
                Modifier::empty().width(380.0).fill_max_height(),
                ColumnSpec::default(),
                move || {
                    let book = m.clone();
                    Panel(
                        1.0,
                        [false, true],
                        vec!["Order Book"],
                        PanelContent(Rc::new(move || OrderBook(book.clone()))),
                    );
                    let tape = m.clone();
                    Panel(
                        1.6,
                        [false, true],
                        vec!["Time & Sales", "News"],
                        PanelContent(Rc::new(move || TimeAndSales(tape.clone()))),
                    );
                    let stats = m.clone();
                    Panel(
                        0.7,
                        [false, false],
                        vec!["Statistics"],
                        PanelContent(Rc::new(move || TradeStats(stats.market_buckets()))),
                    );
                },
            );
        },
    );
}

impl Market {
    fn market_buckets(&self) -> MutableState<[f64; 6]> {
        self.buckets
    }
}

/// What a dock slot shows, compared by identity.
#[derive(Clone)]
struct PanelContent(Rc<dyn Fn()>);

impl PartialEq for PanelContent {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// A dock slot: a tab bar over its active panel. `lines` says whether it
/// draws the hairline to its right and below it.
#[composable]
fn Panel(grow: f32, lines: [bool; 2], tabs: Vec<&'static str>, content: PanelContent) {
    let [right, bottom] = lines;
    Column(
        hairlines(
            Modifier::empty().fill_max_width().weight(grow),
            [false, false, right, bottom],
        )
        .clip_to_bounds(),
        ColumnSpec::default(),
        move || {
            let tabs = tabs.clone();
            Row(
                hairlines(
                    Modifier::empty()
                        .fill_max_width()
                        .height(32.0)
                        .background(MUTED),
                    [false, false, false, true],
                ),
                row_spec(0.0),
                move || {
                    for (ix, title) in tabs.iter().copied().enumerate() {
                        WorkspaceTab(title, ix == 0);
                    }
                },
            );
            let content = content.clone();
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .background(BACKGROUND)
                    .clip_to_bounds(),
                BoxSpec::default(),
                move || (content.0)(),
            );
        },
    );
}

/// One tab in a dock panel's strip. The active tab has no hover style; an
/// inactive tab tints to `BORDER` (gpui's `secondary_hover`) while the
/// pointer rests over it.
#[composable]
fn WorkspaceTab(title: &'static str, active: bool) {
    let interaction = rememberMutableInteractionSource();
    let hovered = collect_is_hovered_as_state(&interaction);
    Box(
        Modifier::empty()
            .fill_max_height()
            .padding_horizontal(12.0)
            .hoverable(interaction, !active)
            .then(if active {
                Modifier::empty().background(BACKGROUND)
            } else if hovered.get() {
                Modifier::empty().background(BORDER)
            } else {
                Modifier::empty()
            }),
        centered(),
        move || {
            Label(
                title,
                Modifier::empty(),
                if active {
                    style(14.0, FOREGROUND, None)
                } else {
                    sm(MUTED_FOREGROUND)
                },
            );
        },
    );
}

/// The watchlist's columns: a title, a width, and whether the values are
/// numbers, aligned to the trailing edge.
const COLUMNS: [(&str, f32, bool); 15] = [
    ("Symbol", 92.0, false),
    ("Name", 200.0, false),
    ("Price", 84.0, true),
    ("% Chg", 76.0, true),
    ("Chg", 76.0, true),
    ("Volume", 128.0, true),
    ("Turnover", 84.0, true),
    ("Turnover %", 88.0, true),
    ("Vol Ratio", 76.0, true),
    ("High", 84.0, true),
    ("Low", 84.0, true),
    ("Pre-Mkt", 84.0, true),
    ("Pre-Mkt %", 84.0, true),
    ("52wk Range", 108.0, false),
    ("Trend", 100.0, false),
];
const SORTED_BY: usize = 3;
/// A row's width: every column and the row's padding.
const TABLE_WIDTH: f32 = 1448.0 + 8.0;

/// One cell of a watchlist row: its column's width, padded, the content
/// trailing-aligned for numbers.
#[composable]
fn Cell_(column: usize, gap: f32, content: impl FnMut() + 'static) {
    let (_, width, numeric) = COLUMNS[column];
    Row(
        Modifier::empty()
            .width(width)
            .fill_max_height()
            .padding_horizontal(8.0)
            .clip_to_bounds(),
        RowSpec::new()
            .horizontal_arrangement(if numeric {
                LinearArrangement::spaced_by_aligned(gap, HorizontalAlignment::End)
            } else {
                LinearArrangement::spaced_by(gap)
            })
            .vertical_alignment(VerticalAlignment::CenterVertically),
        content,
    );
}

#[composable]
fn Watchlist(market: Rc<Market>, state: LazyListState) {
    let scroll = cranpose_ui::rememberScrollState!();
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .height(32.0)
                    .padding_horizontal(4.0),
                row_spec(4.0),
                || {
                    for (ix, group) in [
                        "All 200",
                        "US Stocks",
                        "HK Stocks",
                        "China A",
                        "Singapore",
                        "Holdings",
                        "ETFs",
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        Chip(group, ix == 0, 2.0, 12.0);
                    }
                    Spacer(Modifier::empty().weight(1.0));
                    Label(
                        "Sorted by % Chg",
                        Modifier::empty().padding_each(0.0, 0.0, 8.0, 0.0),
                        xs(MUTED_FOREGROUND),
                    );
                },
            );
            let market = market.clone();
            Column(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .horizontal_scroll_without_gestures(scroll, false),
                ColumnSpec::default(),
                move || {
                    WatchlistHeader();
                    let market = market.clone();
                    LazyColumn(
                        Modifier::empty().width(TABLE_WIDTH).weight(1.0),
                        state,
                        LazyColumnSpec::new(),
                        move |scope| {
                            let market = market.clone();
                            scope.items(
                                LazyItems::new(SYMBOLS)
                                    .key(|index| index as u64)
                                    .content_type(|_| 0),
                                move |ix| WatchlistRow(market.clone(), ix),
                            );
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn WatchlistHeader() {
    Row(
        hairlines(
            Modifier::empty()
                .width(TABLE_WIDTH)
                .height(32.0)
                .background(MUTED)
                .padding_horizontal(4.0),
            [false, true, false, true],
        ),
        row_spec(0.0),
        || {
            for (column, (title, _, sortable)) in COLUMNS.iter().copied().enumerate() {
                Cell_(column, 4.0, move || {
                    Label(
                        title,
                        Modifier::empty(),
                        style(12.0, MUTED_FOREGROUND, None),
                    );
                    if sortable {
                        SortArrows(column == SORTED_BY);
                    }
                });
            }
        },
    );
}

/// The two small triangles of a sortable column, the one sorted by filled.
#[composable]
fn SortArrows(sorted_down: bool) {
    Box(
        Modifier::empty()
            .size_points(6.0, 12.0)
            .draw_behind(move |scope| {
                let (width, middle) = (scope.size().width, scope.size().height / 2.0);
                for up in [true, false] {
                    let base = middle + if up { -1.0 } else { 1.0 };
                    let tip = if up { base - 4.0 } else { base + 4.0 };
                    let mut path = Path::new();
                    path.move_to(Point::new(0.0, base));
                    path.line_to(Point::new(width, base));
                    path.line_to(Point::new(width / 2.0, tip));
                    path.close();
                    let color = if !up && sorted_down { FOREGROUND } else { BORDER };
                    scope.draw_path(&path, Brush::solid(color), DrawStyle::Fill);
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn WatchlistRow(market: Rc<Market>, ix: usize) {
    let quote = market.quotes[ix].get();
    let change = quote.last - quote.prev_close;
    let color = change_color(change);
    let pre_change = quote.pre_market - quote.prev_close;
    let pre_color = change_color(pre_change);
    let volume = quote.volume as f64;
    let numbers: [(String, Option<Color>); 11] = [
        (price(quote.last), Some(color)),
        (percent(change / quote.prev_close * 100.0), Some(color)),
        (signed(change), Some(color)),
        (format!("{} shares", amount(volume)), None),
        (amount(quote.turnover), None),
        (format!("{:.2}%", volume / quote.float_shares * 100.0), None),
        (format!("{:.2}", volume / quote.average_volume), None),
        (price(quote.high), None),
        (price(quote.low), None),
        (price(quote.pre_market), Some(pre_color)),
        (
            percent(pre_change / quote.prev_close * 100.0),
            Some(pre_color),
        ),
    ];
    let range =
        ((quote.last - quote.low_52) / (quote.high_52 - quote.low_52)).clamp(0.0, 1.0) as f32;
    let mut spark = market.sparks[ix];
    spark[SPARK_POINTS - 1] = quote.last as f32;
    let baseline = quote.prev_close as f32;
    let code = market.codes[ix].clone();
    let name = market.names[ix].clone();
    let interaction = rememberMutableInteractionSource();
    let hovered = collect_is_hovered_as_state(&interaction);
    Row(
        Modifier::empty()
            .width(TABLE_WIDTH)
            .height(ROW_HEIGHT)
            .hoverable(interaction, true)
            .draw_behind(move |scope| {
                let size = scope.size();
                if hovered.get() {
                    scope.draw_rect(Brush::solid(MUTED));
                } else if ix % 2 == 1 {
                    scope.draw_rect(Brush::solid(STRIPE));
                }
                scope.draw_rect_at(
                    Rect {
                        x: 0.0,
                        y: size.height - 1.0,
                        width: size.width,
                        height: 1.0,
                    },
                    Brush::solid(BORDER),
                );
            })
            .padding_horizontal(4.0),
        row_spec(0.0),
        move || {
            let code = code.clone();
            Cell_(0, 4.0, move || {
                Box(
                    Modifier::empty()
                        .background(SECONDARY)
                        .rounded_corners(RADIUS_SM)
                        .padding_horizontal(2.0),
                    BoxSpec::default(),
                    move || {
                        Label(
                            MARKETS[ix % MARKETS.len()],
                            Modifier::empty(),
                            xs(MUTED_FOREGROUND),
                        )
                    },
                );
                Label(
                    code.to_string(),
                    Modifier::empty(),
                    style(14.0, FOREGROUND, None),
                );
            });
            let name = name.clone();
            Cell_(1, 8.0, move || {
                LetterIcon(
                    ["A", "B", "C", "D", "E", "F", "G", "H"][ix % 8],
                    18.0,
                    SECONDARY,
                    MUTED_FOREGROUND,
                    true,
                );
                Label(
                    name.to_string(),
                    Modifier::empty().weight(1.0),
                    sm(FOREGROUND),
                );
                for (shown, letter) in [
                    (ix.is_multiple_of(4), "H"),
                    (ix % 3 == 1, "P"),
                    (ix % 5 == 2, "O"),
                ] {
                    if shown {
                        Box(
                            Modifier::empty().size_points(16.0, 16.0).border(
                                1.0,
                                INPUT,
                                RoundedCornerShape::uniform(RADIUS_SM),
                            ),
                            centered(),
                            move || Label(letter, Modifier::empty(), xs(MUTED_FOREGROUND)),
                        );
                    }
                }
            });
            for (offset, (text, text_color)) in numbers.iter().cloned().enumerate() {
                Cell_(offset + 2, 0.0, move || {
                    Label(
                        text.clone(),
                        Modifier::empty(),
                        sm(text_color.unwrap_or(FOREGROUND)),
                    );
                });
            }
            Cell_(13, 0.0, move || {
                Box(
                    Modifier::empty()
                        .fill_max_width()
                        .height(8.0)
                        .draw_behind(move |scope| {
                            let size = scope.size();
                            let bar = Rect {
                                x: 0.0,
                                y: 2.0,
                                width: size.width,
                                height: 4.0,
                            };
                            scope.draw_round_rect_at(
                                bar,
                                Brush::solid(MUTED),
                                CornerRadii::uniform(2.0),
                            );
                            scope.draw_round_rect_at(
                                Rect {
                                    width: size.width * range,
                                    ..bar
                                },
                                Brush::solid(INPUT),
                                CornerRadii::uniform(2.0),
                            );
                            scope.draw_rect_at(
                                Rect {
                                    x: size.width * range,
                                    y: 0.0,
                                    width: 2.0,
                                    height: 8.0,
                                },
                                Brush::solid(FOREGROUND),
                            );
                        }),
                    BoxSpec::default(),
                    || {},
                );
            });
            Cell_(14, 0.0, move || {
                Box(
                    Modifier::empty()
                        .fill_max_width()
                        .height(20.0)
                        .draw_behind(move |scope| draw_spark(scope, &spark, baseline, color)),
                    BoxSpec::default(),
                    || {},
                );
            });
        },
    );
}

/// A symbol's day as a line over a faint fill.
fn draw_spark(scope: &mut dyn DrawScope, points: &[f32], baseline: f32, color: Color) {
    let size = scope.size();
    let (low, high) = points
        .iter()
        .fold((baseline, baseline), |(low, high), point| {
            (low.min(*point), high.max(*point))
        });
    let span = (high - low).max(0.0001);
    let step = size.width / (points.len() - 1) as f32;
    let y = |value: f32| size.height * ((high - value) / span);
    let mut area = Path::new();
    area.move_to(Point::new(0.0, size.height));
    let mut line = Path::new();
    for (ix, value) in points.iter().enumerate() {
        let point = Point::new(step * ix as f32, y(*value));
        area.line_to(point);
        if ix == 0 {
            line.move_to(point);
        } else {
            line.line_to(point);
        }
    }
    area.line_to(Point::new(size.width, size.height));
    area.close();
    scope.draw_path(&area, Brush::solid(faded(color, 0.12)), DrawStyle::Fill);
    scope.draw_path(&line, Brush::solid(color), DrawStyle::Stroke(Stroke::new(1.0)));
}

#[composable]
fn QuoteDetail(market: Rc<Market>) {
    let quote = market.quotes[SELECTED].get();
    let change = quote.last - quote.prev_close;
    let color = change_color(change);
    let pre_change = quote.pre_market - quote.prev_close;
    let volume = quote.volume as f64;
    let shares = quote.float_shares * 1.08;
    let stats: [(&'static str, String); 24] = [
        ("Open", price(quote.open)),
        ("Prev. Close", price(quote.prev_close)),
        ("High", price(quote.high)),
        ("Low", price(quote.low)),
        ("Volume", format!("{} shares", amount(volume))),
        ("Turnover", amount(quote.turnover)),
        ("52wk High", price(quote.high_52)),
        ("52wk Low", price(quote.low_52)),
        ("Mkt Cap", amount(shares * quote.last)),
        ("Float Cap", amount(quote.float_shares * quote.last)),
        ("Shares", amount(shares)),
        ("Float Shares", amount(quote.float_shares)),
        ("P/E (TTM)", format!("{:.2}", quote.last / 8.83)),
        ("P/E (Static)", format!("{:.2}", quote.last / 7.91)),
        ("P/B", format!("{:.2}", quote.last / 21.4)),
        ("Dividend (TTM)", "1.060".to_string()),
        ("Div. Yield", format!("{:.2}%", 1.06 / quote.last * 100.0)),
        (
            "Turnover %",
            format!("{:.2}%", volume / quote.float_shares * 100.0),
        ),
        ("Vol Ratio", format!("{:.2}", volume / quote.average_volume)),
        (
            "Amplitude",
            format!(
                "{:.2}%",
                (quote.high - quote.low) / quote.prev_close * 100.0
            ),
        ),
        ("Avg Price", price(quote.turnover / volume)),
        ("Bid/Ask", format!("{:.2}%", 50.0 + change * 3.0)),
        ("Lot Size", "100".to_string()),
        ("Min Tick", "0.010".to_string()),
    ];
    let code = market.codes[SELECTED].clone();
    let name = market.names[SELECTED].clone();
    Column(
        Modifier::empty().fill_max_size().padding(12.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            let (code, name) = (code.clone(), name.clone());
            Row(Modifier::empty(), row_spec(8.0), move || {
                Label(
                    code.to_string(),
                    Modifier::empty(),
                    style(14.0, FOREGROUND, Some(FontWeight::BOLD)),
                );
                Label(name.to_string(), Modifier::empty(), xs(MUTED_FOREGROUND));
                Box(
                    Modifier::empty()
                        .background(faded(SUCCESS, 0.15))
                        .rounded_corners(RADIUS_SM)
                        .padding_horizontal(4.0),
                    BoxSpec::default(),
                    || Label("Trading", Modifier::empty(), xs(SUCCESS)),
                );
            });
            Row(Modifier::empty(), row_spec(8.0), move || {
                Label(
                    price(quote.last),
                    Modifier::empty(),
                    style(24.0, color, Some(FontWeight::BOLD)),
                );
                Label(signed(change), Modifier::empty(), sm(color));
                Label(
                    percent(change / quote.prev_close * 100.0),
                    Modifier::empty(),
                    sm(color),
                );
            });
            Row(Modifier::empty(), row_spec(8.0), move || {
                let pre = change_color(pre_change);
                Label("Pre-market", Modifier::empty(), xs(MUTED_FOREGROUND));
                Label(price(quote.pre_market), Modifier::empty(), xs(pre));
                Label(
                    percent(pre_change / quote.prev_close * 100.0),
                    Modifier::empty(),
                    xs(pre),
                );
                Label("04:12 EST", Modifier::empty(), xs(MUTED_FOREGROUND));
            });
            let stats = stats.clone();
            FlowRow(
                Modifier::empty().fill_max_width(),
                FlowRowSpec::new(),
                move || {
                    for (ix, (label, value)) in stats.iter().cloned().enumerate() {
                        Row(
                            Modifier::empty().fill_max_width_fraction(0.5).padding_each(
                                if ix % 2 == 1 { 12.0 } else { 0.0 },
                                2.0,
                                if ix % 2 == 0 { 12.0 } else { 0.0 },
                                2.0,
                            ),
                            RowSpec::new()
                                .horizontal_arrangement(LinearArrangement::SpaceBetween)
                                .vertical_alignment(VerticalAlignment::CenterVertically),
                            move || {
                                Label(label, Modifier::empty(), xs(MUTED_FOREGROUND));
                                Label(value.clone(), Modifier::empty(), xs(FOREGROUND));
                            },
                        );
                    }
                },
            );
        },
    );
}

const AVERAGES: [usize; 3] = [5, 10, 20];
const PRICE_AXIS: f32 = 64.0;
const TIME_AXIS: f32 = 18.0;

#[composable]
fn Chart(market: Rc<Market>) {
    let _version = market.chart.get();
    let candles: Rc<[Candle]> = market.candles.borrow().iter().copied().collect();
    let (low, high, max_volume) = candles.iter().fold(
        (f64::MAX, f64::MIN, 1.0f64),
        |(low, high, volume), candle| {
            (
                low.min(candle.low),
                high.max(candle.high),
                volume.max(candle.volume),
            )
        },
    );
    let averages: Rc<[Vec<f64>; 3]> = Rc::new(AVERAGES.map(|period| {
        candles
            .windows(period)
            .map(|window| window.iter().map(|candle| candle.close).sum::<f64>() / period as f64)
            .collect::<Vec<_>>()
    }));
    let last = candles[candles.len() - 1];
    let change = last.close - last.open;
    let span = (high - low).max(0.0001);
    let last_at = ((high - last.close) / span) as f32 * 0.75;
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .height(28.0)
                    .padding_horizontal(4.0),
                row_spec(2.0),
                || {
                    for period in ["1m", "5m", "15m", "30m", "1h", "D", "W", "M", "Y"] {
                        Chip(period, period == "D", 0.0, 12.0);
                    }
                    Spacer(Modifier::empty().weight(1.0));
                    Label(
                        "MA · VOL",
                        Modifier::empty().padding_each(0.0, 0.0, 8.0, 0.0),
                        xs(MUTED_FOREGROUND),
                    );
                },
            );
            let averages_legend = averages.clone();
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding_horizontal(12.0)
                    .clip_to_bounds(),
                row_spec(12.0),
                move || {
                    for ((average, period), color) in
                        averages_legend.iter().zip(AVERAGES).zip(SERIES)
                    {
                        let value = average.last().copied().unwrap_or_default();
                        Label(
                            format!("MA{period} {}", price(value)),
                            Modifier::empty(),
                            xs(color),
                        );
                    }
                    Label(
                        format!(
                            "O {} H {} L {} C {}",
                            price(last.open),
                            price(last.high),
                            price(last.low),
                            price(last.close)
                        ),
                        Modifier::empty(),
                        xs(change_color(change)),
                    );
                },
            );
            let (candles, averages) = (candles.clone(), averages.clone());
            Box(
                Modifier::empty()
                    .fill_max_width()
                    .weight(1.0)
                    .padding_symmetric(12.0, 8.0),
                BoxSpec::default(),
                move || {
                    let (candles, averages) = (candles.clone(), averages.clone());
                    Box(
                        Modifier::empty().fill_max_size().draw_behind(move |scope| {
                            draw_chart(scope, &candles, &averages, (low, high, max_volume))
                        }),
                        BoxSpec::default(),
                        || {},
                    );
                    // The price axis and the last price's box.
                    Column(
                        Modifier::empty()
                            .width(PRICE_AXIS)
                            .fill_max_height()
                            .align(Alignment::TOP_END)
                            .padding_each(4.0, 0.0, 0.0, TIME_AXIS),
                        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpaceBetween),
                        move || {
                            for line in 0..=4 {
                                Label(
                                    price(high - span * (line as f64 / 4.0) * 0.75),
                                    Modifier::empty(),
                                    xs(MUTED_FOREGROUND),
                                );
                            }
                        },
                    );
                    Box(
                        Modifier::empty()
                            .align(Alignment::TOP_END)
                            .offset_fraction(0.0, last_at)
                            .background(change_color(change))
                            .rounded_corners(RADIUS_SM)
                            .padding_horizontal(4.0),
                        BoxSpec::default(),
                        move || Label(price(last.close), Modifier::empty(), xs(Color::WHITE)),
                    );
                    Row(
                        Modifier::empty()
                            .fill_max_width()
                            .height(TIME_AXIS)
                            .align(Alignment::BOTTOM_START)
                            .padding_each(0.0, 0.0, PRICE_AXIS, 0.0),
                        RowSpec::new().horizontal_arrangement(LinearArrangement::SpaceBetween),
                        || {
                            for month in ["2026-04", "2026-05", "2026-06", "2026-07", "2026-08"] {
                                Label(month, Modifier::empty(), xs(MUTED_FOREGROUND));
                            }
                        },
                    );
                },
            );
        },
    );
}

fn draw_chart(
    scope: &mut dyn DrawScope,
    candles: &[Candle],
    averages: &[Vec<f64>; 3],
    (low, high, max_volume): (f64, f64, f64),
) {
    let size = scope.size();
    let plot = Size {
        width: size.width - PRICE_AXIS,
        height: size.height - TIME_AXIS,
    };
    let price_height = plot.height * 0.75;
    let volume_top = plot.height * 0.78;
    let volume_height = plot.height * 0.22;
    let span = (high - low).max(0.0001);
    let y = |value: f64| price_height * ((high - value) / span) as f32;
    let step = plot.width / candles.len() as f32;
    let x = |ix: usize| step * (ix as f32 + 0.5);
    let grid = Brush::solid(BORDER);
    for line in 0..=4 {
        let top = price_height * (line as f32 / 4.0);
        scope.draw_rect_at(
            Rect {
                x: 0.0,
                y: top,
                width: plot.width,
                height: 1.0,
            },
            grid.clone(),
        );
    }
    for line in 0..=5 {
        let left = plot.width * (line as f32 / 5.0);
        scope.draw_rect_at(
            Rect {
                x: left,
                y: 0.0,
                width: 1.0,
                height: plot.height,
            },
            grid.clone(),
        );
    }
    let body = (step * 0.7).max(1.0);
    for (ix, candle) in candles.iter().enumerate() {
        let color = if candle.close >= candle.open {
            SUCCESS
        } else {
            DANGER
        };
        let center = x(ix);
        scope.draw_rect_at(
            Rect {
                x: center - 0.5,
                y: y(candle.high),
                width: 1.0,
                height: (y(candle.low) - y(candle.high)).max(1.0),
            },
            Brush::solid(color),
        );
        let top = y(candle.open.max(candle.close));
        let bottom = y(candle.open.min(candle.close));
        scope.draw_rect_at(
            Rect {
                x: center - body / 2.0,
                y: top,
                width: body,
                height: (bottom - top).max(1.0),
            },
            Brush::solid(color),
        );
        let height = volume_height * (candle.volume / max_volume) as f32;
        scope.draw_rect_at(
            Rect {
                x: center - body / 2.0,
                y: volume_top + volume_height - height,
                width: body,
                height,
            },
            Brush::solid(faded(color, 0.5)),
        );
    }
    for (average, (period, color)) in averages.iter().zip(AVERAGES.into_iter().zip(SERIES)) {
        let mut path = Path::new();
        for (ix, value) in average.iter().enumerate() {
            let point = Point::new(x(ix + period - 1), y(*value));
            if ix == 0 {
                path.move_to(point);
            } else {
                path.line_to(point);
            }
        }
        scope.draw_path(&path, Brush::solid(color), DrawStyle::Stroke(Stroke::new(1.2)));
    }
    let close_y = y(candles[candles.len() - 1].close);
    let last_x = x(candles.len() - 1);
    let dashed = DrawStyle::DashedStroke(Stroke::new(1.0), DashPathEffect::new(&[3.0, 3.0], 0.0));
    for (start, end) in [
        (Point::new(0.0, close_y), Point::new(plot.width, close_y)),
        (Point::new(last_x, 0.0), Point::new(last_x, plot.height)),
    ] {
        let mut guide = Path::new();
        guide.move_to(start);
        guide.line_to(end);
        scope.draw_path(&guide, Brush::solid(MUTED_FOREGROUND), dashed);
    }
}

#[composable]
fn OrderBook(market: Rc<Market>) {
    let (mid, updates) = market.book.get();
    let size = move |level: usize, bid: bool| -> f64 {
        let salt = if bid { 13 } else { 7 };
        (((level * 37 + updates * salt + 11) % 900) + 20) as f64 * 100.0
    };
    let depth = |bid: bool| (0..BOOK_LEVELS).map(|ix| size(ix, bid)).sum::<f64>();
    let (bids, asks) = (depth(true), depth(false));
    let ratio = (bids / (bids + asks)) as f32;
    let largest = (0..BOOK_LEVELS)
        .flat_map(|ix| [size(ix, true), size(ix, false)])
        .fold(1.0, f64::max);
    Column(
        Modifier::empty().fill_max_size().padding(8.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            BookBalance(ratio);
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(4.0)),
                move || {
                    for bid in [true, false] {
                        Column(
                            Modifier::empty().weight(1.0),
                            ColumnSpec::default(),
                            move || {
                                for ix in 0..BOOK_LEVELS {
                                    let offset = (ix + 1) as f64 * 0.01;
                                    let level = size(ix, bid);
                                    BookLevel(
                                        ix,
                                        bid,
                                        if bid { mid - offset } else { mid + offset },
                                        level,
                                        (level / largest) as f32,
                                        (ix * 7 + updates) % 30 + 1,
                                    );
                                }
                            },
                        );
                    }
                },
            );
        },
    );
}

/// The bid share against the ask share, as a split bar between the two.
#[composable]
fn BookBalance(ratio: f32) {
    Row(
        Modifier::empty().fill_max_width().padding_horizontal(4.0),
        row_spec(8.0),
        move || {
            Label(
                format!("Bid {:.2}%", ratio * 100.0),
                Modifier::empty(),
                xs(SUCCESS),
            );
            Box(
                Modifier::empty()
                    .weight(1.0)
                    .height(4.0)
                    .draw_behind(move |scope| {
                        let size = scope.size();
                        scope.draw_round_rect(Brush::solid(DANGER), CornerRadii::uniform(2.0));
                        scope.draw_round_rect_at(
                            Rect {
                                x: 0.0,
                                y: 0.0,
                                width: size.width * ratio,
                                height: size.height,
                            },
                            Brush::solid(SUCCESS),
                            CornerRadii::uniform(2.0),
                        );
                    }),
                BoxSpec::default(),
                || {},
            );
            Label(
                format!("{:.2}% Ask", (1.0 - ratio) * 100.0),
                Modifier::empty(),
                xs(DANGER),
            );
        },
    );
}

/// One level of the book: its rank, price, size and order count over a bar
/// as long as its share of the largest level, bids growing from the right.
#[composable]
fn BookLevel(ix: usize, bid: bool, value: f64, level: f64, share: f32, orders: usize) {
    let color = if bid { SUCCESS } else { DANGER };
    Row(
        Modifier::empty()
            .fill_max_width()
            .height(20.0)
            .draw_behind(move |scope| {
                let size = scope.size();
                let width = size.width * share;
                scope.draw_rect_at(
                    Rect {
                        x: if bid { size.width - width } else { 0.0 },
                        y: 0.0,
                        width,
                        height: size.height,
                    },
                    Brush::solid(faded(color, 0.12)),
                );
            })
            .padding_horizontal(4.0),
        row_spec(8.0),
        move || {
            Label(
                format!("{}", ix + 1),
                Modifier::empty().width(16.0),
                xs(MUTED_FOREGROUND),
            );
            Label(price(value), Modifier::empty().weight(1.0), xs(color));
            Row(
                Modifier::empty().width(48.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::End),
                move || Label(amount(level), Modifier::empty(), xs(FOREGROUND)),
            );
            Row(
                Modifier::empty().width(24.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::End),
                move || {
                    Label(
                        format!("({orders})"),
                        Modifier::empty(),
                        xs(MUTED_FOREGROUND),
                    )
                },
            );
        },
    );
}

#[composable]
fn TimeAndSales(market: Rc<Market>) {
    let _version = market.tape.get();
    let trades: Rc<[Trade]> = market.trades.borrow().iter().copied().collect();
    Row(
        Modifier::empty()
            .fill_max_size()
            .padding_symmetric(12.0, 8.0),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(16.0)),
        move || {
            for half in [0..TRADES / 2, TRADES / 2..TRADES] {
                let trades = trades.clone();
                Column(
                    Modifier::empty().weight(1.0),
                    ColumnSpec::default(),
                    move || {
                        // Keyed by the trade, so a new trade on top moves the rows
                        // below instead of recomposing each with its neighbour's.
                        for trade in trades[half.clone()].iter().copied() {
                            key(trade.time, || TradeRow(trade));
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn TradeRow(trade: Trade) {
    let color = if trade.buy { SUCCESS } else { DANGER };
    Row(
        Modifier::empty().fill_max_width().height(18.0),
        row_spec(4.0),
        move || {
            Label(
                format!(
                    "{:02}:{:02}:{:02}",
                    trade.time / 3_600,
                    trade.time / 60 % 60,
                    trade.time % 60
                ),
                Modifier::empty(),
                xs(MUTED_FOREGROUND),
            );
            Row(
                Modifier::empty().weight(1.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::End),
                move || Label(price(trade.price), Modifier::empty(), xs(color)),
            );
            Row(
                Modifier::empty().width(48.0),
                RowSpec::new().horizontal_arrangement(LinearArrangement::End),
                move || {
                    Label(
                        grouped(trade.size as f64, 0),
                        Modifier::empty(),
                        xs(FOREGROUND),
                    )
                },
            );
            Box(
                Modifier::empty()
                    .size_points(2.0, 12.0)
                    .background(color)
                    .rounded_corners(1.0),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

const BUCKETS: [&str; 6] = [
    "Large buy",
    "Medium buy",
    "Small buy",
    "Small sell",
    "Medium sell",
    "Large sell",
];

#[composable]
fn TradeStats(buckets: MutableState<[f64; 6]>) {
    let values = buckets.get();
    let total: f64 = values.iter().sum();
    let colors = [
        SUCCESS,
        faded(SUCCESS, 0.7),
        faded(SUCCESS, 0.4),
        faded(DANGER, 0.4),
        faded(DANGER, 0.7),
        DANGER,
    ];
    let inflow = values[..3].iter().sum::<f64>() - values[3..].iter().sum::<f64>();
    Row(
        Modifier::empty().fill_max_size().padding(12.0),
        row_spec(16.0),
        move || {
            Box(
                Modifier::empty()
                    .size_points(112.0, 112.0)
                    .draw_behind(move |scope| {
                        let size = scope.size();
                        let center = Point::new(size.width / 2.0, size.height / 2.0);
                        let radius = size.width.min(size.height) / 2.0 - 8.0;
                        let mut start = -std::f32::consts::FRAC_PI_2;
                        for (value, color) in values.iter().zip(colors) {
                            let sweep = (value / total) as f32 * std::f32::consts::TAU;
                            scope.draw_arc(
                                Brush::solid(color),
                                center,
                                radius,
                                start,
                                sweep,
                                Stroke::new(14.0),
                            );
                            start += sweep;
                        }
                    }),
                BoxSpec::default(),
                || {},
            );
            Column(
                Modifier::empty().weight(1.0),
                ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(2.0)),
                move || {
                    for ((value, label), color) in values.iter().copied().zip(BUCKETS).zip(colors) {
                        Row(
                            Modifier::empty().fill_max_width(),
                            row_spec(8.0),
                            move || {
                                Box(
                                    Modifier::empty()
                                        .size_points(8.0, 8.0)
                                        .background(color)
                                        .rounded_corners(4.0),
                                    BoxSpec::default(),
                                    || {},
                                );
                                Label(label, Modifier::empty().weight(1.0), xs(MUTED_FOREGROUND));
                                Row(
                                    Modifier::empty().width(64.0),
                                    RowSpec::new().horizontal_arrangement(LinearArrangement::End),
                                    move || Label(amount(value), Modifier::empty(), xs(FOREGROUND)),
                                );
                                Row(
                                    Modifier::empty().width(48.0),
                                    RowSpec::new().horizontal_arrangement(LinearArrangement::End),
                                    move || {
                                        Label(
                                            format!("{:.2}%", value / total * 100.0),
                                            Modifier::empty(),
                                            xs(FOREGROUND),
                                        )
                                    },
                                );
                            },
                        );
                    }
                    Row(
                        hairlines(
                            Modifier::empty()
                                .fill_max_width()
                                .padding_each(0.0, 4.0, 0.0, 0.0),
                            [false, true, false, false],
                        ),
                        RowSpec::new().horizontal_arrangement(LinearArrangement::SpaceBetween),
                        move || {
                            Label("Net inflow", Modifier::empty(), xs(FOREGROUND));
                            Label(
                                amount(inflow.abs()),
                                Modifier::empty(),
                                xs(change_color(inflow)),
                            );
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn StatusBar(tick: MutableState<usize>) {
    let tick = tick.get();
    Row(
        hairlines(
            Modifier::empty()
                .fill_max_width()
                .height(24.0)
                .padding_horizontal(12.0)
                .clip_to_bounds(),
            [false, true, false, false],
        ),
        row_spec(16.0),
        move || {
            for (ix, name) in ["HSI", "HSCEI", "HSTECH", "SSE", "SPX", "IXIC"]
                .into_iter()
                .enumerate()
            {
                let value = 20_000.0 + (ix * 1_234 + tick * 7) as f64 * 0.37;
                let change = if ix % 2 == 0 { -1.0 } else { 1.0 } * (ix as f64 + 1.0) * 12.3;
                let color = change_color(change);
                Row(Modifier::empty(), row_spec(4.0), move || {
                    Label(name, Modifier::empty(), xs(MUTED_FOREGROUND));
                    Label(price(value), Modifier::empty(), xs(color));
                    Label(signed(change), Modifier::empty(), xs(color));
                    Label(
                        percent(change / value * 100.0),
                        Modifier::empty(),
                        xs(color),
                    );
                });
            }
            Spacer(Modifier::empty().weight(1.0));
            Row(Modifier::empty(), row_spec(4.0), || {
                Box(
                    Modifier::empty()
                        .size_points(8.0, 8.0)
                        .background(SUCCESS)
                        .rounded_corners(4.0),
                    BoxSpec::default(),
                    || {},
                );
                Label("Connected · Level 2", Modifier::empty(), xs(FOREGROUND));
            });
            Label(
                format!("10:{:02}:{:02} EST", tick / 60 % 60, tick % 60),
                Modifier::empty(),
                xs(MUTED_FOREGROUND),
            );
        },
    );
}
