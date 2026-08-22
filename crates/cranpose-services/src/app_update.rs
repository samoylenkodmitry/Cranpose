//! Typed application update discovery, verification, and platform installation.
//!
//! An update is the one download an application performs that can replace the
//! application, so what arrives is checked against what was promised before it
//! reaches a platform installer. The check lives here, once, rather than in each
//! platform's installer: a digest computed four different ways is four chances
//! to compute it wrongly, and one of them will be the one nobody tested.

use crate::registry::ServiceRegistry;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

/// How a package's bytes are checked against what the release promised.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DigestAlgorithm {
    /// SHA-256, which is what release feeds publish.
    #[default]
    Sha256,
}

impl DigestAlgorithm {
    /// The name a release feed writes, and the name a platform's own digest API
    /// answers to.
    pub fn name(self) -> &'static str {
        match self {
            DigestAlgorithm::Sha256 => "sha256",
        }
    }

    /// Reads an algorithm a release feed named, or `None` for one this
    /// framework cannot compute — which is refused rather than skipped, because
    /// a digest nobody checks is worse than no digest at all.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "sha256" | "sha-256" => Some(DigestAlgorithm::Sha256),
            _ => None,
        }
    }
}

/// The digest a downloaded package must match.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PackageDigest {
    pub algorithm: DigestAlgorithm,
    /// Lower-case hexadecimal.
    pub value: String,
}

impl PackageDigest {
    /// A SHA-256 digest, from hexadecimal in either case.
    pub fn sha256(value: impl AsRef<str>) -> Self {
        Self {
            algorithm: DigestAlgorithm::Sha256,
            value: value.as_ref().trim().to_ascii_lowercase(),
        }
    }

    /// Reads the `sha256:<hex>` form release feeds publish.
    pub fn parse(value: &str) -> Option<Self> {
        let (algorithm, digest) = value.split_once(':')?;
        let algorithm = DigestAlgorithm::parse(algorithm)?;
        let digest = digest.trim().to_ascii_lowercase();
        (!digest.is_empty()).then_some(Self {
            algorithm,
            value: digest,
        })
    }

    /// Whether this digest could be one: the right length, and hexadecimal.
    ///
    /// A malformed digest is refused before a download starts rather than after
    /// it, so nobody waits for two hundred megabytes to learn the release feed
    /// was misconfigured.
    pub fn is_well_formed(&self) -> bool {
        let expected = match self.algorithm {
            DigestAlgorithm::Sha256 => 64,
        };
        self.value.len() == expected && self.value.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    /// The `sha256:<hex>` form.
    pub fn to_feed_string(&self) -> String {
        format!("{}:{}", self.algorithm.name(), self.value)
    }
}

/// The SHA-256 of `bytes`, as lower-case hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

/// Checks a package as it is read, so a package too large to hold in memory is
/// still checked.
///
/// An installer feeds every chunk it writes through this and calls
/// [`DigestVerifier::finish`] before committing; nothing installs a package
/// whose bytes were never seen in full.
pub struct DigestVerifier {
    expected: PackageDigest,
    hasher: Sha256,
    len: u64,
}

impl DigestVerifier {
    /// A verifier for `expected`, or an error when the digest is malformed.
    pub fn new(expected: PackageDigest) -> Result<Self, AppUpdateError> {
        if !expected.is_well_formed() {
            return Err(AppUpdateError::MalformedDigest(expected.to_feed_string()));
        }
        Ok(Self {
            expected,
            hasher: Sha256::new(),
            len: 0,
        })
    }

    /// Feeds the next chunk of the package.
    pub fn update(&mut self, chunk: &[u8]) {
        self.hasher.update(chunk);
        self.len += chunk.len() as u64;
    }

    /// How many bytes have been read so far.
    pub fn len(&self) -> u64 {
        self.len
    }

