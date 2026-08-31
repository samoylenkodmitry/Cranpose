#![allow(non_snake_case)]

use std::sync::Arc;

use cranpose::{
    composable,
    liquid::prelude::*,
    remember,
    text::{SpanStyle, TextStyle, TextUnit},
    widgets::{Box, BoxSpec, Column, ColumnSpec, LazyColumn, LazyColumnSpec, Row, RowSpec, Text},
    Brush, Color, CornerRadii, LazyItems, Modifier, Point, Size,
};
use cranpose_foundation::{
    lazy::{rememberLazyListState, LazyListScope, LazyListState},
    text::TextFieldState,
    SemanticsConfiguration,
};
use cranpose_ui::{Alignment, HorizontalAlignment, LinearArrangement, VerticalAlignment};

pub const GLASS_FEED_LIST_TAG: &str = "GlassFeedList";

const CHROME_CLEARANCE: f32 = 150.0;
const CARD_HEIGHT: f32 = 76.0;
const CARD_SPACING: f32 = 12.0;
const FEED_ROWS: usize = 120;

fn feed_glass() -> Glass {
    Glass::regular().blur_radius(0.0)
}

fn feed_button_spec() -> GlassButtonSpec {
    GlassButtonSpec::glass().with_glass(feed_glass())
}

fn feed_search_spec() -> LiquidSearchFieldSpec {
    LiquidSearchFieldSpec {
        glass: feed_glass(),
        ..Default::default()
    }
}

const CARD_GRADIENTS: [[Color; 2]; 6] = [
    [
        Color::from_rgb_u8(255, 94, 98),
        Color::from_rgb_u8(255, 175, 123),
    ],
    [
        Color::from_rgb_u8(58, 123, 213),
        Color::from_rgb_u8(0, 210, 255),
    ],
    [
        Color::from_rgb_u8(17, 153, 142),
        Color::from_rgb_u8(56, 239, 125),
    ],
    [
        Color::from_rgb_u8(142, 45, 226),
        Color::from_rgb_u8(74, 0, 224),
    ],
    [
        Color::from_rgb_u8(255, 210, 0),
        Color::from_rgb_u8(255, 105, 180),
    ],
    [
        Color::from_rgb_u8(240, 80, 174),
        Color::from_rgb_u8(94, 92, 230),
    ],
];

fn feed_text_style(size_sp: f32, color: Color) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size_sp),
            ..Default::default()
        },
        ..Default::default()
    }
}

const CARD_TITLES: [&str; 6] = [
    "Grocery run",
    "Coffee beans",
    "Hardware store",
    "Book haul",
    "Garden supply",
    "Weekly market",
];

#[composable]
pub fn GlassFeedTab() {
    LiquidTheme(LiquidThemeSpec::default(), || {
        let colors = liquid_colors();
        let list_state = rememberLazyListState();
        Box(
            Modifier::empty().fill_max_size().draw_behind(move |scope| {
                scope.draw_rect(Brush::solid(colors.background));
            }),
            BoxSpec::default(),
            move || {
                FeedList(list_state);
                FeedChrome();
            },
        );
    });
}

#[composable]
fn FeedList(list_state: LazyListState) {
    LazyColumn(
        Modifier::empty()
            .fill_max_size()
            .semantics(|config: &mut SemanticsConfiguration| {
                config.content_description = Some(GLASS_FEED_LIST_TAG.to_string());
            }),
        list_state,
        LazyColumnSpec::new()
            .vertical_arrangement(LinearArrangement::SpacedBy(CARD_SPACING))
            .content_padding(CHROME_CLEARANCE, 24.0),
        move |scope| {
            scope.items(
                LazyItems::new(FEED_ROWS).key(|index: usize| index as u64),
                move |index| FeedCard(index),
            );
        },
    );
}

