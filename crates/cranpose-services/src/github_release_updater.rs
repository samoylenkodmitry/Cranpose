//! A portable [`AppUpdater`] that discovers a newer release from a GitHub
//! repository's release feed.
//!
//! Desktop has no framework-owned installer, and iOS is forbidden by App
//! Store Review Guideline 3.3.2 from replacing its own binary — but both can
//! still tell an application a newer version exists. [`GitHubAppUpdater`] is
//! that check, written once against the framework's own [`HttpClient`] rather
//! than per platform, so every platform reads the same release feed the same
//! way and none of them link a second HTTP stack to do it.

use crate::app_update::{
    set_app_update_status, AppUpdateCapabilities, AppUpdateError, AppUpdateStatus, AppUpdater,
    GitHubReleaseUpdate, PackageDigest, UpdatePackage,
};
use crate::http::{default_http_client, HttpClient, HttpClientRef, HttpControl, HttpRequest};

/// Root of the GitHub REST API this checker reads.
///
/// Unauthenticated, so only public repositories and the API's anonymous rate
/// limit — the same limit a browser hits reading the same page.
const GITHUB_API_ROOT: &str = "https://api.github.com";

/// Discovers releases through a repository's public GitHub release feed.
///
/// [`AppUpdater::check`] starts the request on a thread of its own and
/// returns immediately; the outcome arrives later through
/// [`set_app_update_status`] — the same fire-and-forget contract Android's
/// JNI-backed updater uses, so an application observing update status cannot
/// tell which platform answered it.
pub struct GitHubAppUpdater {
    client: HttpClientRef,
    /// Whether the client behind this updater can actually make a request.
    ///
    /// The framework's own client needs the `http-native` feature, which is
    /// the application's choice to enable. Without it every request fails
    /// with [`crate::http::HttpError::UnsupportedFeature`], and an updater
    /// that still claimed it could check would be claiming a capability the
    /// build does not have — which is the exact thing
    /// [`AppUpdateCapabilities`] exists to prevent.
    can_reach_network: bool,
}

impl GitHubAppUpdater {
    /// An updater over the framework's [`default_http_client`].
    pub fn new() -> Self {
        Self {
            client: default_http_client(),
            can_reach_network: cfg!(feature = "http-native"),
        }
    }

    /// An updater over a specific client — the seam a test replaces with a
    /// [`crate::http::StubHttpClient`], and the one an application uses to
    /// supply a client of its own. A client the caller built is one the
    /// caller can send with.
    pub fn with_client(client: HttpClientRef) -> Self {
        Self {
            client,
            can_reach_network: true,
        }
    }
}

impl Default for GitHubAppUpdater {
    fn default() -> Self {
        Self::new()
    }
}

impl AppUpdater for GitHubAppUpdater {
    fn capabilities(&self) -> AppUpdateCapabilities {
        AppUpdateCapabilities {
            check: self.can_reach_network,
            // Neither desktop nor iOS has a framework-owned way to replace
            // its own binary, so this updater only ever discovers a release —
            // it never claims it can install one.
            install: false,
        }
    }

    fn check(&self, source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
        let client = self.client.clone();
        let source = source.clone();
        std::thread::Builder::new()
            .name("cranpose-app-update-check".to_string())
            .spawn(move || {
                let status = pollster::block_on(latest_release_status(client.as_ref(), &source));
                set_app_update_status(status);
            })
            .map_err(|error| AppUpdateError::Request(error.to_string()))?;
        Ok(())
    }
}

/// Fetches the latest release and turns it into the status this checker
/// publishes, folding every failure into [`AppUpdateStatus::Error`] rather
/// than letting the worker thread's result go nowhere.
async fn latest_release_status(
    client: &dyn HttpClient,
    source: &GitHubReleaseUpdate,
) -> AppUpdateStatus {
    match fetch_latest_release(client, source).await {
        Ok(release) => release_status(&release, source),
        Err(message) => AppUpdateStatus::Error(message),
    }
}

/// The fields this checker reads out of a GitHub release feed. Not every
/// field the API returns — only what a caller of [`GitHubAppUpdater`] needs
/// to decide whether a package is newer and to fetch it.
struct GitHubRelease {
    tag_name: String,
    notes: Option<String>,
    assets: Vec<GitHubReleaseAsset>,
}

struct GitHubReleaseAsset {
    name: String,
    download_url: String,
    size: Option<u64>,
    /// The `algorithm:hex` digest GitHub publishes per asset, when it did —
    /// older releases and third-party mirrors may carry none.
    digest: Option<String>,
}

impl GitHubRelease {
    fn from_json(value: &serde_json::Value) -> Option<Self> {
        let tag_name = value.get("tag_name")?.as_str()?.to_string();
        let notes = value
            .get("body")
            .and_then(serde_json::Value::as_str)
            .filter(|body| !body.trim().is_empty())
            .map(str::to_string);
        let assets = value
            .get("assets")
            .and_then(serde_json::Value::as_array)
            .map(|assets| {
                assets
                    .iter()
                    .filter_map(GitHubReleaseAsset::from_json)
                    .collect()
            })
            .unwrap_or_default();
        Some(Self {
            tag_name,
            notes,
            assets,
        })
    }
}

