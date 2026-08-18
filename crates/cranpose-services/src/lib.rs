//! Multiplatform service abstractions used by Cranpose applications.

#![deny(unsafe_code)]

#[cfg(test)]
use cranpose_core::{location_key, Composition, MemoryApplier};

pub mod app_info;
pub mod audio;
pub mod background;
pub mod camera;
pub mod device_info;
pub mod file_picker;
pub mod haptics;
pub mod http;
pub mod image_picker;
pub mod launch_args;
pub mod navigation;
pub mod network_status;
pub mod notifier;
#[cfg(not(target_arch = "wasm32"))]
pub mod peer;
pub mod purchases;
pub mod share_sheet;
pub mod theme;
pub mod uri_handler;
pub mod writable_folder;

pub use app_info::{
    app_info, build_version, clear_platform_app_info, set_platform_app_info, version_name, AppInfo,
    AppInfoRef,
};
pub use audio::{
    clear_platform_audio, default_audio, local_audio, rememberSoundBank, set_platform_audio,
    AudioBus, AudioClip, AudioError, AudioPlayer, AudioPlayerRef, NoopAudioPlayer, PlaybackParams,
    ProvideAudio, SoundBank, SoundBankEntry, SoundBankFailure, SoundId, SoundSpec, VoiceId,
};
pub use background::{
    background_active, background_activity, clear_platform_background_activity,
    set_background_active, set_platform_background_activity, BackgroundActivity,
    BackgroundActivityRef,
};
pub use camera::{
    camera, clear_platform_camera, set_platform_camera, Camera, CameraError, CameraFrame,
    CameraLens, CameraRef, CameraStill, FlashMode,
};
pub use device_info::{
    clear_platform_device_info, device_info, set_platform_device_info, DeviceInfo, DeviceInfoRef,
};
pub use file_picker::{
    clear_platform_file_picker, default_file_picker, local_file_picker, set_platform_file_picker,
    FileFilter, FilePicker, FilePickerError, FilePickerOptions, FilePickerRef, FolderStream,
    FolderStreamRef, PickedEntry, PickedEntryRef, PickedKind, PickerFuture, ProvideFilePicker,
    ResumedPick, SaveFileRequest,
};
pub use haptics::{
    clear_platform_haptics, default_haptics, local_haptics, set_platform_haptics, HapticEffect,
    HapticError, HapticFeedback, HapticPattern, Haptics, HapticsRef, ProvideHaptics,
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
pub use launch_args::{
    clear_platform_launch_args, isDebuggable, is_debuggable, launch_args,
    launch_args_from_command_line, local_launch_args, set_platform_launch_args, LaunchArgValue,
    LaunchArgs, LaunchArgsRef, ProvideLaunchArgs,
};
pub use navigation::{
    back_interception_enabled, exit_requested, push_back_request, request_exit,
    set_back_interception, set_back_request_listener, take_back_requests, take_exit_request,
};
pub use network_status::{
    clear_platform_network_monitor, network_monitor, network_status, set_platform_network_monitor,
    NetworkMonitor, NetworkMonitorRef, NetworkStatus,
};
pub use notifier::{
    clear_platform_notifier, default_notifier, local_notifier, push_notification_deeplink,
    set_platform_notifier, take_notification_deeplink, Notifier, NotifierRef, NotifyRequest,
    ProvideNotifier,
};
#[cfg(not(target_arch = "wasm32"))]
pub use peer::{
    content_length, fetch_range, fetch_to_writer, ByteSource, BytesSource, FetchResult, PeerError,
    PeerServer, SourceResolver,
};
pub use purchases::{
    clear_platform_purchases, note_store_news, purchases, set_platform_purchases,
    set_store_listener, store_available, store_state, Product, PurchaseEvent, Purchases,
    PurchasesRef, StorePhase, StoreState,
};
pub use share_sheet::{
    clear_platform_share_sheet, default_share_sheet, local_share_sheet, set_platform_share_sheet,
    ProvideShareSheet, ShareContent, ShareError, ShareSheet, ShareSheetRef,
};
pub use theme::{
    clear_platform_system_theme, default_system_theme, isSystemInDarkTheme, local_system_theme,
    set_platform_system_theme, ProvideSystemTheme, SystemTheme,
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
