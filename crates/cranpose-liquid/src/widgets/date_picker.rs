//! A month calendar to pick a day in, as Compose's `DatePicker`.

use cranpose_core::{MutableState, mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui::{
    Alignment, Modifier, Point, Size, TextStyle, UiString, VerticalAlignment,
    text::TextAlign,
    widgets::{Box, BoxSpec, Column, ColumnSpec, Row, RowSpec, Text},
};
use cranpose_ui_graphics::{Brush, Stroke};

use crate::{
    icons::{CHEVRON_LEFT, CHEVRON_RIGHT, Icon},
    theme::{liquid_colors, liquid_typography},
};

/// A day of the Gregorian calendar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CalendarDate {
    pub year: i32,
    /// 1 for January through 12 for December.
    pub month: u8,
    /// 1 through the length of the month.
    pub day: u8,
}

impl CalendarDate {
    /// The date, or `None` when the month has no such day.
    pub fn new(year: i32, month: u8, day: u8) -> Option<Self> {
        (1..=12).contains(&month).then_some(())?;
        (1..=days_in_month(year, month))
            .contains(&day)
            .then_some(Self { year, month, day })
    }

    /// The date `days` days after 1970-01-01. An app reads today from its
    /// clock in its time zone and passes it here.
    pub fn from_unix_days(days: i64) -> Self {
        // Howard Hinnant's civil_from_days.
        let shifted = days + 719_468;
        let era = shifted.div_euclid(146_097);
        let day_of_era = shifted.rem_euclid(146_097);
        let year_of_era =
            (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let month_index = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u8;
        let month = if month_index < 10 {
            month_index + 3
        } else {
            month_index - 9
        } as u8;
        let year = (year_of_era + era * 400 + i64::from(month <= 2)) as i32;
        Self { year, month, day }
    }

    /// The day of the week, 1 for Monday through 7 for Sunday.
    pub fn weekday(self) -> u8 {
        // Sakamoto's method, 0 for Sunday.
        const OFFSETS: [i32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let year = if self.month < 3 {
            self.year - 1
        } else {
            self.year
        };
        let sunday_first = (year + year.div_euclid(4) - year.div_euclid(100)
            + year.div_euclid(400)
            + OFFSETS[usize::from(self.month - 1)]
            + i32::from(self.day))
        .rem_euclid(7);
        if sunday_first == 0 {
            7
        } else {
            sunday_first as u8
        }
    }
}

/// How many days `month` of `year` has.
pub fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        2 if year.rem_euclid(4) == 0
            && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0) =>
        {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// The month before or after `(year, month)`, by `step` months.
fn shift_month((year, month): (i32, u8), step: i32) -> (i32, u8) {
    let index = year * 12 + i32::from(month) - 1 + step;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u8)
}

/// Which day a [`LiquidDatePicker`] has picked and which month it shows.
#[derive(Clone, Copy, PartialEq)]
pub struct LiquidDatePickerState {
    selected: MutableState<Option<CalendarDate>>,
    month: MutableState<(i32, u8)>,
}

impl LiquidDatePickerState {
    /// The picked day.
    pub fn selected(&self) -> Option<CalendarDate> {
        self.selected.get()
    }

    /// Picks `date`, or nothing, and shows its month.
    pub fn select(&self, date: Option<CalendarDate>) {
        if let Some(date) = date {
            self.month.set((date.year, date.month));
        }
        self.selected.set(date);
    }

    /// The month the picker shows, as `(year, month)`.
    pub fn displayed_month(&self) -> (i32, u8) {
        self.month.get()
    }

    /// Shows `month` of `year`.
    pub fn show_month(&self, year: i32, month: u8) {
        self.month.set((year, month.clamp(1, 12)));
    }
}

/// Remembers a date picker's state with `selected` picked; it shows the
/// month of `selected`, else the month of `today`.
#[composable]
pub fn rememberLiquidDatePickerState(
    selected: Option<CalendarDate>,
    today: CalendarDate,
) -> LiquidDatePickerState {
    let shown = selected.unwrap_or(today);
    remember(move || LiquidDatePickerState {
        selected: mutableStateOf(selected),
        month: mutableStateOf((shown.year, shown.month)),
    })
    .with(|state| *state)
}

/// The words a calendar shows, in the app's language when the
/// `localization-formatting` feature is on and in English otherwise.
struct CalendarText {
    /// 1 for Monday through 7 for Sunday.
    first_weekday: u8,
    #[cfg(feature = "localization-formatting")]
    formatters: Option<std::rc::Rc<cranpose_ui::localization::LocaleFormatters>>,
}

const ENGLISH_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const ENGLISH_WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const ENGLISH_LONG_WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];

