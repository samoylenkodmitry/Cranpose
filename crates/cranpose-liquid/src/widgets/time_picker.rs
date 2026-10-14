//! A time picker of wheels, as the iOS time picker; it picks what Compose's
//! `TimePicker` picks.

use std::rc::Rc;

use cranpose_core::DisposableEffect;
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, LazyColumn, LazyColumnSpec, LazyItems, LazyListScope, LazyListState, Modifier,
    ProgressBarRangeInfo, SemanticsSetProgress, Size, TextStyle, UiString, VerticalAlignment,
    rememberLazyListStateWithPosition,
    text::TextAlign,
    widgets::{Box, BoxSpec, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, CornerRadii, Point, Rect};

use crate::theme::{liquid_colors, liquid_typography};

/// The height of one row of a wheel.
const ROW: f32 = 40.0;
/// Rows a wheel shows: the picked one in the middle and two on each side.
const VISIBLE_ROWS: usize = 5;
/// Empty rows before the first value and after the last, so both can sit
/// in the middle.
const PAD: usize = VISIBLE_ROWS / 2;

/// The value a wheel's list rests on: the row in its middle.
fn wheel_value(list: LazyListState, count: usize) -> usize {
    let index = list.first_visible_item_index();
    let past_half = list.first_visible_item_scroll_offset() > ROW / 2.0;
    (index + usize::from(past_half)).min(count.saturating_sub(1))
}

/// The time a [`LiquidTimePicker`] shows.
#[derive(Clone, Copy, PartialEq)]
pub struct LiquidTimePickerState {
    hours: LazyListState,
    minutes: LazyListState,
    periods: LazyListState,
    twelve_hour: bool,
}

impl LiquidTimePickerState {
    /// The hour, 0 through 23.
    pub fn hour(&self) -> u8 {
        if self.twelve_hour {
            (wheel_value(self.hours, 12) + 12 * wheel_value(self.periods, 2)) as u8
        } else {
            wheel_value(self.hours, 24) as u8
        }
    }

    /// The minute, 0 through 59.
    pub fn minute(&self) -> u8 {
        wheel_value(self.minutes, 60) as u8
    }

    /// Turns the wheels to `hour` (0 through 23) and `minute`.
    pub fn set_time(&self, hour: u8, minute: u8) {
        let hour = hour.min(23);
        if self.twelve_hour {
            self.hours.scroll_to_item(usize::from(hour % 12), 0.0);
            self.periods.scroll_to_item(usize::from(hour / 12), 0.0);
        } else {
            self.hours.scroll_to_item(usize::from(hour), 0.0);
        }
        self.minutes
            .scroll_to_item(usize::from(minute.min(59)), 0.0);
    }

    /// Whether the hours run through 24 rather than twice through 12.
    pub fn is_24_hour(&self) -> bool {
        !self.twelve_hour
    }
}

/// Whether the app's locale counts hours through 24.
fn locale_uses_24_hours() -> bool {
    #[cfg(feature = "localization-formatting")]
    if let Ok(formatters) = cranpose_ui::localization::local_formatters() {
        return formatters.uses_24_hour_clock();
    }
    false
}

/// The names of the two halves of a 12-hour day.
fn day_periods() -> [String; 2] {
    #[cfg(feature = "localization-formatting")]
    if let Ok(periods) = cranpose_ui::localization::local_formatters()
        .and_then(|formatters| formatters.day_periods())
    {
        return periods;
    }
    ["AM".to_owned(), "PM".to_owned()]
}

/// Remembers a time picker's state at `hour` (0 through 23) and `minute`.
/// `is_24_hour` picks the clock; `None` takes the clock of the app's
/// language with the `localization-formatting` feature, else 12 hours.
#[composable]
pub fn rememberLiquidTimePickerState(
    hour: u8,
    minute: u8,
    is_24_hour: Option<bool>,
) -> LiquidTimePickerState {
    let twelve_hour = !is_24_hour.unwrap_or_else(locale_uses_24_hours);
    let hour = hour.min(23);
    let hour_row = if twelve_hour { hour % 12 } else { hour };
    LiquidTimePickerState {
        hours: rememberLazyListStateWithPosition(usize::from(hour_row), 0.0),
        minutes: rememberLazyListStateWithPosition(usize::from(minute.min(59)), 0.0),
        periods: rememberLazyListStateWithPosition(usize::from(hour / 12), 0.0),
        twelve_hour,
    }
}

