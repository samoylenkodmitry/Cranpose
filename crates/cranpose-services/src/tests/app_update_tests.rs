use std::sync::{PoisonError, atomic::AtomicUsize};

use super::*;

struct RecordingUpdater {
    checks: AtomicUsize,
    installed: Mutex<Vec<UpdatePackage>>,
}

impl RecordingUpdater {
    fn new() -> Self {
        Self {
            checks: AtomicUsize::new(0),
            installed: Mutex::new(Vec::new()),
        }
    }

    fn installs(&self) -> Vec<UpdatePackage> {
        self.installed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl AppUpdater for RecordingUpdater {
    fn capabilities(&self) -> AppUpdateCapabilities {
        AppUpdateCapabilities {
            check: true,
            install: true,
        }
    }

    fn check(&self, _source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
        self.checks.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn install(&self, package: &UpdatePackage) -> Result<(), AppUpdateError> {
        self.installed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(package.clone());
        Ok(())
    }
}

const EMPTY_SHA256: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
const ABC_SHA256: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

#[test]
fn the_digest_is_the_one_every_other_implementation_computes() {
    assert_eq!(sha256_hex(b""), EMPTY_SHA256);
    assert_eq!(sha256_hex(b"abc"), ABC_SHA256);
}

#[test]
fn a_feed_digest_is_read_in_the_form_feeds_publish_it() {
    let digest = PackageDigest::parse(&format!("sha256:{}", ABC_SHA256.to_uppercase()))
        .expect("a sha256 digest");
    assert_eq!(digest.algorithm, DigestAlgorithm::Sha256);
    assert_eq!(digest.value, ABC_SHA256, "case is normalised on the way in");
    assert_eq!(digest.to_feed_string(), format!("sha256:{ABC_SHA256}"));
    assert!(digest.is_well_formed());
}

#[test]
fn a_digest_this_framework_cannot_check_is_refused_rather_than_ignored() {
    assert_eq!(PackageDigest::parse("md5:abcdef"), None);
    assert_eq!(PackageDigest::parse("sha256:"), None);
    assert_eq!(PackageDigest::parse("no-algorithm"), None);
    assert!(!PackageDigest::sha256("not hexadecimal").is_well_formed());
    assert!(!PackageDigest::sha256("abcd").is_well_formed(), "too short");
    assert!(matches!(
        DigestVerifier::new(PackageDigest::sha256("abcd")),
        Err(AppUpdateError::MalformedDigest(_))
    ));
}

#[test]
fn a_package_that_matches_its_digest_verifies() {
    assert_eq!(
        verify_package(b"abc", &PackageDigest::sha256(ABC_SHA256)),
        Ok(())
    );
}

#[test]
fn a_package_that_does_not_match_reports_both_digests() {
    let error = verify_package(b"abd", &PackageDigest::sha256(ABC_SHA256))
        .expect_err("a changed byte must not verify");
    match error {
        AppUpdateError::VerificationFailed { expected, actual } => {
            assert_eq!(expected, ABC_SHA256);
            assert_ne!(actual, ABC_SHA256);
            assert_eq!(actual, sha256_hex(b"abd"));
        }
        other => panic!("expected a verification failure, got {other}"),
    }
}

#[test]
fn a_package_read_in_chunks_verifies_the_same_as_one_read_whole() {
    let mut verifier =
        DigestVerifier::new(PackageDigest::sha256(ABC_SHA256)).expect("a well-formed digest");
    assert!(verifier.is_empty());
    verifier.update(b"a");
    verifier.update(b"b");
    verifier.update(b"c");
    assert_eq!(verifier.len(), 3);
    assert_eq!(verifier.finish(), Ok(()));
}

#[test]
fn a_package_carries_what_the_feed_promised_about_it() {
    let package = UpdatePackage::new("1.2.3", "https://example.test/app.apk")
        .with_size(4096)
        .with_digest(PackageDigest::sha256(ABC_SHA256))
        .with_notes("Fixes the thing");
    assert_eq!(package.version, "1.2.3");
    assert_eq!(package.size, Some(4096));
    assert!(package.is_verifiable());
    assert_eq!(package.notes.as_deref(), Some("Fixes the thing"));

    assert!(
        !UpdatePackage::new("1.2.3", "https://example.test/app.apk").is_verifiable(),
        "a feed that published no digest leaves nothing to check against"
    );
}

#[test]
fn request_builds_typed_source() {
    let source = GitHubReleaseUpdate::new("owner/app", "1.2.3", ".apk");
    assert_eq!(source.repository, "owner/app");
    assert_eq!(source.current_version, "1.2.3");
    assert_eq!(source.asset_suffix, ".apk");
}

#[test]
fn operations_publish_and_forward() {
    let _guard = crate::registry::test_service_guard();
    let updater = Arc::new(RecordingUpdater::new());
    set_platform_app_updater(updater.clone());
    assert!(app_updates_supported());
    check_for_app_update(&GitHubReleaseUpdate::new("owner/app", "1", ".apk")).unwrap();
    assert_eq!(updater.checks.load(Ordering::Relaxed), 1);
    assert_eq!(app_update_status(), AppUpdateStatus::Checking);

    let package = UpdatePackage::new("2", "https://example.test/app.apk")
        .with_size(4096)
        .with_digest(PackageDigest::sha256(sha256_hex(b"package")));
    install_app_update(&package).unwrap();
    assert_eq!(updater.installs(), vec![package]);
    assert_eq!(
        app_update_status(),
        AppUpdateStatus::Downloading {
            downloaded: 0,
            total: Some(4096)
        },
        "the size the feed published is reported before the first byte arrives"
    );
    clear_platform_app_updater();
    assert!(!app_updates_supported());
}

#[test]
fn a_package_with_an_uncheckable_digest_is_refused_before_it_is_downloaded() {
    let _guard = crate::registry::test_service_guard();
    let updater = Arc::new(RecordingUpdater::new());
    set_platform_app_updater(updater.clone());
    let package = UpdatePackage::new("2", "https://example.test/app.apk")
        .with_digest(PackageDigest::sha256("not-a-digest"));
    assert!(matches!(
        install_app_update(&package),
        Err(AppUpdateError::MalformedDigest(_))
    ));
    assert!(updater.installs().is_empty());
    assert!(matches!(app_update_status(), AppUpdateStatus::Error(_)));
    clear_platform_app_updater();
}

#[test]
fn a_host_that_cannot_update_says_so_through_the_status_and_not_only_the_result() {
    let _guard = crate::registry::test_service_guard();

    clear_platform_app_updater();
    set_app_update_status(AppUpdateStatus::Idle);
    assert_eq!(
        check_for_app_update(&GitHubReleaseUpdate::new("owner/app", "1", ".apk")),
        Err(AppUpdateError::Unsupported)
    );
    assert!(matches!(app_update_status(), AppUpdateStatus::Error(_)));

    struct CheckOnlyUpdater;
    impl AppUpdater for CheckOnlyUpdater {
        fn capabilities(&self) -> AppUpdateCapabilities {
            AppUpdateCapabilities {
                check: true,
                install: false,
            }
        }
        fn check(&self, _source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
            Ok(())
        }
    }
    set_platform_app_updater(Arc::new(CheckOnlyUpdater));
    set_app_update_status(AppUpdateStatus::Idle);
    let package = UpdatePackage::new("2", "https://example.test/app.apk")
        .with_digest(PackageDigest::sha256(sha256_hex(b"package")));
    assert_eq!(
        install_app_update(&package),
        Err(AppUpdateError::Unsupported)
    );
    assert!(matches!(app_update_status(), AppUpdateStatus::Error(_)));
    assert!(app_update_checks_supported());
    assert!(!app_updates_supported());
    clear_platform_app_updater();
}

#[test]
fn a_package_with_no_digest_at_all_never_reaches_the_installer() {
    let _guard = crate::registry::test_service_guard();
    let updater = Arc::new(RecordingUpdater::new());
    set_platform_app_updater(updater.clone());
    let package = UpdatePackage::new("2", "https://example.test/app.apk");

    assert!(!package.is_verifiable());
    assert_eq!(
        install_app_update(&package),
        Err(AppUpdateError::Unverifiable)
    );

    assert!(updater.installs().is_empty());
    assert!(matches!(app_update_status(), AppUpdateStatus::Error(_)));
    clear_platform_app_updater();
}

#[test]
fn observer_receives_current_and_changed_status() {
    let _guard = crate::registry::test_service_guard();
    set_app_update_status(AppUpdateStatus::Idle);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let captured = Arc::clone(&seen);
    let observer = observe_app_update_status(move |status| {
        captured
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(status);
    });
    set_app_update_status(AppUpdateStatus::Verifying);
    set_app_update_status(AppUpdateStatus::Installing);
    assert_eq!(
        *seen.lock().unwrap_or_else(PoisonError::into_inner),
        vec![
            AppUpdateStatus::Idle,
            AppUpdateStatus::Verifying,
            AppUpdateStatus::Installing
        ]
    );
    drop(observer);
    set_app_update_status(AppUpdateStatus::Idle);
}
