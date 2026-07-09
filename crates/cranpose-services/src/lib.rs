//! Multiplatform service abstractions used by Cranpose applications.

#![deny(unsafe_code)]

#[cfg(test)]
use cranpose_core::{location_key, Composition, MemoryApplier};

pub mod device_info;
pub mod file_picker;
pub mod haptics;
pub mod http;
pub mod image_picker;
pub mod network_status;
pub mod notifier;
#[cfg(not(target_arch = "wasm32"))]
pub mod peer;
pub mod share_sheet;
pub mod theme;
pub mod uri_handler;
pub mod writable_folder;

pub use device_info::{
    clear_platform_device_info, device_info, set_platform_device_info, DeviceInfo, DeviceInfoRef,
};
pub use file_picker::{
    clear_platform_file_picker, default_file_picker, local_file_picker, set_platform_file_picker,
    FileFilter, FilePicker, FilePickerError, FilePickerOptions, FilePickerRef, FolderStream,
    FolderStreamRef, PickedEntry, PickedEntryRef, PickedKind, PickerFuture, ProvideFilePicker,
    ResumedPick,
};
pub use haptics::{
    clear_platform_haptics, default_haptics, local_haptics, set_platform_haptics, HapticFeedback,
    Haptics, HapticsRef, ProvideHaptics,
};
pub use http::{
    default_http_client, local_http_client, map_ordered_concurrent, HttpClient, HttpClientRef,
    HttpError, HttpFuture,
};
pub use image_picker::{
    clear_platform_image_picker, default_image_picker, local_image_picker,
    set_platform_image_picker, ImagePicker, ImagePickerError, ImagePickerRef, ImageSource,
    ProvideImagePicker, IMAGE_EXTENSIONS,
};
pub use network_status::{
    clear_platform_network_monitor, network_monitor, network_status, set_platform_network_monitor,
    NetworkMonitor, NetworkMonitorRef, NetworkStatus,
};
pub use notifier::{
    clear_platform_notifier, default_notifier, local_notifier, set_platform_notifier, Notifier,
    NotifierRef, NotifyRequest, ProvideNotifier,
};
#[cfg(not(target_arch = "wasm32"))]
pub use peer::{
    content_length, fetch_range, fetch_to_writer, ByteSource, BytesSource, FetchResult, PeerError,
    PeerServer, SourceResolver,
};
pub use share_sheet::{
    clear_platform_share_sheet, default_share_sheet, local_share_sheet, set_platform_share_sheet,
    ProvideShareSheet, ShareContent, ShareError, ShareSheet, ShareSheetRef,
};
pub use theme::{
    default_system_theme, isSystemInDarkTheme, local_system_theme, ProvideSystemTheme, SystemTheme,
};
pub use uri_handler::{
    clear_platform_uri_handler, default_uri_handler, local_uri_handler, set_platform_uri_handler,
    ProvideUriHandler, UriHandler, UriHandlerError, UriHandlerRef,
};
pub use writable_folder::{
    clear_platform_writable_folder_picker, open_writable_folder, pick_writable_folder,
    set_platform_writable_folder_picker, set_writable_folder_store_factory,
    take_resumed_writable_folder, FolderError, WritableFolderPicker, WritableFolderPickerRef,
    WritableFolderStore, WritableFolderStoreRef,
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
