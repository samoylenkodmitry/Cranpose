//! Typed application update discovery, verification, and platform installation.
//!
//! An update is the one download an application performs that can replace the
//! application, so what arrives is checked against what was promised before it
//! reaches a platform installer. The check lives here, once, rather than in each
//! platform's installer: a digest computed four different ways is four chances
//! to compute it wrongly, and one of them will be the one nobody tested.

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use sha2::{Digest, Sha256};

use crate::registry::ServiceRegistry;

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
    #[error("the downloaded package does not match its digest (expected {expected}, got {actual})")]
    VerificationFailed {
        /// The digest the release feed published.
        expected: String,
        /// The digest the bytes that arrived actually have.
        actual: String,
    },
}

/// Platform implementation for update discovery and package installation.
pub trait AppUpdater: Send + Sync {
    /// What this backend can do.
    ///
    /// Defaults to neither, so a backend states what it can do rather than
    /// inheriting a claim: the two entry points below refuse a half a backend
    /// has not claimed, and a backend that forgot to declare one is refused
    /// rather than allowed to fail at the platform boundary.
    fn capabilities(&self) -> AppUpdateCapabilities {
        AppUpdateCapabilities::default()
    }

    /// Starts release discovery. Progress is published through
    /// [`set_app_update_status`].
    fn check(&self, source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
        let _ = source;
        Err(AppUpdateError::Unsupported)
    }

    /// Transfers a package to the platform installer.
    ///
    /// An implementation checks the package against
    /// [`UpdatePackage::digest`] before committing it — nothing is installed
    /// whose bytes were not the ones the release feed promised.
    fn install(&self, package: &UpdatePackage) -> Result<(), AppUpdateError> {
        let _ = package;
        Err(AppUpdateError::Unsupported)
    }
}

/// What an update backend can do on this platform.
///
/// The two halves are separate because a platform can genuinely have one
/// without the other: an iOS application may discover that a newer version
/// exists and send the reader to the store, while installing a replacement
/// binary is something the platform does not allow it to do at all. Reporting
/// one flag for both would make `check` look unavailable where it works, or
/// make `install` look available where it can only fail.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppUpdateCapabilities {
    /// Whether this platform can discover a newer release.
    pub check: bool,
    /// Whether this platform can install one.
    pub install: bool,
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

/// What this host can do about application updates.
pub fn app_update_capabilities() -> AppUpdateCapabilities {
    PLATFORM_UPDATER
        .get()
        .map(|updater| updater.capabilities())
        .unwrap_or_default()
}

/// Returns whether this host can install application updates.
pub fn app_updates_supported() -> bool {
    app_update_capabilities().install
}

/// Returns whether this host can discover a newer release.
///
/// A host may answer yes here and no to [`app_updates_supported`]: knowing an
/// update exists is what lets an application point at the store it cannot
/// install from itself.
pub fn app_update_checks_supported() -> bool {
    app_update_capabilities().check
}

fn publish_failure(error: AppUpdateError) -> Result<(), AppUpdateError> {
    set_app_update_status(AppUpdateStatus::Error(error.to_string()));
    Err(error)
}

/// Starts update discovery and publishes the initial state.
pub fn check_for_app_update(source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
    let Some(updater) = PLATFORM_UPDATER.get() else {
        return publish_failure(AppUpdateError::Unsupported);
    };
    if !updater.capabilities().check {
        return publish_failure(AppUpdateError::Unsupported);
    }
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
    let Some(updater) = PLATFORM_UPDATER.get() else {
        return publish_failure(AppUpdateError::Unsupported);
    };
    if !updater.capabilities().install {
        return publish_failure(AppUpdateError::Unsupported);
    }
    let error = match &package.digest {
        None => Some(AppUpdateError::Unverifiable),
        Some(digest) if !digest.is_well_formed() => {
            Some(AppUpdateError::MalformedDigest(digest.to_feed_string()))
        }
        Some(_) => None,
    };
    if let Some(error) = error {
        return publish_failure(error);
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
    status_slot().lock().map_or_else(
        |poisoned| poisoned.into_inner().clone(),
        |status| status.clone(),
    )
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
            .push((id, std::rc::Rc::clone(&observer)));
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
#[path = "tests/app_update_tests.rs"]
mod tests;
