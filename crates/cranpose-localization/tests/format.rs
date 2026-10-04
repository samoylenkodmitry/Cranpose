#![cfg(feature = "formatting")]

use cranpose_localization::{FormatError, Locale, LocaleFormatters};

fn formatters(locale: &str) -> LocaleFormatters {
    LocaleFormatters::new(&Locale::parse(locale).expect("valid locale")).expect("compiled ICU data")
}

#[test]
fn formats_month_year_in_the_explicit_locale() {
    assert_eq!(
        formatters("en-US")
            .format_year_month(2024, 12)
            .expect("formatting"),
        "December 2024"
    );
    assert_eq!(
        formatters("fr-FR")
            .format_year_month(2024, 12)
            .expect("formatting"),
        "décembre 2024"
    );
    assert_eq!(
        formatters("ru-RU")
            .format_year_month(2024, 12)
            .expect("formatting"),
        "декабрь 2024\u{202f}г."
    );
}

#[test]
fn formats_dates_and_numbers_using_locale_conventions() {
    let us = formatters("en-US");
    assert_eq!(
        us.format_date(2024, 12, 3).expect("formatting"),
        "Dec 3, 2024"
    );
    assert_eq!(
        us.format_decimal(12_345.5, 2).expect("formatting"),
        "12,345.50"
    );

    let french = formatters("fr-FR");
    assert_eq!(
        french.format_decimal(12_345.5, 2).expect("formatting"),
        "12\u{202f}345,50"
    );
}

#[test]
fn formats_percent_ratios_and_currency_without_converting_amounts() {
    let us = formatters("en-US");
    assert_eq!(us.format_percent(0.125, 1).expect("formatting"), "12.5%");
    assert_eq!(
        us.format_currency(12_345.67, "EUR").expect("formatting"),
        "€12,345.67"
    );

    let french = formatters("fr-FR");
    assert_eq!(
        french
            .format_currency(12_345.67, "EUR")
            .expect("formatting"),
        "12\u{202f}345,67\u{a0}€"
    );
}

#[test]
fn invalid_dates_numbers_and_currency_codes_are_reported() {
    let formatters = formatters("en-US");
    assert!(matches!(
        formatters.format_date(2024, 2, 30),
        Err(FormatError::InvalidDate { .. })
    ));
    assert!(matches!(
        formatters.format_decimal(f64::NAN, 2),
        Err(FormatError::InvalidNumber)
    ));
    assert!(matches!(
        formatters.format_percent(f64::INFINITY, 1),
        Err(FormatError::InvalidNumber)
    ));
    assert!(matches!(
        formatters.format_currency(1.0, "usd"),
        Err(FormatError::InvalidCurrency(_))
    ));
}
