//! Optional locale-sensitive formatting for dates, numbers, percentages, and currencies.

use std::{collections::HashMap, sync::Mutex};

use icu_calendar::{
    Date,
    types::Weekday,
    week::{WeekInformation, WeekPreferences},
};
use icu_datetime::{
    DateTimeFormatter, DateTimeFormatterPreferences, NoCalendarFormatter, fieldsets, input::Time,
    preferences::HourCycle,
};
use icu_decimal::{
    DecimalFormatter,
    input::{Decimal, FloatPrecision},
};
use icu_experimental::dimension::{
    currency::{
        CurrencyType,
        formatter::{CurrencyFormatter, CurrencyFormatterPreferences},
    },
    percent::formatter::PercentFormatter,
};
use icu_locale::Locale as IcuLocale;

use crate::Locale;

/// A locale formatting operation could not be completed.
#[derive(Clone, Debug, thiserror::Error)]
pub enum FormatError {
    /// ICU data could not be loaded for the selected locale.
    #[error("formatting data is unavailable for locale `{locale}`: {detail}")]
    Data {
        /// Selected language tag.
        locale: String,
        /// ICU diagnostic.
        detail: String,
    },
    /// The requested date is outside the supported Gregorian calendar range.
    #[error("invalid date {year:04}-{month:02}-{day:02}")]
    InvalidDate {
        /// Gregorian year.
        year: i32,
        /// Month number, from 1 through 12.
        month: u8,
        /// Day of month.
        day: u8,
    },
    /// The requested time of day does not exist.
    #[error("invalid time {hour:02}:{minute:02}")]
    InvalidTime {
        /// Hour, from 0 through 23.
        hour: u8,
        /// Minute, from 0 through 59.
        minute: u8,
    },
    /// The numeric value or requested precision cannot be formatted.
    #[error("invalid number or precision")]
    InvalidNumber,
    /// The currency identifier is not an uppercase three-letter ISO code.
    #[error("invalid ISO currency code `{0}`")]
    InvalidCurrency(String),
}

/// Reusable ICU4X formatters bound to one explicit locale.
///
/// Construct this type when the selected formatting locale changes, then reuse it for display
/// values. The locale controls language, calendar conventions, numbering system, separators,
/// date order, and currency placement. Formatting only changes presentation; it never changes
/// stored dates, currency codes, or monetary values.
pub struct LocaleFormatters {
    locale_tag: String,
    calendar: CalendarFormatters,
    decimal: DecimalFormatter,
    percent: PercentFormatter<DecimalFormatter>,
    currency_preferences: CurrencyFormatterPreferences,
    currencies: Mutex<HashMap<CurrencyType, CurrencyFormatter<DecimalFormatter>>>,
}

/// The date and time formatters of one locale.
struct CalendarFormatters {
    month_year: DateTimeFormatter<fieldsets::YM>,
    date: DateTimeFormatter<fieldsets::YMD>,
    long_date: DateTimeFormatter<fieldsets::YMDE>,
    weekday: DateTimeFormatter<fieldsets::E>,
    /// The day a week starts on, 1 for Monday through 7 for Sunday.
    first_weekday: u8,
    time: NoCalendarFormatter<fieldsets::T>,
    /// Times on a 12-hour clock, whichever clock the locale prefers.
    twelve_hour_time: NoCalendarFormatter<fieldsets::T>,
}

impl CalendarFormatters {
    fn new(locale: &IcuLocale, tag: &str) -> Result<Self, FormatError> {
        let data = |error: icu_datetime::DateTimeFormatterLoadError| data_error(tag, error);
        let time_preferences = DateTimeFormatterPreferences::from(locale);
        let mut twelve_hour_preferences = time_preferences;
        twelve_hour_preferences.hour_cycle = Some(HourCycle::Clock12);
        Ok(Self {
            month_year: DateTimeFormatter::try_new(locale.into(), fieldsets::YM::long())
                .map_err(data)?,
            date: DateTimeFormatter::try_new(locale.into(), fieldsets::YMD::medium())
                .map_err(data)?,
            long_date: DateTimeFormatter::try_new(locale.into(), fieldsets::YMDE::long())
                .map_err(data)?,
            weekday: DateTimeFormatter::try_new(locale.into(), fieldsets::E::short())
                .map_err(data)?,
            first_weekday: weekday_number(
                WeekInformation::try_new(WeekPreferences::from(locale))
                    .map_err(|error| data_error(tag, error))?
                    .first_weekday,
            ),
            time: NoCalendarFormatter::try_new(time_preferences, fieldsets::T::hm())
                .map_err(data)?,
            twelve_hour_time: NoCalendarFormatter::try_new(
                twelve_hour_preferences,
                fieldsets::T::hm(),
            )
            .map_err(data)?,
        })
    }
}

