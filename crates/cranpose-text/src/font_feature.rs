use std::{
    fmt,
    hash::{Hash, Hasher},
    sync::Arc,
};

/// One OpenType feature setting: its tag and its value, 0 for off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontFeature {
    /// The feature's tag, such as `liga`.
    pub tag: [u8; 4],
    /// The value: 0 turns the feature off, 1 on, and larger values pick an
    /// alternate.
    pub value: u32,
}

/// Compose's `fontFeatureSettings`: a CSS `font-feature-settings` string,
/// such as `smcp, liga 0`, kept with the settings it parses to, so that a
/// layout reads them without parsing the string again. Two settings are
/// equal when their strings are.
#[derive(Clone)]
pub struct FontFeatureSettings(Arc<ParsedSettings>);

struct ParsedSettings {
    text: Box<str>,
    features: Box<[FontFeature]>,
}

impl FontFeatureSettings {
    /// The settings `text` writes, parsed as [`font_feature_settings`] does.
    pub fn new(text: &str) -> Self {
        Self(Arc::new(ParsedSettings {
            text: text.into(),
            features: font_feature_settings(text).collect(),
        }))
    }

    /// The string the settings are written as.
    pub fn as_str(&self) -> &str {
        &self.0.text
    }

    /// The settings, in order, invalid entries skipped.
    pub fn features(&self) -> &[FontFeature] {
        &self.0.features
    }
}

impl From<&str> for FontFeatureSettings {
    fn from(text: &str) -> Self {
        Self::new(text)
    }
}

impl From<String> for FontFeatureSettings {
    fn from(text: String) -> Self {
        Self::new(&text)
    }
}

impl From<Arc<str>> for FontFeatureSettings {
    fn from(text: Arc<str>) -> Self {
        Self::new(&text)
    }
}

impl PartialEq for FontFeatureSettings {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0.text == other.0.text
    }
}

impl Eq for FontFeatureSettings {}

impl Hash for FontFeatureSettings {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl fmt::Debug for FontFeatureSettings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("FontFeatureSettings")
            .field(&self.as_str())
            .finish()
    }
}

/// The settings of a CSS `font-feature-settings` string, Compose's
/// `fontFeatureSettings`, in order. Entries are comma separated: a quoted or
/// bare four-letter tag, then nothing or `on` for 1, `off` for 0, or a
/// number. Invalid entries are skipped.
pub fn font_feature_settings(settings: &str) -> impl Iterator<Item = FontFeature> + '_ {
    settings.split(',').filter_map(parse_font_feature)
}

#[inline(never)]
fn parse_font_feature(entry: &str) -> Option<FontFeature> {
    let (tag, rest) = split_feature_tag(entry.trim())?;
    let tag: [u8; 4] = tag.as_bytes().try_into().ok()?;
    if !tag.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
        return None;
    }
    Some(FontFeature {
        tag,
        value: parse_feature_value(rest.trim_start())?,
    })
}

fn split_feature_tag(entry: &str) -> Option<(&str, &str)> {
    match *entry.as_bytes().first()? {
        quote @ (b'"' | b'\'') => {
            if entry.as_bytes().get(5) != Some(&quote) {
                return None;
            }
            Some((entry.get(1..5)?, entry.get(6..)?))
        }
        _ => {
            let length = entry
                .bytes()
                .position(|byte| !byte.is_ascii_alphanumeric())
                .unwrap_or(entry.len());
            if length != 4 {
                return None;
            }
            entry.split_at_checked(length)
        }
    }
}

fn parse_feature_value(value: &str) -> Option<u32> {
    if value.is_empty() || value.eq_ignore_ascii_case("on") {
        return Some(1);
    }
    if value.eq_ignore_ascii_case("off") {
        return Some(0);
    }
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit())
        .then(|| value.parse().ok())
        .flatten()
}
