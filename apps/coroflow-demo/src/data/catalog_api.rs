use std::time::Duration;

use coroflow::{MutableStateFlow, StateFlow, delay};

use crate::domain::model::{CatalogError, CatalogHit};

/// Counts of catalog requests by how they ended.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestStats {
    /// Requests sent.
    pub started: u32,
    /// Requests that got an answer.
    pub completed: u32,
    /// Requests abandoned because the user typed past them.
    pub cancelled: u32,
}

/// A remote catalog reached over a slow network.
///
/// `search` is asynchronous like a real HTTP client: while it waits it holds
/// no thread, and dropping it abandons the request.
pub struct CatalogApi {
    latency: Duration,
    entries: Vec<CatalogHit>,
    stats: MutableStateFlow<RequestStats>,
}

impl CatalogApi {
    /// A catalog whose answers take `latency`.
    pub fn new(latency: Duration) -> Self {
        Self {
            latency,
            entries: catalog_entries(),
            stats: MutableStateFlow::new(RequestStats::default()),
        }
    }

    /// How requests have ended so far.
    pub fn stats(&self) -> StateFlow<RequestStats> {
        self.stats.as_state_flow()
    }

    /// The entries whose title or topic contains `query`.
    pub async fn search(&self, query: &str) -> Result<Vec<CatalogHit>, CatalogError> {
        let request = RequestGuard::start(&self.stats);
        delay(self.latency).await;
        request.complete();
        let needle = query.to_lowercase();
        if needle.contains("fail") {
            return Err(CatalogError::Unavailable);
        }
        Ok(self
            .entries
            .iter()
            .filter(|entry| {
                entry.title.to_lowercase().contains(&needle)
                    || entry.topic.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect())
    }
}

struct RequestGuard<'a> {
    stats: &'a MutableStateFlow<RequestStats>,
    completed: bool,
}

impl<'a> RequestGuard<'a> {
    fn start(stats: &'a MutableStateFlow<RequestStats>) -> Self {
        stats.update(|stats| RequestStats {
            started: stats.started + 1,
            ..*stats
        });
        Self {
            stats,
            completed: false,
        }
    }

    fn complete(mut self) {
        self.completed = true;
        self.stats.update(|stats| RequestStats {
            completed: stats.completed + 1,
            ..*stats
        });
    }
}

impl Drop for RequestGuard<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.stats.update(|stats| RequestStats {
                cancelled: stats.cancelled + 1,
                ..*stats
            });
        }
    }
}

fn catalog_entries() -> Vec<CatalogHit> {
    [
        ("Cold flows run once per collector", "flow"),
        ("StateFlow conflates to the latest value", "flow"),
        ("SharedFlow broadcasts events", "flow"),
        ("flatMapLatest cancels stale work", "flow"),
        ("debounce waits for a quiet period", "flow"),
        ("combine merges the latest of each input", "flow"),
        ("flowOn moves the upstream to a dispatcher", "coroutines"),
        ("withContext hops threads and back", "coroutines"),
        ("Structured concurrency: scopes own jobs", "coroutines"),
        ("viewModelScope dies with the view model", "architecture"),
        ("Repositories hide where data comes from", "architecture"),
        ("Use cases hold one business rule each", "architecture"),
        ("stateIn with WhileSubscribed(5000)", "architecture"),
        ("Rust futures cancel when dropped", "rust"),
        ("Send is inferred from what a future captures", "rust"),
        ("Rc stays on the main thread", "rust"),
        ("Recomposition reads snapshot state", "compose"),
        ("collectAsState turns a flow into state", "compose"),
        ("snapshotFlow turns state into a flow", "compose"),
        ("LaunchedEffect ties work to a key", "compose"),
    ]
    .into_iter()
    .map(|(title, topic)| CatalogHit {
        title: title.to_string(),
        topic: topic.to_string(),
    })
    .collect()
}
