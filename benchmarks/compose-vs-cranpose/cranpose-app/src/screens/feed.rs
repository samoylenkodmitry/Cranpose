use std::rc::Rc;

use cranpose::LazyItems;
use cranpose::prelude::*;
use cranpose_foundation::lazy::rememberLazyListState;
use cranpose_ui::widgets::{FlowRow, FlowRowSpec};

use super::{clamped_text_options, text_style};
use crate::data::{self, BAR_COUNT, CHIP_BACKGROUND, Comment, GRADIENT_END, PALETTE, Post};

const INK: Color = Color::from_rgb_u8(0x11, 0x18, 0x27);
const MUTED: Color = Color::from_rgb_u8(0x4B, 0x55, 0x63);
/// List rows: the posts repeat, so no scroll speed reaches the end of the list
/// inside a measurement window.
const ROWS: usize = 1_000_000;

/// Everything a card draws, in dp and sp before `scale` shrinks it.
#[derive(Clone, Copy, PartialEq)]
pub struct FeedLoad {
    /// Auto-scroll speed in dp per second; 0 holds still for layout parity
    /// checks.
    pub speed: f32,
    /// Cards side by side in each list row.
    pub columns: usize,
    /// Comment rows under each card.
    pub comments: usize,
    /// Multiplies every size and text size: below 1 fits more cards on screen.
    pub scale: f32,
}

#[composable]
pub fn FeedScreen(load: FeedLoad) {
    let posts: Rc<[Rc<Post>]> =
        remember(|| data::posts().into_iter().map(Rc::new).collect::<Rc<[_]>>())
            .with(|posts| posts.clone());
    let list_state = rememberLazyListState();
    let speed = load.speed;

    LaunchedEffectAsync(speed.to_bits(), move |scope| {
        std::boxed::Box::pin(async move {
            if speed == 0.0 {
                return;
            }
            let clock = scope.runtime().frame_clock();
            let start = clock.next_frame().await;
            let mut last = start;
            let mut next_report = 5.0;
            while scope.is_active() {
                let now = clock.next_frame().await;
                if !scope.is_active() {
                    break;
                }
                let dt = now.saturating_sub(last) as f32 / 1e9;
                last = now;
                list_state.dispatch_scroll_delta(-speed * dt);
                let elapsed = now.saturating_sub(start) as f32 / 1e9;
                if elapsed >= next_report {
                    next_report += 5.0;
                    log::info!(
                        "PERF feed t={elapsed:.1} first_visible={}",
                        list_state.first_visible_item_index_non_reactive()
                    );
                }
            }
        })
    });

    let s = load.scale;
    LazyColumn(
        Modifier::empty().fill_max_width().weight(1.0),
        list_state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::spaced_by(12.0 * s))
            .content_padding(12.0 * s, 12.0 * s),
        move |scope| {
            let items = posts.clone();
            scope.items(
                LazyItems::new(ROWS)
                    .key(|index| index as u64)
                    .content_type(|_| 0),
                move |row| PostRow(items.clone(), row, load),
            );
        },
    );
}

#[composable]
fn PostRow(posts: Rc<[Rc<Post>]>, row: usize, load: FeedLoad) {
    let s = load.scale;
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding_horizontal(12.0 * s),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(12.0 * s)),
        move || {
            for column in 0..load.columns {
                let index = (row * load.columns + column) % posts.len();
                PostCard(posts[index].clone(), load.comments, s);
            }
        },
    );
}

#[composable]
fn PostCard(post: Rc<Post>, comments: usize, s: f32) {
    let thread: Rc<[Comment]> =
        remember(|| Rc::<[Comment]>::from(data::comments(post.id, comments)))
            .with(|thread| thread.clone());
    Column(
        Modifier::empty()
            .weight(1.0)
            .background(Color::WHITE)
            .rounded_corners(16.0 * s)
            .padding(16.0 * s),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::spaced_by(10.0 * s)),
        move || {
            PostHeader(post.clone(), s);
            TextWithOptions(
                post.title.clone(),
                Modifier::empty(),
                text_style(18.0 * s, INK, true),
                clamped_text_options(2),
            );
            TextWithOptions(
                post.body.clone(),
                Modifier::empty(),
                text_style(15.0 * s, Color::from_rgb_u8(0x37, 0x41, 0x51), false),
                clamped_text_options(4),
            );
            MediaChart(post.clone(), s);
            // Chips and counters wrap onto new lines on narrow cards.
            FlowRow(
                Modifier::empty(),
                FlowRowSpec::new()
                    .main_axis_spacing(8.0 * s)
                    .cross_axis_spacing(6.0 * s),
                {
                    let post = post.clone();
                    move || {
                        for (tag, color) in &post.tags {
                            Text(
                                tag.clone(),
                                Modifier::empty()
                                    .background(CHIP_BACKGROUND[*color])
                                    .rounded_corners(14.0 * s)
                                    .padding_symmetric(12.0 * s, 6.0 * s),
                                text_style(13.0 * s, GRADIENT_END[*color], false),
                            );
                        }
                    }
                },
            );
            PostActions(post.clone(), s);
            for comment in thread.iter() {
                CommentRow(
                    comment.author.clone(),
                    comment.text.clone(),
                    comment.color,
                    s,
                );
            }
        },
    );
}

