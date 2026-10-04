use std::{collections::BTreeSet, fs, path::Path};

/// One ordered language entry from an application's `localization.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocaleMetadata {
    /// Canonical BCP 47 tag used by the matching catalog directory.
    pub tag: String,
    /// Name shown in the language's own writing system.
    pub native_name: String,
}

/// Loads the ordered `[[locale]]` entries from a localization manifest.
pub fn load_locale_manifest(path: &Path) -> Result<Vec<LocaleMetadata>, String> {
    let source =
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let value: toml::Table = source
        .parse()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let entries = value
        .get("locale")
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{}: expected ordered [[locale]] entries", path.display()))?;
    if entries.is_empty() {
        return Err(format!(
            "{}: at least one [[locale]] entry is required",
            path.display()
        ));
    }
    let mut tags = BTreeSet::new();
    entries
        .iter()
        .map(|entry| {
            let tag = entry
                .get("tag")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| "each [[locale]] needs a string `tag`".to_owned())?;
            let native_name = entry
                .get("native_name")
                .and_then(toml::Value::as_str)
                .ok_or_else(|| format!("locale `{tag}` needs a string `native_name`"))?;
            let parsed = crate::Locale::parse(tag).map_err(|error| error.to_string())?;
            if parsed.to_string() != tag {
                return Err(format!(
                    "locale tag `{tag}` must use canonical BCP 47 casing"
                ));
            }
            if native_name.trim().is_empty() {
                return Err(format!("locale `{tag}` has an empty native name"));
            }
            if !tags.insert(tag.to_owned()) {
                return Err(format!("duplicate locale `{tag}`"));
            }
            Ok(LocaleMetadata {
                tag: tag.to_owned(),
                native_name: native_name.to_owned(),
            })
        })
        .collect()
}
