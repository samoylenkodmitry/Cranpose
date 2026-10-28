/// One OpenType feature setting: its tag and its value, 0 for off.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FontFeature {
    /// The feature's tag, such as `liga`.
    pub tag: [u8; 4],
    /// The value: 0 turns the feature off, 1 on, and larger values pick an
    /// alternate.
    pub value: u32,
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
