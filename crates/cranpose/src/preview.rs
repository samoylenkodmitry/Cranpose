//! Component previews registered with the `#[preview]` attribute.
//!
//! Enable the `preview` Cargo feature. Preview functions take no arguments and
//! return `()`; they can call composables with representative fixture data.
//! The IDE selects a descriptor using `CRANPOSE_PREVIEW` when launching an
//! embedded application. Normal desktop launches keep the application's root.

use serde::Serialize;

/// A component fixture, compiled into the app and discovered without source rewriting.
#[derive(Debug, Serialize)]
pub struct Preview {
    /// Stable descriptor identifier, including the function and variant.
    pub id: &'static str,
    /// Display name of this variant.
    pub name: &'static str,
    /// Optional group shown in the preview browser.
    pub group: &'static str,
    /// Rust function name.
    pub function: &'static str,
    /// Source path supplied by the compiler.
    pub file: &'static str,
    /// One-based source line supplied by the compiler.
    pub line: u32,
    /// Suggested logical viewport width.
    pub width: u32,
    /// Suggested logical viewport height.
    pub height: u32,
    /// Suggested dark theme.
    pub dark: bool,
    /// Render entry point, invoked inside composition.
    #[serde(skip)]
    pub render: fn(),
}

inventory::collect!(Preview);

#[doc(hidden)]
pub use inventory::submit as __submit;

/// Returns compiled preview variants, ordered by group, name and identity.
pub fn registered() -> Vec<&'static Preview> {
    let mut previews: Vec<_> = inventory::iter::<Preview>.into_iter().collect();
    previews.sort_by_key(|preview| (preview.group, preview.name, preview.id));
    previews
}

/// Finds an exact descriptor or an unambiguous function/name selector.
pub fn find(selector: &str) -> Result<&'static Preview, PreviewError> {
    select(&registered(), selector)
}

/// Failure to select a compiled preview.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PreviewError {
    /// No descriptor matches the requested identity or name.
    #[error(
        "No compiled preview matches '{0}'. Enable the preview feature and annotate a parameterless composable with #[cranpose::preview]."
    )]
    Missing(String),
    /// More than one variant matches a short name.
    #[error("Preview '{0}' is ambiguous; select its full descriptor identity.")]
    Ambiguous(String),
}

fn select(previews: &[&'static Preview], selector: &str) -> Result<&'static Preview, PreviewError> {
    if let Some(found) = previews.iter().find(|preview| preview.id == selector) {
        return Ok(found);
    }
    let mut matches = previews
        .iter()
        .copied()
        .filter(|preview| preview.function == selector || preview.name == selector);
    let first = matches
        .next()
        .ok_or_else(|| PreviewError::Missing(selector.to_string()))?;
    if matches.next().is_some() {
        return Err(PreviewError::Ambiguous(selector.to_string()));
    }
    Ok(first)
}

#[cfg(test)]
#[path = "tests/preview_tests.rs"]
mod tests;