    /// Whether nothing has been read yet.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Checks what was read against what was promised.
    pub fn finish(self) -> Result<(), AppUpdateError> {
        let actual = hex(&self.hasher.finalize());
        if actual == self.expected.value {
            Ok(())
        } else {
            Err(AppUpdateError::VerificationFailed {
                expected: self.expected.value,
                actual,
            })
        }
    }
}

/// Checks a package held in memory against `digest`.
pub fn verify_package(bytes: &[u8], digest: &PackageDigest) -> Result<(), AppUpdateError> {
    let mut verifier = DigestVerifier::new(digest.clone())?;
    verifier.update(bytes);
    verifier.finish()
}

/// A package an update would install.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct UpdatePackage {
    /// The version this package installs.
    pub version: String,
    /// Where the package is downloaded from.
    pub download_url: String,
    /// How large it is, when the release feed says.
    pub size: Option<u64>,
    /// What its bytes must hash to.
    ///
    /// `None` means the release feed published none, and
    /// [`install_app_update`] refuses such a package: the platform's own
    /// signature check catches a package signed by someone else, but not one
    /// that arrived corrupted, and this is the one download that replaces the
    /// application. A feed with no digest is a feed to fix.
    pub digest: Option<PackageDigest>,
    /// Release notes, when the feed carries them.
    pub notes: Option<String>,
}

impl UpdatePackage {
    /// A package at `download_url` installing `version`.
    pub fn new(version: impl Into<String>, download_url: impl Into<String>) -> Self {
        Self {
            version: version.into(),
            download_url: download_url.into(),
            ..Self::default()
        }
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_digest(mut self, digest: PackageDigest) -> Self {
        self.digest = Some(digest);
        self
    }

    pub fn with_notes(mut self, notes: impl Into<String>) -> Self {
        self.notes = Some(notes.into());
        self
    }

    /// Whether this package can be checked against what the feed promised.
    pub fn is_verifiable(&self) -> bool {
        self.digest
            .as_ref()
            .is_some_and(PackageDigest::is_well_formed)
    }
}

/// A GitHub release feed used to discover an application package.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GitHubReleaseUpdate {
    /// Repository in `owner/name` form.
    pub repository: String,
    /// Version of the running application.
    pub current_version: String,
    /// File-name suffix selected from the release assets, such as `.apk`.
    pub asset_suffix: String,
}

impl GitHubReleaseUpdate {
    /// Creates a GitHub release request.
    pub fn new(
        repository: impl Into<String>,
        current_version: impl Into<String>,
        asset_suffix: impl Into<String>,
    ) -> Self {
        Self {
            repository: repository.into(),
            current_version: current_version.into(),
            asset_suffix: asset_suffix.into(),
        }
    }
}

/// Observable state of the application update flow.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum AppUpdateStatus {
    /// No operation has started.
    #[default]
    Idle,
    /// The release feed is being queried.
    Checking,
    /// The running application is current.
    UpToDate,
    /// A package can be installed.
    Available {
        /// What the release feed offers, including its size and digest when it
        /// published them.
        package: UpdatePackage,
    },
    /// A package is being transferred to the platform installer.
    Downloading {
        /// Bytes transferred so far.
        downloaded: u64,
        /// Total bytes when supplied by the server.
        total: Option<u64>,
    },
    /// The transfer finished and the package is being checked against the
    /// digest the release feed published.
    Verifying,
    /// The platform is asking the user to approve installation.
    AwaitingConfirmation,
    /// The platform installer accepted the package.
    Installing,
    /// The operation could not continue.
    Error(String),
}

/// Failure to start an update operation.
#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub enum AppUpdateError {
    /// This platform has no registered installer.
    #[error("application updates are unavailable on this platform")]
    Unsupported,
    /// The platform rejected the request before work started.
    #[error("application update request failed: {0}")]
    Request(String),
    /// The release feed published no digest for this package.
    ///
    /// Refused rather than installed: this is an application replacing itself
    /// with bytes off the network, and bytes nobody checked are bytes nobody
    /// checked whether or not the feed mentioned it. A feed that publishes no
    /// digest is a feed to fix, not a check to skip.
    #[error("the release feed published no digest for this package, so it cannot be checked")]
    Unverifiable,
    /// The release feed published a digest this framework cannot check.
    ///
    /// Refused rather than ignored: a digest nobody checks reads as a package
    /// that was verified.
    #[error("the release feed published a digest that cannot be checked: {0}")]
    MalformedDigest(String),
    /// What arrived is not what the release feed promised.
    #[error(
        "the downloaded package does not match its digest (expected {expected}, got {actual})"
    )]
    VerificationFailed {
        /// The digest the release feed published.
        expected: String,
        /// The digest the bytes that arrived actually have.
        actual: String,
    },
}