#[composable]
fn CommentRow(author: String, text: String, color: usize, s: f32) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new().horizontal_arrangement(LinearArrangement::spaced_by(8.0 * s)),
        move || {
            Box(
                Modifier::empty()
                    .size_points(24.0 * s, 24.0 * s)
                    .background(PALETTE[color])
                    .rounded_corners(12.0 * s),
                BoxSpec::default(),
                || {},
            );
            Column(Modifier::empty().weight(1.0), ColumnSpec::default(), {
                let author = author.clone();
                let text = text.clone();
                move || {
                    Text(
                        author.clone(),
                        Modifier::empty(),
                        text_style(13.0 * s, INK, true),
                    );
                    Text(
                        text.clone(),
                        Modifier::empty(),
                        text_style(13.0 * s, MUTED, false),
                    );
                }
            });
        },
    );
}

#[composable]
fn PostHeader(post: Rc<Post>, s: f32) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::spaced_by(12.0 * s))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Box(
                Modifier::empty()
                    .size_points(44.0 * s, 44.0 * s)
                    .background(PALETTE[post.color])
                    .rounded_corners(22.0 * s),
                BoxSpec::new().content_alignment(Alignment::CENTER),
                {
                    let initials = post.initials.clone();
                    move || {
                        Text(
                            initials.clone(),
                            Modifier::empty(),
                            text_style(16.0 * s, Color::WHITE, true),
                        );
                    }
                },
            );
            Column(Modifier::empty().weight(1.0), ColumnSpec::default(), {
                let post = post.clone();
                move || {
                    TextWithOptions(
                        post.author.clone(),
                        Modifier::empty(),
                        text_style(16.0 * s, INK, true),
                        clamped_text_options(1),
                    );
                    TextWithOptions(
                        post.handle.clone(),
                        Modifier::empty(),
                        text_style(13.0 * s, Color::from_rgb_u8(0x6B, 0x72, 0x80), false),
                        clamped_text_options(1),
                    );
                }
            });
            Box(
                Modifier::empty()
                    .size_points(28.0 * s, 28.0 * s)
                    .background(Color::from_rgb_u8(0xE0, 0xE7, 0xFF))
                    .rounded_corners(14.0 * s),
                BoxSpec::default(),
                || {},
            );
        },
    );
}

#[composable]
fn MediaChart(post: Rc<Post>, s: f32) {
    let height = 140.0 * s;
    let gradient = Brush::vertical_gradient(
        vec![PALETTE[post.color], GRADIENT_END[post.color]],
        0.0,
        height,
    );
    let bar_brush = Brush::solid(Color::rgba(1.0, 1.0, 1.0, 0.8));
    let bubble_brush = Brush::solid(Color::rgba(1.0, 1.0, 1.0, 0.25));
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(height)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(gradient.clone(), CornerRadii::uniform(12.0 * s));
                scope.draw_circle(
                    bubble_brush.clone(),
                    Point {
                        x: size.width * 0.8,
                        y: size.height * 0.25,
                    },
                    22.0 * s,
                );
                let pad = 12.0 * s;
                let slot = (size.width - pad * 2.0) / BAR_COUNT as f32;
                let bar_width = slot * 0.6;
                let chart_height = size.height - pad * 2.0;
                for (index, value) in post.bars.iter().enumerate() {
                    let bar_height = value * chart_height;
                    scope.draw_round_rect_at(
                        Rect {
                            x: pad + index as f32 * slot + (slot - bar_width) / 2.0,
                            y: size.height - pad - bar_height,
                            width: bar_width,
                            height: bar_height,
                        },
                        bar_brush.clone(),
                        CornerRadii::uniform(3.0 * s),
                    );
                }
            }),
        BoxSpec::default(),
        || {},
    );
}

#[composable]
fn PostActions(post: Rc<Post>, s: f32) {
    FlowRow(
        Modifier::empty(),
        FlowRowSpec::new()
            .main_axis_spacing(20.0 * s)
            .cross_axis_spacing(6.0 * s),
        move || {
            for stat in &post.stats {
                let stat = stat.clone();
                Row(
                    Modifier::empty(),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::spaced_by(6.0 * s))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    move || {
                        Box(
                            Modifier::empty()
                                .size_points(18.0 * s, 18.0 * s)
                                .background(Color::from_rgb_u8(0x9C, 0xA3, 0xAF))
                                .rounded_corners(9.0 * s),
                            BoxSpec::default(),
                            || {},
                        );
                        Text(
                            stat.clone(),
                            Modifier::empty(),
                            text_style(13.0 * s, MUTED, false),
                        );
                    },
                );
            }
        },
    );
}