#[composable]
fn FeedCard(index: usize) {
    let gradient = CARD_GRADIENTS[index % CARD_GRADIENTS.len()];
    let title = CARD_TITLES[index % CARD_TITLES.len()];
    let subtitle = Arc::new(format!("Receipt #{:04} — 12 items", index + 1));
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(CARD_HEIGHT)
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect(
                    Brush::linear_gradient_range(
                        vec![gradient[0], gradient[1]],
                        Point::new(0.0, 0.0),
                        Point::new(size.width, size.height),
                    ),
                    CornerRadii::uniform(18.0),
                );
            })
            .padding_symmetric(16.0, 12.0),
        BoxSpec::default(),
        move || {
            let subtitle = Arc::clone(&subtitle);
            Row(
                Modifier::empty().fill_max_size(),
                RowSpec::default()
                    .vertical_alignment(VerticalAlignment::CenterVertically)
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                move || {
                    let subtitle = Arc::clone(&subtitle);
                    Column(
                        Modifier::empty().weight(1.0),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
                        move || {
                            Text(
                                title,
                                Modifier::empty(),
                                feed_text_style(17.0, Color::WHITE),
                            );
                            Text(
                                String::clone(&subtitle),
                                Modifier::empty(),
                                feed_text_style(13.0, Color::WHITE.with_alpha(0.85)),
                            );
                        },
                    );
                    if index.is_multiple_of(3) {
                        GlassButton(
                            Modifier::empty().size(Size::new(44.0, 44.0)),
                            feed_button_spec(),
                            || {},
                            || {
                                Text("★", Modifier::empty(), feed_text_style(17.0, Color::WHITE));
                            },
                        );
                    }
                },
            );
        },
    );
}

#[composable]
fn FeedChrome() {
    let colors = liquid_colors();
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding_each(8.0, 8.0, 8.0, 0.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::SpacedBy(10.0)),
        move || {
            TopBar(colors.label);
            SearchBar();
            FilterPanel(colors.label);
        },
    );
}

#[composable]
fn TopBar(label: Color) {
    GlassSurface(
        Modifier::empty().fill_max_width().height(56.0),
        feed_glass(),
        move || {
            Row(
                Modifier::empty()
                    .fill_max_size()
                    .padding_symmetric(16.0, 6.0),
                RowSpec::default()
                    .vertical_alignment(VerticalAlignment::CenterVertically)
                    .horizontal_arrangement(LinearArrangement::SpacedBy(12.0)),
                move || {
                    Text(
                        "Library",
                        Modifier::empty().weight(1.0),
                        feed_text_style(22.0, label),
                    );
                    GlassButton(
                        Modifier::empty().size(Size::new(44.0, 44.0)),
                        feed_button_spec(),
                        || {},
                        || {
                            Text("⋯", Modifier::empty(), feed_text_style(20.0, Color::WHITE));
                        },
                    );
                },
            );
        },
    );
}

#[composable]
fn SearchBar() {
    let search_state = remember(|| TextFieldState::new("")).with(|state| *state);
    LiquidSearchField(
        Modifier::empty().fill_max_width(),
        search_state,
        feed_search_spec(),
    );
}

#[composable]
fn FilterPanel(label: Color) {
    Box(
        Modifier::empty().fill_max_width(),
        BoxSpec::default().content_alignment(Alignment::new(
            HorizontalAlignment::End,
            VerticalAlignment::Top,
        )),
        move || {
            GlassSurface(
                Modifier::empty().size(Size::new(139.0, 126.0)),
                feed_glass().shape(LiquidShape::RoundedRect(20.0)),
                move || {
                    Column(
                        Modifier::empty().fill_max_size().padding(12.0),
                        ColumnSpec::default()
                            .vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
                        move || {
                            for label_text in ["Newest", "By store", "By total"] {
                                Text(label_text, Modifier::empty(), feed_text_style(14.0, label));
                            }
                        },
                    );
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_feed_glass_material_explicitly_disables_blur() {
        assert_eq!(feed_glass().blur_radius, Some(0.0));
        assert_eq!(
            feed_button_spec()
                .glass
                .expect("feed buttons must override their glass material")
                .blur_radius,
            Some(0.0)
        );
        assert_eq!(feed_search_spec().glass.blur_radius, Some(0.0));
    }
}