impl CalendarText {
    #[cfg(feature = "localization-formatting")]
    fn current() -> Self {
        let formatters = cranpose_ui::localization::local_formatters().ok();
        Self {
            first_weekday: formatters
                .as_ref()
                .map_or(7, |formatters| formatters.first_weekday()),
            formatters,
        }
    }

    #[cfg(not(feature = "localization-formatting"))]
    fn current() -> Self {
        Self { first_weekday: 7 }
    }

    fn month_title(&self, (year, month): (i32, u8)) -> String {
        #[cfg(feature = "localization-formatting")]
        if let Some(title) = self
            .formatters
            .as_deref()
            .and_then(|formatters| formatters.format_year_month(year, month).ok())
        {
            return title;
        }
        format!("{} {year}", ENGLISH_MONTHS[usize::from(month - 1)])
    }

    fn weekday(&self, date: CalendarDate) -> String {
        #[cfg(feature = "localization-formatting")]
        if let Some(name) = self.formatters.as_deref().and_then(|formatters| {
            formatters
                .format_weekday(date.year, date.month, date.day)
                .ok()
        }) {
            return name;
        }
        ENGLISH_WEEKDAYS[usize::from(date.weekday() - 1)].to_owned()
    }

    /// The date in full, as a reader reads a day.
    fn long_date(&self, date: CalendarDate) -> String {
        #[cfg(feature = "localization-formatting")]
        if let Some(text) = self.formatters.as_deref().and_then(|formatters| {
            formatters
                .format_long_date(date.year, date.month, date.day)
                .ok()
        }) {
            return text;
        }
        format!(
            "{}, {} {}, {}",
            ENGLISH_LONG_WEEKDAYS[usize::from(date.weekday() - 1)],
            ENGLISH_MONTHS[usize::from(date.month - 1)],
            date.day,
            date.year
        )
    }
}

const CELL_HEIGHT: f32 = 40.0;
const DOT_RADIUS: f32 = 18.0;