impl LocaleFormatters {
    /// Creates reusable formatters for the selected locale.
    pub fn new(locale: &Locale) -> Result<Self, FormatError> {
        let locale_tag = locale.to_string();
        let icu_locale = locale_tag
            .parse::<IcuLocale>()
            .map_err(|error| FormatError::Data {
                locale: locale_tag.clone(),
                detail: error.to_string(),
            })?;
        let calendar = CalendarFormatters::new(&icu_locale, &locale_tag)?;
        let decimal = DecimalFormatter::try_new(icu_locale.clone().into(), Default::default())
            .map_err(|error| data_error(&locale_tag, error))?;
        let percent = PercentFormatter::try_new(icu_locale.clone().into(), Default::default())
            .map_err(|error| data_error(&locale_tag, error))?;
        let currency_preferences = CurrencyFormatterPreferences::from(icu_locale);

        Ok(Self {
            locale_tag,
            calendar,
            decimal,
            percent,
            currency_preferences,
            currencies: Mutex::new(HashMap::new()),
        })
    }

    /// Formats a Gregorian year and month with localized month names and ordering.
    pub fn format_year_month(&self, year: i32, month: u8) -> Result<String, FormatError> {
        let date = self.date_input(year, month, 1)?;
        Ok(self.calendar.month_year.format(&date).to_string())
    }

    /// Formats a Gregorian date using the locale's date order and conventions.
    pub fn format_date(&self, year: i32, month: u8, day: u8) -> Result<String, FormatError> {
        let date = self.date_input(year, month, day)?;
        Ok(self.calendar.date.format(&date).to_string())
    }

    /// Formats a Gregorian date in full, with its weekday, as a screen reader
    /// reads a day out: "Wednesday, October 14, 2026".
    pub fn format_long_date(&self, year: i32, month: u8, day: u8) -> Result<String, FormatError> {
        let date = self.date_input(year, month, day)?;
        Ok(self.calendar.long_date.format(&date).to_string())
    }

    /// The short name of the weekday a Gregorian date falls on: "Wed", "ср".
    pub fn format_weekday(&self, year: i32, month: u8, day: u8) -> Result<String, FormatError> {
        let date = self.date_input(year, month, day)?;
        Ok(self.calendar.weekday.format(&date).to_string())
    }

    /// The day the locale's week starts on, 1 for Monday through 7 for
    /// Sunday.
    pub fn first_weekday(&self) -> u8 {
        self.calendar.first_weekday
    }

    /// Formats a time of day on the locale's clock: "9:05 PM", "21:05".
    pub fn format_time(&self, hour: u8, minute: u8) -> Result<String, FormatError> {
        Ok(self
            .calendar
            .time
            .format(&time_input(hour, minute)?)
            .to_string())
    }

    /// Whether the locale's clock runs through 24 hours rather than twice
    /// through 12: 1:00 and 13:00 then show different hours.
    pub fn uses_24_hour_clock(&self) -> bool {
        let hours = |hour| {
            time_input(hour, 0)
                .map(|time| self.calendar.time.format(&time).to_string())
                .map(|text| {
                    text.chars()
                        .filter(|char| char.is_numeric())
                        .collect::<String>()
                })
        };
        hours(1).ok() != hours(13).ok()
    }

