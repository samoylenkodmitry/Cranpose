//! A time picker of wheels, as the iOS time picker; it picks what Compose's
//! `TimePicker` picks.

use std::rc::Rc;

use cranpose_macros::composable;
use cranpose_ui::{
    LazyListState, Modifier, UiString, VerticalAlignment, rememberLazyListStateWithPosition,
    widgets::{Row, RowSpec},
};
use cranpose_ui_layout::{HorizontalAlignment, LinearArrangement};

use super::wheel::{PICKER_HEIGHT, Wheel, WheelAlign, draw_band, wheel_value};
use crate::theme::liquid_colors;

// The iOS 27 time picker, read off the native accessibility frames and
// captures at 3x: wheels 5 points apart, centered as a group. The hour wheel
// is 59 points wide on a 24-hour clock and 55 on a 12-hour one, the minute
// wheel 50 and the day-half wheel 65. Minutes and day halves sit centered in
// their wheels. Hours end 26 points from the left of their wheel above and
// below the lens and 28.6 points from it inside the lens.
const GAP: f32 = 5.0;
const HOUR_WIDTH_24: f32 = 59.0;
const HOUR_WIDTH_12: f32 = 55.0;
const MINUTE_WIDTH: f32 = 50.0;
const PERIOD_WIDTH: f32 = 65.0;
const HOURS: WheelAlign = WheelAlign::End {
    outer: 26.0,
    lens: 28.6,
};

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

// Whether the app's locale counts hours through 24.
fn locale_uses_24_hours() -> bool {
    #[cfg(feature = "localization-formatting")]
    if let Ok(formatters) = cranpose_ui::localization::local_formatters() {
        return formatters.uses_24_hour_clock();
    }
    false
}

// The names of the two halves of a 12-hour day.
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
        (0..24).map(|hour: u8| format!("{hour:02}")).collect()
    };
    let minutes: Rc<[String]> = (0..60).map(|minute: u8| format!("{minute:02}")).collect();
    let periods: Rc<[String]> = day_periods().into_iter().collect();
    let (hour_name, minute_name) = (UiString::Hour.resolve(), UiString::Minute.resolve());
    let band = liquid_colors().quaternary_fill;
    let hour_width = if state.twelve_hour {
        HOUR_WIDTH_12
    } else {
        HOUR_WIDTH_24
    };
    Row(
        modifier
            .height(PICKER_HEIGHT)
            .draw_behind(move |scope| draw_band(scope, band)),
        RowSpec::default()
            .vertical_alignment(VerticalAlignment::CenterVertically)
            .horizontal_arrangement(LinearArrangement::spaced_by_aligned(
                GAP,
                HorizontalAlignment::CenterHorizontally,
            )),
        move || {
            Wheel(
                Modifier::empty().width(hour_width),
                state.hours,
                Rc::clone(&hours),
                Some(hour_name.clone()),
                HOURS,
            );
            Wheel(
                Modifier::empty().width(MINUTE_WIDTH),
                state.minutes,
                Rc::clone(&minutes),
                Some(minute_name.clone()),
                WheelAlign::Center,
            );
            if state.twelve_hour {
                Wheel(
                    Modifier::empty().width(PERIOD_WIDTH),
                    state.periods,
                    Rc::clone(&periods),
                    None,
                    WheelAlign::Center,
                );
            }
        },
    );
}
