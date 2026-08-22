//! What every service says when this platform has no backend for it.
//!
//! A service that quietly returns a plausible default when nothing is
//! registered is worse than one that says so: a screen greys out no control,
//! the user presses it, and nothing happens. The rule the whole registry is
//! built on is that a platform without the concept reports it — `false`,
//! `None`, `Unsupported` — and this pins it for every service at once, so a
//! backend added later cannot quietly become the thing that hides an absence.

use cranpose_services::{
    app_updates_supported, camera, camera_supported, check_for_app_update,
    clear_platform_app_updater, clear_platform_camera, clear_platform_host_surface,
    clear_platform_media_player, clear_platform_purchases, host_surface, install_app_update,
    media_capabilities, media_playback_supported, media_player, open_media, play_media,
    request_camera_still, seek_media, set_media_analysis_enabled, set_media_speed, start_camera,
    store_available, AppUpdateError, CameraError, GitHubReleaseUpdate, MediaCapabilities,
    MediaError, MediaItem, PackageDigest, ResizeRefused, UpdatePackage,
};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

/// The services are one registry for the process, so tests that clear it take
/// turns rather than clearing each other's backends mid-assertion.
fn one_registry_at_a_time() -> MutexGuard<'static, ()> {
    static REGISTRY: Mutex<()> = Mutex::new(());
    REGISTRY.lock().unwrap_or_else(|error| error.into_inner())
}

#[test]
fn a_platform_without_a_camera_says_so_rather_than_producing_nothing() {
    let _registry = one_registry_at_a_time();
    clear_platform_camera();

    assert!(!camera_supported());
    assert!(camera().is_none());
    assert_eq!(start_camera(), Err(CameraError::Unsupported));
    assert_eq!(request_camera_still(), Err(CameraError::Unsupported));
}

#[test]
fn a_platform_without_media_playback_refuses_every_transport_call() {
    let _registry = one_registry_at_a_time();
    clear_platform_media_player();

    assert!(!media_playback_supported());
    assert!(media_player().is_none());
    assert_eq!(
        media_capabilities(),
        MediaCapabilities::default(),
        "with no backend every control is absent, not assumed present"
    );
    assert_eq!(
        open_media(MediaItem::new("file:///music/track.flac")),
        Err(MediaError::Unsupported)
    );
    assert_eq!(play_media(), Err(MediaError::Unsupported));
    assert_eq!(
        seek_media(Duration::from_secs(1)),
        Err(MediaError::Unsupported)
    );
    assert!(!set_media_speed(2.0));
    assert!(!set_media_analysis_enabled(true));
}

#[test]
fn a_platform_without_a_store_owns_nothing_and_grants_nothing() {
    let _registry = one_registry_at_a_time();
    clear_platform_purchases();

    assert!(!store_available());
}

#[test]
fn a_platform_that_cannot_install_updates_refuses_before_it_downloads() {
    let _registry = one_registry_at_a_time();
    clear_platform_app_updater();

    assert!(!app_updates_supported());
    assert_eq!(
        check_for_app_update(&GitHubReleaseUpdate::new("owner/repo", "1.0.0", ".apk")),
        Err(AppUpdateError::Unsupported)
    );

    let package = UpdatePackage::new("2.0.0", "https://host/app.apk")
        .with_digest(PackageDigest::sha256("00".repeat(32)));
    assert_eq!(
        install_app_update(&package),
        Err(AppUpdateError::Unsupported),
        "an update nobody can install must not be downloaded first"
    );
}

/// A window nobody can resize is the ordinary case on a phone and on a watch.
/// The surface still exists — the size is published either way — so what is
/// absent is the resize, and only that.
#[test]
fn a_host_that_cannot_be_resized_refuses_the_request_rather_than_dropping_it() {
    let _registry = one_registry_at_a_time();
    clear_platform_host_surface();

    let surface = host_surface();
    assert!(!surface.can_resize());
    assert_eq!(
        surface.request_size(320.0, 240.0),
        Err(ResizeRefused::Unsupported)
    );
}

/// A package the framework cannot check is a package it will not install,
/// whatever the release feed said about it.
#[test]
fn a_package_nobody_can_verify_is_one_nobody_installs() {
    let _registry = one_registry_at_a_time();
    clear_platform_app_updater();

    assert!(
        !UpdatePackage::new("2.0.0", "https://host/app.apk").is_verifiable(),
        "a package with no digest cannot be verified"
    );
    assert!(
        !UpdatePackage::new("2.0.0", "https://host/app.apk")
            .with_digest(PackageDigest::sha256("not-a-digest"))
            .is_verifiable(),
        "a digest this framework cannot read is not a check"
    );
    assert!(UpdatePackage::new("2.0.0", "https://host/app.apk")
        .with_digest(PackageDigest::sha256("00".repeat(32)))
        .is_verifiable());
}