/// A time of day on wheels: hours, minutes and, on a 12-hour clock, the
/// half of the day. A wheel turns by a drag or a fling and rests on whole
/// rows; a reader hears each wheel as its name and value and adjusts it.
#[composable]
pub fn LiquidTimePicker(state: LiquidTimePickerState, modifier: Modifier) {
    let hours: Rc<[String]> = if state.twelve_hour {
        (0..12)
            .map(|hour| if hour == 0 { 12 } else { hour }.to_string())
            .collect()
    } else {
        (0..24).map(|hour: u8| hour.to_string()).collect()
    };
    let minutes: Rc<[String]> = (0..60).map(|minute: u8| format!("{minute:02}")).collect();
    let periods: Rc<[String]> = day_periods().into_iter().collect();
    let (hour_name, minute_name) = (UiString::Hour.resolve(), UiString::Minute.resolve());
    let title = liquid_typography().title3;
    let style = TextStyle {
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_align: TextAlign::Center,
            ..title.paragraph_style
        },
        ..title
    };
    Row(
        modifier,
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Wheel(
                state.hours,
                Rc::clone(&hours),
                Some(hour_name.clone()),
                64.0,
                style.clone(),
            );
            Text(
                ":",
                Modifier::empty().hide_from_accessibility(),
                style.clone(),
            );
            Wheel(
                state.minutes,
                Rc::clone(&minutes),
                Some(minute_name.clone()),
                64.0,
                style.clone(),
            );
            if state.twelve_hour {
                Wheel(
                    state.periods,
                    Rc::clone(&periods),
                    None,
                    72.0,
                    style.clone(),
                );
            }
        },
    );
}

/// The labels a wheel shows, compared by identity.
#[derive(Clone)]
struct Labels(Rc<[String]>);

impl PartialEq for Labels {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

#[composable]
fn Wheel(
    list: LazyListState,
    labels: Rc<[String]>,
    name: Option<String>,
    width: f32,
    style: TextStyle,
) {
    DisposableEffect((), move |scope| {
        // A release rests on the nearer row.
        list.set_settle_policy(Some(Rc::new(
            |proposed, _velocity| {
                if proposed > ROW / 2.0 { ROW } else { 0.0 }
            },
        )));
        scope.on_dispose(move || list.set_settle_policy(None))
    });
    let count = labels.len();
    let value = wheel_value(list, count);
    let current = labels.get(value).cloned().unwrap_or_default();
    let band = liquid_colors().fill;
    let semantics = move |config: &mut cranpose_ui::SemanticsConfiguration| {
        config.content_description.clone_from(&name);
        config.state_description = Some(current.clone());
        config.progress = Some(ProgressBarRangeInfo::new(
            value as f32,
            0.0,
            count.saturating_sub(1) as f32,
            count.saturating_sub(2) as u32,
        ));
        config.set_progress = Some(SemanticsSetProgress::new(move |next: f32| {
            let row = next.round().clamp(0.0, count.saturating_sub(1) as f32) as usize;
            list.scroll_to_item(row, 0.0);
            true
        }));
    };
    let labels = Labels(labels);
    Box(
        Modifier::empty()
            .size(Size::new(width, ROW * VISIBLE_ROWS as f32))
            .draw_behind(move |scope| {
                let size = scope.size();
                scope.draw_round_rect_at(
                    Rect::from_origin_size(
                        Point::new(0.0, ROW * PAD as f32),
                        Size::new(size.width, ROW),
                    ),
                    Brush::solid(band),
                    CornerRadii::uniform(8.0),
                );
            })
            .value_picker()
            .semantics(semantics),
        BoxSpec::default(),
        move || {
            let labels = labels.clone();
            let style = style.clone();
            LazyColumn(
                Modifier::empty().fill_max_size(),
                list,
                LazyColumnSpec::default(),
                move |scope| {
                    let labels = labels.clone();
                    let style = style.clone();
                    scope.items(LazyItems::new(count + 2 * PAD), move |row| {
                        let label = row
                            .checked_sub(PAD)
                            .and_then(|value| labels.0.get(value))
                            .cloned();
                        let style = style.clone();
                        Box(
                            Modifier::empty().fill_max_width().height(ROW),
                            BoxSpec::default().content_alignment(Alignment::CENTER),
                            move || {
                                if let Some(label) = label.clone() {
                                    Text(
                                        label,
                                        Modifier::empty().hide_from_accessibility(),
                                        style.clone(),
                                    );
                                }
                            },
                        );
                    });
                },
            );
        },
    );
}
