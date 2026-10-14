//! `LiquidDatePicker` and its calendar: dates and weekdays match the
//! Gregorian calendar, a tap picks a day, the month buttons turn the month,
//! the weekdays start on Sunday in English, and a reader hears each day as
//! its full date and whether it is picked.

use cranpose_liquid::prelude::*;
use cranpose_testing::{
    RobotTestRule, TestRenderer, create_headless_robot_test, placed_semantics_from_shell,
};
use cranpose_ui::Modifier;

fn date(year: i32, month: u8, day: u8) -> CalendarDate {
    CalendarDate::new(year, month, day).expect("a valid date")
}

#[test]
fn dates_and_weekdays_follow_the_gregorian_calendar() {
    assert_eq!(CalendarDate::from_unix_days(0), date(1970, 1, 1));
    assert_eq!(CalendarDate::from_unix_days(-1), date(1969, 12, 31));
    assert_eq!(CalendarDate::from_unix_days(11_017), date(2000, 3, 1));
    assert_eq!(CalendarDate::from_unix_days(20_740), date(2026, 10, 14));
    assert_eq!(date(2026, 10, 14).weekday(), 3, "a Wednesday");
    assert_eq!(date(2000, 1, 1).weekday(), 6, "a Saturday");
    assert_eq!(date(1970, 1, 1).weekday(), 4, "a Thursday");
    assert_eq!(days_in_month(2024, 2), 29);
    assert_eq!(days_in_month(1900, 2), 28);
    assert_eq!(days_in_month(2000, 2), 29);
    assert_eq!(CalendarDate::new(2026, 2, 29), None);
    assert_eq!(CalendarDate::new(2026, 13, 1), None);
}

const TODAY: CalendarDate = CalendarDate {
    year: 2026,
    month: 10,
    day: 14,
};

struct Screen {
    robot: RobotTestRule<TestRenderer>,
    state: std::rc::Rc<std::cell::Cell<Option<LiquidDatePickerState>>>,
}

fn screen() -> Screen {
    let state: std::rc::Rc<std::cell::Cell<Option<LiquidDatePickerState>>> = std::rc::Rc::default();
    let captured = std::rc::Rc::clone(&state);
    let mut robot = create_headless_robot_test(360, 480, move || {
        let captured = std::rc::Rc::clone(&captured);
        LiquidTheme(LiquidThemeSpec::default(), move || {
            let picker = rememberLiquidDatePickerState(None, TODAY);
            captured.set(Some(picker));
            LiquidDatePicker(picker, TODAY, Modifier::empty().fill_max_width());
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    Screen { robot, state }
}

impl Screen {
    fn state(&self) -> LiquidDatePickerState {
        self.state.get().expect("composed")
    }

    /// Taps the control a reader names `label`.
    fn tap(&mut self, label: &str) {
        let tree = placed_semantics_from_shell(self.robot.shell_mut()).expect("semantics");
        let bounds = tree
            .flatten()
            .into_iter()
            .find(|node| node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("a control named {label}"))
            .target_bounds();
        self.robot.click_at(
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
    }

    fn picked(&mut self) -> Vec<String> {
        let tree = placed_semantics_from_shell(self.robot.shell_mut()).expect("semantics");
        tree.flatten()
            .into_iter()
            .filter(|node| node.selected == Some(true))
            .filter_map(|node| node.label.clone())
            .collect()
    }
}

#[test]
fn a_tap_picks_the_day_and_a_reader_hears_it_picked() {
    let mut screen = screen();
    assert!(screen.robot.find_by_text("October 2026").exists());
    assert!(screen.picked().is_empty());
    screen.tap("Tuesday, October 20, 2026");
    assert_eq!(screen.state().selected(), Some(date(2026, 10, 20)));
    assert_eq!(screen.picked(), ["Tuesday, October 20, 2026"]);
}

#[test]
fn the_month_buttons_turn_the_month() {
    let mut screen = screen();
    screen.tap("Next month");
    assert_eq!(screen.state().displayed_month(), (2026, 11));
    assert!(screen.robot.find_by_text("November 2026").exists());
    screen.tap("Previous month");
    screen.tap("Previous month");
    assert_eq!(screen.state().displayed_month(), (2026, 9));
    screen.tap("Wednesday, September 30, 2026");
    assert_eq!(screen.state().selected(), Some(date(2026, 9, 30)));
}

#[test]
fn an_english_week_starts_on_sunday() {
    let mut screen = screen();
    let names: Vec<(f32, String)> = screen
        .robot
        .get_all_rects()
        .into_iter()
        .filter_map(|(rect, text)| {
            text.filter(|text| ["Sun", "Mon", "Sat"].contains(&text.as_str()))
                .map(|text| (rect.x, text))
        })
        .collect();
    let leftmost = names
        .iter()
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .expect("weekday names");
    assert_eq!(leftmost.1, "Sun");
}

#[cfg(feature = "localization-formatting")]
#[test]
fn a_german_calendar_reads_in_german_and_starts_on_monday() {
    use cranpose_ui::{Catalog, Locale, ProvideLocalization};
    let catalog = Catalog::from_resources("en", &[]).expect("catalog");
    let mut robot = create_headless_robot_test(360, 480, move || {
        ProvideLocalization(&catalog, Locale::parse("de").expect("locale"), || {
            LiquidTheme(LiquidThemeSpec::default(), || {
                let picker = rememberLiquidDatePickerState(None, TODAY);
                LiquidDatePicker(picker, TODAY, Modifier::empty().fill_max_width());
            });
        });
    });
    robot.shell_mut().set_semantics_enabled(true);
    robot.wait_for_idle();
    assert!(robot.find_by_text("Oktober 2026").exists());
    let tree = placed_semantics_from_shell(robot.shell_mut()).expect("semantics");
    let labels: Vec<String> = tree
        .flatten()
        .into_iter()
        .filter_map(|node| node.label.clone())
        .collect();
    assert!(
        labels.iter().any(|label| label == "Nächster Monat"),
        "{labels:?}"
    );
    assert!(
        labels
            .iter()
            .any(|label| label == "Dienstag, 20. Oktober 2026"),
        "{labels:?}"
    );
    let mut headers: Vec<(f32, String)> = robot
        .get_all_rects()
        .into_iter()
        .filter_map(|(rect, text)| {
            text.filter(|text| ["Mo", "Di", "Mi", "Do", "Fr", "Sa", "So"].contains(&text.as_str()))
                .map(|text| (rect.x, text))
        })
        .collect();
    headers.sort_by(|a, b| a.0.total_cmp(&b.0));
    assert_eq!(headers.first().map(|(_, name)| name.as_str()), Some("Mo"));
}
