//! Multiplatform service abstractions used by Cranpose applications.

#![deny(unsafe_code)]

#[cfg(test)]
use cranpose_core::{location_key, Composition, MemoryApplier};

pub mod file_picker;
pub mod http;
pub mod theme;
pub mod uri_handler;

pub use file_picker::{
    clear_platform_file_picker, default_file_picker, local_file_picker, set_platform_file_picker,
    FileFilter, FilePicker, FilePickerError, FilePickerOptions, FilePickerRef, PickedEntry,
    PickedEntryRef, PickedKind, PickerFuture, ProvideFilePicker,
};
pub use http::{
    default_http_client, local_http_client, map_ordered_concurrent, HttpClient, HttpClientRef,
    HttpError, HttpFuture,
};
pub use theme::{
    default_system_theme, isSystemInDarkTheme, local_system_theme, ProvideSystemTheme, SystemTheme,
};
pub use uri_handler::{
    default_uri_handler, local_uri_handler, ProvideUriHandler, UriHandler, UriHandlerError,
    UriHandlerRef,
};

/// Convenience alias used in unit tests.
#[cfg(test)]
pub(crate) type TestComposition = Composition<MemoryApplier>;

/// Build a composition with a simple in-memory applier and run the provided closure once.
#[cfg(test)]
pub(crate) fn run_test_composition(build: impl FnMut()) -> TestComposition {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
    composition
}