/// Platform implementation for update discovery and package installation.
pub trait AppUpdater: Send + Sync {
    /// Starts release discovery. Progress is published through
    /// [`set_app_update_status`].
    fn check(&self, source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError>;

    /// Transfers a package to the platform installer.
    ///
    /// An implementation checks the package against
    /// [`UpdatePackage::digest`] before committing it — nothing is installed
    /// whose bytes were not the ones the release feed promised.
    fn install(&self, package: &UpdatePackage) -> Result<(), AppUpdateError>;
}

/// Shared updater service.
pub type AppUpdaterRef = Arc<dyn AppUpdater>;

static PLATFORM_UPDATER: ServiceRegistry<dyn AppUpdater> = ServiceRegistry::new();

/// Installs the platform updater.
pub fn set_platform_app_updater(updater: AppUpdaterRef) {
    PLATFORM_UPDATER.set(updater);
}

/// Removes the platform updater.
pub fn clear_platform_app_updater() {
    PLATFORM_UPDATER.clear();
}

/// Returns whether this host can install application updates.
pub fn app_updates_supported() -> bool {
    PLATFORM_UPDATER.get().is_some()
}

/// Starts update discovery and publishes the initial state.
pub fn check_for_app_update(source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
    let updater = PLATFORM_UPDATER.get().ok_or(AppUpdateError::Unsupported)?;
    set_app_update_status(AppUpdateStatus::Checking);
    updater.check(source).inspect_err(|error| {
        set_app_update_status(AppUpdateStatus::Error(error.to_string()));
    })
}

/// Starts package installation and publishes the initial transfer state.
///
/// A digest the release feed published but this framework cannot check is
/// refused here, before anything is downloaded: nobody waits for two hundred
/// megabytes to learn the feed was misconfigured, and nothing reaches an
/// installer unchecked because its digest was unreadable.
pub fn install_app_update(package: &UpdatePackage) -> Result<(), AppUpdateError> {
    let updater = PLATFORM_UPDATER.get().ok_or(AppUpdateError::Unsupported)?;
    let error = match &package.digest {
        None => Some(AppUpdateError::Unverifiable),
        Some(digest) if !digest.is_well_formed() => {
            Some(AppUpdateError::MalformedDigest(digest.to_feed_string()))
        }
        Some(_) => None,
    };
    if let Some(error) = error {
        set_app_update_status(AppUpdateStatus::Error(error.to_string()));
        return Err(error);
    }
    set_app_update_status(AppUpdateStatus::Downloading {
        downloaded: 0,
        total: package.size,
    });
    updater.install(package).inspect_err(|error| {
        set_app_update_status(AppUpdateStatus::Error(error.to_string()));
    })
}

fn status_slot() -> &'static Mutex<AppUpdateStatus> {
    static STATUS: OnceLock<Mutex<AppUpdateStatus>> = OnceLock::new();
    STATUS.get_or_init(|| Mutex::new(AppUpdateStatus::Idle))
}

/// Returns the latest update state.
pub fn app_update_status() -> AppUpdateStatus {
    status_slot()
        .lock()
        .map(|status| status.clone())
        .unwrap_or_else(|poisoned| poisoned.into_inner().clone())
}

#[cfg(not(target_arch = "wasm32"))]
type Observer = Arc<dyn Fn(AppUpdateStatus) + Send + Sync>;
#[cfg(target_arch = "wasm32")]
type Observer = std::rc::Rc<dyn Fn(AppUpdateStatus)>;

