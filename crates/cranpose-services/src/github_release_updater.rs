//! A portable [`AppUpdater`] that discovers a newer release from a GitHub
//! repository's release feed.
//!
//! Desktop has no framework-owned installer, and iOS is forbidden by App
//! Store Review Guideline 3.3.2 from replacing its own binary — but both can
//! still tell an application a newer version exists. [`GitHubAppUpdater`] is
//! that check, written once against the framework's own [`HttpClient`] rather
//! than per platform, so every platform reads the same release feed the same
//! way and none of them link a second HTTP stack to do it.

use crate::{
    app_update::{
        AppUpdateCapabilities, AppUpdateError, AppUpdateStatus, AppUpdater, GitHubReleaseUpdate,
        PackageDigest, UpdatePackage, set_app_update_status,
    },
    http::{HttpClient, HttpClientRef, HttpControl, HttpRequest, default_http_client},
};

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

async fn latest_release_status(
    client: &dyn HttpClient,
    source: &GitHubReleaseUpdate,
) -> AppUpdateStatus {
    match fetch_latest_release(client, source).await {
        Ok(release) => release_status(&release, source),
        Err(message) => AppUpdateStatus::Error(message),
    }
}

struct GitHubRelease {
    tag_name: String,
    notes: Option<String>,
    assets: Vec<GitHubReleaseAsset>,
}

struct GitHubReleaseAsset {
    name: String,
    download_url: String,
    size: Option<u64>,
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
    if let Some(digest) = asset.digest.as_deref().and_then(PackageDigest::parse) {
        package = package.with_digest(digest);
    }
    if let Some(notes) = &release.notes {
        package = package.with_notes(notes.clone());
    }
    AppUpdateStatus::Available { package }
}

fn version_from_tag(tag: &str) -> &str {
    let trimmed = tag.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
}

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