impl GitHubReleaseAsset {
    fn from_json(value: &serde_json::Value) -> Option<Self> {
        Some(Self {
            name: value.get("name")?.as_str()?.to_string(),
            download_url: value.get("browser_download_url")?.as_str()?.to_string(),
            size: value.get("size").and_then(serde_json::Value::as_u64),
            digest: value
                .get("digest")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string),
        })
    }
}

/// Sends the one request this checker needs and reads its body as JSON.
///
/// Errors are flattened to a message rather than kept as [`crate::http::HttpError`]
/// or [`serde_json::Error`]: the caller only ever turns them into
/// [`AppUpdateStatus::Error`], and a status is text, not a typed error a
/// caller branches on.
async fn fetch_latest_release(
    client: &dyn HttpClient,
    source: &GitHubReleaseUpdate,
) -> Result<GitHubRelease, String> {
    let url = format!(
        "{GITHUB_API_ROOT}/repos/{}/releases/latest",
        source.repository
    );
    let request = HttpRequest::get(url).header("Accept", "application/vnd.github+json");
    let response = client
        .send(&request, HttpControl::new())
        .await
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?;
    let body = response
        .read_text()
        .await
        .map_err(|error| error.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|error| format!("the release feed did not answer with JSON: {error}"))?;
    GitHubRelease::from_json(&value).ok_or_else(|| {
        format!(
            "the release feed for {} is missing tag_name or assets",
            source.repository
        )
    })
}

/// Turns a fetched release into the status [`GitHubAppUpdater::check`]
/// publishes: current if nothing newer was found, available with a package
/// when it was, or an error when the release is newer but carries none of
/// the assets this application asked for.
fn release_status(release: &GitHubRelease, source: &GitHubReleaseUpdate) -> AppUpdateStatus {
    let latest_version = version_from_tag(&release.tag_name);
    if !is_newer_version(latest_version, &source.current_version) {
        return AppUpdateStatus::UpToDate;
    }
    let Some(asset) = release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with(source.asset_suffix.as_str()))
    else {
        return AppUpdateStatus::Error(format!(
            "the latest release of {} ({}) has no asset ending in {}",
            source.repository, release.tag_name, source.asset_suffix
        ));
    };
    let mut package = UpdatePackage::new(latest_version, asset.download_url.clone());
    if let Some(size) = asset.size {
        package = package.with_size(size);
    }
    // A digest the feed did not publish, or published in a form this
    // framework cannot check, leaves the package unverifiable — which
    // `install_app_update` refuses rather than installing blind. Inventing
    // one here would hide that the feed never promised anything to check
    // against.
    if let Some(digest) = asset.digest.as_deref().and_then(PackageDigest::parse) {
        package = package.with_digest(digest);
    }
    if let Some(notes) = &release.notes {
        package = package.with_notes(notes.clone());
    }
    AppUpdateStatus::Available { package }
}

/// Strips a release tag's leading `v` (or `V`), the one prefix GitHub's own
/// tagging convention adds and this application's own version string never
/// carries.
fn version_from_tag(tag: &str) -> &str {
    let trimmed = tag.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
}

/// Parses a version string into numeric components, so they compare the way
/// a version number means to rather than the way its digits happen to sort:
/// `"10"` reads as `10`, not as a string that sorts before `"9"`.
///
/// A component that is not purely numeric (a pre-release suffix such as
/// `10-beta`) contributes its leading digits and drops the rest — enough to
/// order releases correctly without a general version grammar this framework
/// has no other use for.
fn version_components(version: &str) -> Vec<u64> {
    version_from_tag(version)
        .split('.')
        .map(|component| {
            component
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0)
        })
        .collect()
}

/// Whether `candidate` is a newer release than `current`, comparing version
/// components numerically component by component rather than as strings or
/// as whole numbers.
fn is_newer_version(candidate: &str, current: &str) -> bool {
    let mut candidate = version_components(candidate);
    let mut current = version_components(current);
    let len = candidate.len().max(current.len());
    candidate.resize(len, 0);
    current.resize(len, 0);
    candidate > current
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contract this checker relies on to decide whether a release is
    /// worth surfacing: version numbers compare numerically, not as strings
    /// or as whole dotted numbers, so `v0.1.10` — the release after nine
    /// patches — is correctly newer than `v0.1.9` rather than sorting before
    /// it the way `"10" < "9"` would as strings.
    #[test]
    fn version_comparison_is_numeric_per_component() {
        assert!(
            is_newer_version("v0.1.10", "v0.1.9"),
            "10 must sort after 9 numerically, not before it as strings would"
        );
        assert!(
            is_newer_version("0.1.10", "0.1.9"),
            "the comparison works the same without a leading v"
        );
        assert!(!is_newer_version("v0.1.9", "v0.1.10"));
        assert!(
            !is_newer_version("v1.2.3", "v1.2.3"),
            "identical versions are not newer than themselves"
        );
        assert!(is_newer_version("v1.3.0", "v1.2.9"));
        assert!(
            !is_newer_version("v1.2", "v1.2.0"),
            "a missing trailing component reads as zero, not as older"
        );
    }
}
