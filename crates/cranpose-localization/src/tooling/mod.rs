//! Source extraction, catalog validation, and macro expansion shared by the CLI and compiler.

mod call;
mod catalogs;
mod extract;
mod signature;

pub use call::TranslationCall;
pub use catalogs::{CatalogFile, load_catalogs, validate_catalogs};
pub use extract::{extract_sources, merge_source_catalog};
pub use signature::{MessageSignature, catalog_signatures};
