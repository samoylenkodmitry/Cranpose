//! Multiplatform service abstractions used by Cranpose applications.

#![deny(unsafe_code)]

#[cfg(test)]
use cranpose_core::{location_key, Composition, MemoryApplier};

pub mod app_info;
pub mod app_update;
pub mod async_io;
pub mod audio;
pub mod background;
pub mod bundled_assets;
pub mod camera;
pub mod content;
pub mod device_info;
pub mod file_picker;
pub mod haptics;
pub mod host;
pub mod host_surface;
pub mod http;
pub mod image_picker;
pub mod incoming_share;
pub mod launch_args;
pub mod launcher;
pub mod media;
pub mod navigation;
pub mod network_status;
pub mod notifier;
#[cfg(not(target_arch = "wasm32"))]
pub mod peer;
pub mod power;
pub mod preferences;
pub mod purchases;
mod registry;
pub mod share_sheet;
pub mod theme;
pub mod uri_handler;
pub mod writable_folder;

pub use app_info::{
    app_info, build_version, clear_platform_app_info, set_platform_app_info, version_name, AppInfo,
    AppInfoRef,
};
pub use app_update::{
    app_update_status, app_updates_supported, check_for_app_update, clear_platform_app_updater,
    install_app_update, observe_app_update_status, set_app_update_status, set_platform_app_updater,
    sha256_hex, verify_package, AppUpdateError, AppUpdateObserver, AppUpdateStatus, AppUpdater,
    AppUpdaterRef, DigestAlgorithm, DigestVerifier, GitHubReleaseUpdate, PackageDigest,
    UpdatePackage,
};
pub use async_io::{ChunkChannel, ChunkNext, ChunkStream, Signal, SignalWait, MAX_PENDING_CHUNKS};
pub use audio::{
    clear_platform_audio, default_audio, local_audio, rememberSoundBank, set_platform_audio,
    AudioBus, AudioClip, AudioError, AudioPlayer, AudioPlayerRef, NoopAudioPlayer, PlaybackParams,
    ProvideAudio, SoundBank, SoundBankEntry, SoundBankFailure, SoundId, SoundSpec, VoiceId,
};
pub use background::{
    acquire_background_work, background_active, background_activity,
    clear_platform_background_activity, set_platform_background_activity, BackgroundActivity,
    BackgroundActivityRef, BackgroundWorkLease,
};
pub use bundled_assets::{
    bundled_assets, clear_platform_bundled_assets, set_platform_bundled_assets, BundledAssetError,
    BundledAssetReader, BundledAssets, BundledAssetsRef,
};
#[cfg(not(target_arch = "wasm32"))]
pub use bundled_assets::{
    install_bundled_asset_set, BundledAssetEntry, BundledAssetInstallOutcome,
    BundledAssetInstallSpec,
};
pub use camera::{
    camera, camera_state, camera_supported, capture_camera_still, clear_platform_camera,
    dropped_camera_frames, latest_camera_frame, observe_camera_frames, observe_camera_state,
    observe_camera_stills, publish_camera_frame, publish_camera_state, publish_camera_still,
    record_dropped_camera_frame, rememberCameraFrames, rememberCameraState, rememberCameraStills,
    request_camera_still, set_platform_camera, start_camera, stop_camera, Camera, CameraError,
    CameraFrame, CameraLens, CameraObserver, CameraRef, CameraState, CameraStill, FlashMode,
    FrameFormat,
};
pub use content::{
    clear_platform_content_resolver, collect_stream, drain_reader, folder_files, resolve_content,
    set_platform_content_resolver, write_all, BytesContent, Content, ContentChannel, ContentEntry,
    ContentError, ContentFolder, ContentFolderRef, ContentFuture, ContentHandle, ContentMetadata,
    ContentReader, ContentReaderRef, ContentResolver, ContentResolverRef, ContentSink,
    ContentSinkRef, ContentStream, ContentStreamRef, ReadyFolder, DEFAULT_CHUNK_LEN,
};
#[cfg(not(target_arch = "wasm32"))]
pub use content::{file_content, file_folder, FileContent, FileFolder, FileSink};
pub use device_info::{
    clear_platform_device_info, device_info, release_free_memory, set_platform_device_info,
    DeviceInfo, DeviceInfoRef,
};
pub use file_picker::{
    clear_platform_file_picker, default_file_picker, local_file_picker, set_platform_file_picker,
    FileFilter, FilePicker, FilePickerError, FilePickerOptions, FilePickerRef, PickerFuture,
    ProvideFilePicker, RecoveredPick, SaveDocumentRequest,
};
pub use haptics::{
    clear_platform_haptics, default_haptics, local_haptics, set_platform_haptics, HapticEffect,
    HapticError, HapticFeedback, HapticPattern, Haptics, HapticsRef, ProvideHaptics,
};
pub use host::{
    application_directories, application_id, background_app, clear_application_id,
    clear_host_controller, current_lifecycle_state, dispatch_lifecycle, dispatch_lifecycle_state,
    exit_app, host_controller, local_lifecycle_state, observe_lifecycle, register_durable_save,
    rememberLifecycleEvents, rememberLifecycleState, set_application_id, set_host_controller,
    set_keep_screen_on, DurableSaveEffect, DurableSaveOutcome, DurableSaveRegistration,
    HostController, HostControllerRef, LifecycleEvent, LifecycleObserver, LifecycleState,
    PlatformDirectories, PlatformDirectoryError, ProvideLifecycle, DEFAULT_DURABLE_SAVE_DEADLINE,
};
#[cfg(not(target_arch = "wasm32"))]
pub use host::{durable_save_deadline, run_durable_saves};
pub use host_surface::{
    clear_platform_host_surface, host_surface, host_surface_size, observe_host_surface_size,
    publish_host_surface_size, rememberHostSurfaceSize, request_host_surface_size,
    set_platform_host_surface, HostSurface, HostSurfaceObserver, HostSurfaceRef, HostSurfaceSize,
    ResizeRefused,
};
pub use http::{
    default_http_client, local_http_client, map_ordered_concurrent, BytesBody, HttpBody,
    HttpBodyRef, HttpClient, HttpClientRef, HttpControl, HttpError, HttpFuture, HttpMethod,
    HttpProgress, HttpRequest, HttpResponse, ProgressHandler, StubAnswer, StubHttpClient,
};
pub use image_picker::{
    clear_platform_image_picker, default_image_picker, local_image_picker,
    set_platform_image_picker, ImagePicker, ImagePickerError, ImagePickerRef, ImageSource,
    ProvideImagePicker, IMAGE_EXTENSIONS,
};
pub use incoming_share::{
    clear_incoming_content, observe_incoming_content, publish_incoming_content,
    rememberIncomingContent, IncomingContent, IncomingContentObserver, IncomingSource,
};
pub use launch_args::{
    clear_platform_launch_args, isDebuggable, is_debuggable, launch_args,
    launch_args_from_command_line, local_launch_args, set_platform_launch_args, LaunchArgValue,
    LaunchArgs, LaunchArgsRef, ProvideLaunchArgs,
};
pub use launcher::{
    clear_launcher_state, rememberOpenFileLauncher, rememberOpenFilesLauncher,
    rememberOpenFolderLauncher, rememberSaveDocumentLauncher, rememberWritableFolderLauncher,
    LauncherResult, OpenFileLauncher, OpenFilesLauncher, OpenFolderLauncher, SaveDocumentLauncher,
    WritableFolderLauncher,
};
pub use media::{
    audio_focus, clear_platform_media_player, current_media_item, dropped_media_samples,
    latest_media_samples, media_capabilities, media_equalizer, media_equalizer_bands,
    media_playback_supported, media_player, media_volume, observe_audio_focus,
    observe_media_commands, observe_media_samples, observe_playback_progress,
    observe_playback_state, octave_equalizer_bands, open_media, path_from_uri, pause_media,
    play_media, playback_progress, playback_state, probe_media_duration, publish_audio_focus,
    publish_media_command, publish_media_samples, publish_playback_progress,
    publish_playback_state, record_dropped_media_samples, rememberAudioFocus,
    rememberMediaCommands, rememberMediaSamples, rememberPlaybackProgress, rememberPlaybackState,
    seek_media, seek_media_fraction, set_media_analysis_enabled, set_media_equalizer,
    set_media_looping, set_media_metadata, set_media_speed, set_media_volume,
    set_platform_media_player, stop_media, toggle_media, uri_for_path, AudioFocus, EqualizerBand,
    EqualizerSettings, MediaArtwork, MediaCapabilities, MediaCommand, MediaError, MediaItem,
    MediaMetadata, MediaObserver, MediaPlayer, MediaPlayerRef, MediaSamples, PlaybackProgress,
    PlaybackState, DUCKED_GAIN, OCTAVE_BAND_CENTERS_HZ,
};
pub use navigation::{
    back_interception_enabled, exit_requested, observe_back_requests, push_back_request,
    request_exit, set_back_interception, take_back_requests, take_exit_request,
    BackRequestObserver,
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
pub use power::{
    clear_platform_power_monitor, observe_power_state, power_capabilities, power_monitor,
    power_state, publish_power_state, rememberPowerState, set_platform_power_monitor,
    BatteryStatus, PowerCapabilities, PowerMonitor, PowerMonitorRef, PowerObserverRegistration,
    PowerReading, PowerState, ThermalState,
};
#[cfg(all(target_arch = "wasm32", feature = "preferences-web"))]
pub use preferences::BrowserPreferences;
#[cfg(not(target_arch = "wasm32"))]
pub use preferences::FilePreferences;
pub use preferences::{
    clear_platform_preferences, preferences, rememberSaveable, set_platform_preferences,
    MemoryPreferences, PreferencesError, PreferencesRef, PreferencesStore, Saver,
};
pub use purchases::{
    clear_platform_purchases, note_store_news, observe_store_news, purchases,
    rememberPurchaseEvents, rememberStoreState, set_platform_purchases, store_available,
    store_state, Product, PurchaseEvent, Purchases, PurchasesRef, StoreObserver, StorePhase,
    StoreState,
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
    open_writable_folder, set_writable_folder_store_factory, FolderEntry, FolderError,
    FolderReader, FolderWriter, WritableFolderStore, WritableFolderStoreRef,
};

/// Convenience alias used in unit tests.
#[cfg(test)]
pub(crate) type TestComposition = Composition<MemoryApplier>;

/// A unique, empty directory under the workspace `target/test-output` for a
/// test that needs real files.
///
/// Never the system temporary directory: on Linux that is tmpfs, so a test
/// writing a payload there writes it to RAM, and a failure leaves nothing under
/// `target` to look at afterwards.
#[cfg(test)]
pub(crate) fn test_scratch_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-output/cranpose-services")
        .join(format!("{tag}-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).expect("a scratch directory under target/test-output");
    path
}

/// Build a composition with a simple in-memory applier and run the provided closure once.
#[cfg(test)]
pub(crate) fn run_test_composition(build: impl FnMut()) -> TestComposition {
    let mut composition = Composition::new(MemoryApplier::new());
    composition
        .render(location_key(file!(), line!(), column!()), build)
        .expect("initial render succeeds");
    composition
}
