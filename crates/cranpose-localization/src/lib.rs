#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod catalog;
mod deferred;
mod locale;
mod message;

fn new_bundle<R: std::borrow::Borrow<fluent_bundle::FluentResource>>(
    language: unic_langid::LanguageIdentifier,
) -> fluent_bundle::FluentBundle<R> {
    let mut bundle = fluent_bundle::FluentBundle::new(vec![language]);
    bundle
        .add_builtins()
        .expect("a fresh bundle has no conflicting functions");
    bundle
}
#[cfg(feature = "tooling")]
pub mod tooling;

pub use catalog::{Catalog, Resource, Translator};
pub use deferred::DeferredMessage;
/// Values accepted by generated message accessors, including borrowed text and numbers.
pub use fluent_bundle::FluentValue;
pub use locale::{Locale, PreviewMode};
pub use message::{Argument, Message, SourceCatalog};

/// A catalog, locale, or message could not be used.
#[derive(Debug, thiserror::Error)]
pub enum LocalizationError {
    /// The requested language tag is invalid.
    #[error("invalid language tag `{0}`")]
    InvalidLocale(String),
    /// A Fluent resource is malformed or contains duplicate definitions.
    #[error("invalid catalog {resource}: {detail}")]
    InvalidResource {
        /// Locale and namespace of the failing resource.
        resource: String,
        /// Parser or validation diagnostics.
        detail: String,
    },
    /// Neither a translation nor the source could be formatted.
    #[error("cannot format {namespace}/{id}: {detail}")]
    Format {
        /// Package owning the message.
        namespace: String,
        /// Stable message identifier.
        id: String,
        /// Formatter diagnostics.
        detail: String,
    },
}