    /// The names of the two halves of a 12-hour day, before and after noon:
    /// "AM" and "PM", "오전" and "오후". They are what a 12-hour time shows
    /// beside its digits.
    pub fn day_periods(&self) -> Result<[String; 2], FormatError> {
        let period = |hour| -> Result<String, FormatError> {
            let text = self
                .calendar
                .twelve_hour_time
                .format(&time_input(hour, 0)?)
                .to_string();
            Ok(text
                .split(|char: char| char.is_numeric() || char == ':' || char == '.')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" "))
        };
        Ok([period(9)?, period(21)?])
    }

    /// Formats a decimal with locale-specific digits and separators at fixed precision.
    ///
    /// `fraction_digits` may range from zero through eighteen.
    pub fn format_decimal(&self, value: f64, fraction_digits: u8) -> Result<String, FormatError> {
        let number = decimal(value, fraction_digits)?;
        Ok(self.decimal.format(&number).to_string())
    }

    /// Formats a ratio as a localized percentage; for example, `0.125` represents 12.5%.
    ///
    /// `fraction_digits` may range from zero through eighteen.
    pub fn format_percent(&self, ratio: f64, fraction_digits: u8) -> Result<String, FormatError> {
        let percent = decimal(ratio * 100.0, fraction_digits)?;
        Ok(self.percent.format(&percent).to_string())
    }

    /// Formats a monetary amount in the locale using an uppercase ISO 4217 code.
    ///
    /// This method applies currency-specific display precision and placement. It does not
    /// convert the amount or alter its currency.
    pub fn format_currency(&self, amount: f64, currency_code: &str) -> Result<String, FormatError> {
        if currency_code.len() != 3 || !currency_code.bytes().all(|byte| byte.is_ascii_uppercase())
        {
            return Err(FormatError::InvalidCurrency(currency_code.to_owned()));
        }
        let code = CurrencyType::try_from_str(currency_code)
            .map_err(|_| FormatError::InvalidCurrency(currency_code.to_owned()))?;
        let amount = Decimal::try_from_f64(amount, FloatPrecision::RoundTrip)
            .map_err(|_| FormatError::InvalidNumber)?;
        let mut currencies = self.currencies.lock().map_err(|_| FormatError::Data {
            locale: self.locale_tag.clone(),
            detail: "currency formatter cache is unavailable".to_owned(),
        })?;
        let formatter = match currencies.entry(code) {
            std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::hash_map::Entry::Vacant(entry) => {
                let formatter = CurrencyFormatter::<DecimalFormatter>::try_new_symbol(
                    self.currency_preferences,
                    code,
                    Default::default(),
                )
                .map_err(|error| data_error(&self.locale_tag, error))?;
                entry.insert(formatter)
            }
        };
        Ok(formatter.format_fixed_decimal(&amount).to_string())
    }

    fn date_input(
        &self,
        year: i32,
        month: u8,
        day: u8,
    ) -> Result<Date<icu_calendar::Gregorian>, FormatError> {
        Date::try_new_gregorian(year, month, day).map_err(|_| FormatError::InvalidDate {
            year,
            month,
            day,
        })
    }
}

fn time_input(hour: u8, minute: u8) -> Result<Time, FormatError> {
    Time::try_new(hour, minute, 0, 0).map_err(|_| FormatError::InvalidTime { hour, minute })
}

fn weekday_number(weekday: Weekday) -> u8 {
    match weekday {
        Weekday::Monday => 1,
        Weekday::Tuesday => 2,
        Weekday::Wednesday => 3,
        Weekday::Thursday => 4,
        Weekday::Friday => 5,
        Weekday::Saturday => 6,
        Weekday::Sunday => 7,
    }
}

fn decimal(value: f64, fraction_digits: u8) -> Result<Decimal, FormatError> {
    if fraction_digits > 18 {
        return Err(FormatError::InvalidNumber);
    }
    Decimal::try_from_f64(
        value,
        FloatPrecision::Magnitude(-i16::from(fraction_digits)),
    )
    .map_err(|_| FormatError::InvalidNumber)
}

fn data_error(locale: &str, error: impl std::fmt::Display) -> FormatError {
    FormatError::Data {
        locale: locale.to_owned(),
        detail: error.to_string(),
    }
}
