use std::{collections::HashMap, io::Read, path::Path};

use hyphenation::{Hyphenator as _, Language, Load, Standard};

/// Why a hyphenation dictionary could not be registered.
#[derive(thiserror::Error, Debug)]
pub enum HyphenationError {
    /// The locale tag names no language that has hyphenation patterns.
    #[error("no hyphenation patterns for the locale {0}")]
    UnsupportedLocale(String),
    /// The dictionary could not be read or parsed.
    #[error("hyphenation dictionary for {locale}: {message}")]
    LoadFailed {
        /// The locale tag as it was given.
        locale: String,
        /// What the dictionary loader reported.
        message: String,
    },
}

/// Hyphenation dictionaries by language, for `Hyphens.Auto`. A dictionary
/// the app registers comes first; with the `hyphenation-embedded` feature,
/// the one built in for a language loads the first time text in it
/// hyphenates.
#[derive(Default)]
pub struct Hyphenator {
    // `None` for a language with no dictionary, so it is looked for once.
    dictionaries: HashMap<Language, Option<Standard>>,
}

impl Hyphenator {
    /// Registers the dictionary for the BCP 47 tag `locale` read from
    /// `reader`, in place of an earlier one.
    pub fn register_dictionary(
        &mut self,
        locale: &str,
        reader: &mut impl Read,
    ) -> Result<(), HyphenationError> {
        let language = language(locale)
            .ok_or_else(|| HyphenationError::UnsupportedLocale(locale.to_owned()))?;
        let dictionary = Standard::from_reader(language, reader).map_err(|error| {
            HyphenationError::LoadFailed {
                locale: locale.to_owned(),
                message: error.to_string(),
            }
        })?;
        self.dictionaries.insert(language, Some(dictionary));
        Ok(())
    }

    /// Registers the dictionary for the BCP 47 tag `locale` read from the
    /// file at `path`, in place of an earlier one.
    pub fn register_dictionary_path(
        &mut self,
        locale: &str,
        path: impl AsRef<Path>,
    ) -> Result<(), HyphenationError> {
        let mut file = std::fs::File::open(path).map_err(|error| HyphenationError::LoadFailed {
            locale: locale.to_owned(),
            message: error.to_string(),
        })?;
        self.register_dictionary(locale, &mut file)
    }

    /// Appends the byte offsets inside `word` where it may end a line with a
    /// hyphen, ascending, by the dictionary of the BCP 47 tag `locale`,
    /// American English when there is none. Appends none when no dictionary
    /// of that language is registered or built in.
    pub fn break_points(&mut self, locale: Option<&str>, word: &str, points: &mut Vec<usize>) {
        let Some(language) = language(locale.unwrap_or_default()) else {
            return;
        };
        let dictionary = self
            .dictionaries
            .entry(language)
            .or_insert_with(|| embedded(language));
        if let Some(dictionary) = dictionary {
            points.extend(dictionary.hyphenate(word).breaks);
        }
    }
}

#[cfg(feature = "hyphenation-embedded")]
fn embedded(language: Language) -> Option<Standard> {
    Standard::from_embedded(language).ok()
}

#[cfg(not(feature = "hyphenation-embedded"))]
fn embedded(_language: Language) -> Option<Standard> {
    None
}

// The patterns' language of a BCP 47 tag: the tag itself when the patterns
// carry it, else its language subtag, else that language's usual patterns.
// No tag, or the undetermined `und`, means American English.
fn language(tag: &str) -> Option<Language> {
    let tag = tag.trim().replace('_', "-").to_ascii_lowercase();
    if let Some(language) = Language::try_from_code(&tag) {
        return Some(language);
    }
    let primary = tag.split('-').next().unwrap_or_default();
    Language::try_from_code(primary).or_else(|| {
        Some(match primary {
            "" | "und" | "en" => Language::EnglishUS,
            "de" => Language::German1996,
            "el" => Language::GreekMono,
            "mn" => Language::Mongolian,
            "sr" if tag.contains("latn") => Language::SerbocroatianLatin,
            "sr" => Language::SerbianCyrillic,
            "no" => Language::NorwegianBokmal,
            _ => return None,
        })
    })
}