#[cfg(not(target_arch = "wasm32"))]
fn observers() -> &'static Mutex<Vec<(u64, Observer)>> {
    static OBSERVERS: OnceLock<Mutex<Vec<(u64, Observer)>>> = OnceLock::new();
    OBSERVERS.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static OBSERVERS: std::cell::RefCell<Vec<(u64, Observer)>> = const { std::cell::RefCell::new(Vec::new()) };
}

static NEXT_OBSERVER_ID: AtomicU64 = AtomicU64::new(1);

/// Registration returned by [`observe_app_update_status`].
pub struct AppUpdateObserver {
    id: u64,
}

impl Drop for AppUpdateObserver {
    fn drop(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Ok(mut observers) = observers().lock() {
            observers.retain(|(id, _)| *id != self.id);
        }
        #[cfg(target_arch = "wasm32")]
        OBSERVERS.with(|observers| observers.borrow_mut().retain(|(id, _)| *id != self.id));
    }
}

/// Observes update state changes. The current state is delivered immediately.
#[cfg(not(target_arch = "wasm32"))]
pub fn observe_app_update_status(
    observer: impl Fn(AppUpdateStatus) + Send + Sync + 'static,
) -> AppUpdateObserver {
    let id = NEXT_OBSERVER_ID.fetch_add(1, Ordering::Relaxed);
    let observer: Observer = Arc::new(observer);
    if let Ok(mut observers) = observers().lock() {
        observers.push((id, Arc::clone(&observer)));
    }
    observer(app_update_status());
    AppUpdateObserver { id }
}

/// Observes update state changes. The current state is delivered immediately.
#[cfg(target_arch = "wasm32")]
pub fn observe_app_update_status(
    observer: impl Fn(AppUpdateStatus) + 'static,
) -> AppUpdateObserver {
    let id = NEXT_OBSERVER_ID.fetch_add(1, Ordering::Relaxed);
    let observer: Observer = std::rc::Rc::new(observer);
    OBSERVERS.with(|observers| {
        observers
            .borrow_mut()
            .push((id, std::rc::Rc::clone(&observer)))
    });
    observer(app_update_status());
    AppUpdateObserver { id }
}

/// Publishes state from a platform updater.
pub fn set_app_update_status(status: AppUpdateStatus) {
    if let Ok(mut current) = status_slot().lock() {
        if *current == status {
            return;
        }
        *current = status.clone();
    }
    #[cfg(not(target_arch = "wasm32"))]
    let observers = observers()
        .lock()
        .map(|observers| {
            observers
                .iter()
                .map(|(_, observer)| Arc::clone(observer))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    #[cfg(target_arch = "wasm32")]
    let observers = OBSERVERS.with(|observers| {
        observers
            .borrow()
            .iter()
            .map(|(_, observer)| std::rc::Rc::clone(observer))
            .collect::<Vec<_>>()
    });
    for observer in observers {
        observer(status.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

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
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }

    impl AppUpdater for RecordingUpdater {
        fn check(&self, _source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
            self.checks.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn install(&self, package: &UpdatePackage) -> Result<(), AppUpdateError> {
            self.installed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(package.clone());
            Ok(())
        }
    }

    /// The digest of the empty input, which is the one value every SHA-256
    /// implementation agrees on and the one a broken wiring gets wrong.
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

    /// A package can be larger than memory, so it is checked as it is read.
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

    /// Nobody waits for two hundred megabytes to learn the release feed was
    /// misconfigured, and nothing reaches an installer unchecked because its
    /// digest could not be read.
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

    /// An application replacing itself with bytes off the network is the one
    /// download that must not be taken on trust.
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
                .unwrap_or_else(|error| error.into_inner())
                .push(status);
        });
        set_app_update_status(AppUpdateStatus::Verifying);
        set_app_update_status(AppUpdateStatus::Installing);
        assert_eq!(
            *seen.lock().unwrap_or_else(|error| error.into_inner()),
            vec![
                AppUpdateStatus::Idle,
                AppUpdateStatus::Verifying,
                AppUpdateStatus::Installing
            ]
        );
        drop(observer);
        set_app_update_status(AppUpdateStatus::Idle);
    }
}
