//! Source extraction, catalog validation, and macro expansion shared by the CLI and compiler.

mod audit;
mod call;
mod catalogs;
mod extract;
mod fixtures;
mod manifest;
mod signature;

pub use audit::{UiTextFinding, audit_ui_sources};
pub use call::TranslationCall;
pub use catalogs::{CatalogFile, load_catalogs, validate_catalogs};
pub use extract::{extract_sources, merge_source_catalog};
pub use fixtures::{
    generate_android_locale_config, generate_native_fixtures, update_ios_localizations,
};
pub use manifest::{LocaleMetadata, load_locale_manifest};
pub use signature::{MessageSignature, catalog_signatures};
