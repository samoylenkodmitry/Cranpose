//! Optional locale-sensitive formatting for dates, numbers, percentages, and currencies.

use std::{collections::HashMap, sync::Mutex};

use icu_calendar::Date;
use icu_datetime::{DateTimeFormatter, fieldsets};
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
    month_year: DateTimeFormatter<fieldsets::YM>,
    date: DateTimeFormatter<fieldsets::YMD>,
    decimal: DecimalFormatter,
    percent: PercentFormatter<DecimalFormatter>,
    currency_preferences: CurrencyFormatterPreferences,
    currencies: Mutex<HashMap<CurrencyType, CurrencyFormatter<DecimalFormatter>>>,
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
        let month_year =
            DateTimeFormatter::try_new(icu_locale.clone().into(), fieldsets::YM::long())
                .map_err(|error| data_error(&locale_tag, error))?;
        let date = DateTimeFormatter::try_new(icu_locale.clone().into(), fieldsets::YMD::medium())
            .map_err(|error| data_error(&locale_tag, error))?;
        let decimal = DecimalFormatter::try_new(icu_locale.clone().into(), Default::default())
            .map_err(|error| data_error(&locale_tag, error))?;
        let percent = PercentFormatter::try_new(icu_locale.clone().into(), Default::default())
            .map_err(|error| data_error(&locale_tag, error))?;
        let currency_preferences = CurrencyFormatterPreferences::from(icu_locale);

        Ok(Self {
            locale_tag,
            month_year,
            date,
            decimal,
            percent,
            currency_preferences,
            currencies: Mutex::new(HashMap::new()),
        })
    }

    /// Formats a Gregorian year and month with localized month names and ordering.
    pub fn format_year_month(&self, year: i32, month: u8) -> Result<String, FormatError> {
        let date = self.date_input(year, month, 1)?;
        Ok(self.month_year.format(&date).to_string())
    }

    /// Formats a Gregorian date using the locale's date order and conventions.
    pub fn format_date(&self, year: i32, month: u8, day: u8) -> Result<String, FormatError> {
        let date = self.date_input(year, month, day)?;
        Ok(self.date.format(&date).to_string())
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