/// A month calendar to pick a day in, as Compose's `DatePicker`: the month
/// and buttons to the months around it, the weekdays from the locale's
/// first one, and the days, of which the picked one is filled and `today`
/// ringed. A tap, Space or Enter picks a day; a reader hears each day as
/// its full date and whether it is picked.
#[composable]
pub fn LiquidDatePicker(state: LiquidDatePickerState, today: CalendarDate, modifier: Modifier) {
    let text = CalendarText::current();
    let colors = liquid_colors();
    let typography = liquid_typography();
    let shown = state.displayed_month();
    let selected = state.selected();
    let title = text.month_title(shown);
    let (previous, next) = (
        UiString::PreviousMonth.resolve(),
        UiString::NextMonth.resolve(),
    );
    let first = CalendarDate {
        year: shown.0,
        month: shown.1,
        day: 1,
    };
    // Days before the first of the month, back to the week's first day.
    let lead = (first.weekday() + 7 - text.first_weekday) % 7;
    let length = days_in_month(shown.0, shown.1);
    let weeks = (u16::from(lead) + u16::from(length)).div_ceil(7);
    let weekdays: Vec<String> = (0..7)
        .map(|column| {
            // A week of 2024-01-01 (a Monday) through 2024-01-07.
            let day = 1 + (text.first_weekday - 1 + column) % 7;
            text.weekday(CalendarDate {
                year: 2024,
                month: 1,
                day,
            })
        })
        .collect();
    let days: Vec<(CalendarDate, String)> = (1..=length)
        .map(|day| {
            let date = CalendarDate {
                year: shown.0,
                month: shown.1,
                day,
            };
            (date, text.long_date(date))
        })
        .collect();
    let title_style = TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(colors.label),
            ..typography.headline.span_style.clone()
        },
        ..typography.headline.clone()
    };
    let centered = |style: &TextStyle| TextStyle {
        paragraph_style: cranpose_ui::ParagraphStyle {
            text_align: TextAlign::Center,
            ..style.paragraph_style.clone()
        },
        ..style.clone()
    };
    let weekday_style = centered(&TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(colors.secondary_label),
            ..typography.caption1.span_style.clone()
        },
        ..typography.caption1.clone()
    });
    let day_style = centered(&typography.body);
    Column(modifier, ColumnSpec::default(), move || {
        Row(
            Modifier::empty().fill_max_width(),
            RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
            {
                let (title, title_style) = (title.clone(), title_style.clone());
                let (previous, next) = (previous.clone(), next.clone());
                move || {
                    Text(
                        title.clone(),
                        Modifier::empty().weight(1.0).padding_horizontal(8.0),
                        title_style.clone(),
                    );
                    MonthButton(CHEVRON_LEFT, previous.clone(), move || {
                        let (year, month) = shift_month(state.displayed_month(), -1);
                        state.show_month(year, month);
                    });
                    MonthButton(CHEVRON_RIGHT, next.clone(), move || {
                        let (year, month) = shift_month(state.displayed_month(), 1);
                        state.show_month(year, month);
                    });
                }
            },
        );
        Row(Modifier::empty().fill_max_width(), RowSpec::default(), {
            let (weekdays, weekday_style) = (weekdays.clone(), weekday_style.clone());
            move || {
                for name in &weekdays {
                    Text(
                        name.clone(),
                        Modifier::empty()
                            .weight(1.0)
                            .padding_vertical(6.0)
                            .hide_from_accessibility(),
                        weekday_style.clone(),
                    );
                }
            }
        });
        for week in 0..weeks {
            let days = days.clone();
            let day_style = day_style.clone();
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::default(),
                move || {
                    for column in 0..7_u16 {
                        let slot = week * 7 + column;
                        let day = slot
                            .checked_sub(u16::from(lead))
                            .and_then(|index| days.get(usize::from(index)));
                        match day {
                            Some((date, description)) => DayCell(
                                *date,
                                description.clone(),
                                selected == Some(*date),
                                *date == today,
                                day_style.clone(),
                                state,
                            ),
                            None => {
                                Box(
                                    Modifier::empty().weight(1.0).height(CELL_HEIGHT),
                                    BoxSpec::default(),
                                    || {},
                                );
                            }
                        }
                    }
                },
            );
        }
    });
}

#[composable]
fn MonthButton(icon: &'static str, label: String, on_click: impl Fn() + 'static) {
    let accent = liquid_colors().accent;
    Box(
        Modifier::empty()
            .size(Size::new(44.0, 44.0))
            .clickable(move |_| on_click())
            .content_description(label),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || Icon(icon, None, 24.0, accent),
    );
}

#[composable]
fn DayCell(
    date: CalendarDate,
    description: String,
    selected: bool,
    today: bool,
    style: TextStyle,
    state: LiquidDatePickerState,
) {
    let colors = liquid_colors();
    let (accent, on_accent, label) = (colors.accent, colors.on_accent, colors.label);
    let style = TextStyle {
        span_style: cranpose_ui::SpanStyle {
            color: Some(if selected {
                on_accent
            } else if today {
                accent
            } else {
                label
            }),
            ..style.span_style.clone()
        },
        ..style
    };
    Box(
        Modifier::empty()
            .weight(1.0)
            .height(CELL_HEIGHT)
            .draw_behind(move |scope| {
                let size = scope.size();
                let center = Point::new(size.width * 0.5, size.height * 0.5);
                let radius = DOT_RADIUS.min(size.width * 0.5);
                if selected {
                    scope.draw_circle(Brush::solid(accent), center, radius);
                } else if today {
                    scope.draw_arc(
                        Brush::solid(accent),
                        center,
                        radius - 0.75,
                        0.0,
                        std::f32::consts::TAU,
                        Stroke::new(1.5),
                    );
                }
            })
            .selectable(selected, None, move || state.select(Some(date)))
            .content_description(description),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || {
            Text(
                date.day.to_string(),
                Modifier::empty().hide_from_accessibility(),
                style.clone(),
            );
        },
    );
}
