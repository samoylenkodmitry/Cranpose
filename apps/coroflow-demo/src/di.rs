//! Manual dependency injection — the `AppContainer` of Android's architecture
//! guide. Swapping [`AppDispatchers`] for a
//! [`TestScheduler`](coroflow::TestScheduler)'s dispatchers runs the whole
//! graph on virtual time.

use std::{sync::Arc, time::Duration};

use coroflow::{Dispatcher, Dispatchers, MutableStateFlow, StateFlow};

use crate::{
    data::{
        catalog_api::{CatalogApi, RequestStats},
        notes_store::{NotesStore, starter_notes},
        repository::DefaultNotesRepository,
        sync::{SyncConfig, SyncService},
    },
    domain::{
        repository::{NotesRepository, SyncRepository},
        use_cases::{
            AddNote, DeleteNote, ObserveNote, ObserveNotes, ObserveSync, SearchCatalog,
            TogglePinned,
        },
    },
    presentation::notes_view_model::NotesUseCases,
};

/// The dispatchers the object graph runs on.
#[derive(Clone)]
pub struct AppDispatchers {
    /// For blocking I/O.
    pub io: Dispatcher,
    /// For CPU-bound work and background services.
    pub compute: Dispatcher,
}

impl AppDispatchers {
    /// The process-wide pools.
    pub fn standard() -> Self {
        Self {
            io: Dispatchers::io(),
            compute: Dispatchers::default_pool(),
        }
    }
}

/// Latencies and timings of the simulated backends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppConfig {
    /// How long a local write blocks.
    pub disk_latency: Duration,
    /// How long a catalog request takes.
    pub network_latency: Duration,
    /// The background sync's timing.
    pub sync: SyncConfig,
}

impl AppConfig {
    /// Latencies slow enough to watch in the running app.
    pub fn interactive() -> Self {
        Self {
            disk_latency: Duration::from_millis(150),
            network_latency: Duration::from_millis(900),
            sync: SyncConfig {
                work: Duration::from_millis(800),
                period: Duration::from_millis(2_000),
                stop_timeout: Duration::from_secs(5),
            },
        }
    }
}

/// The application's object graph.
pub struct AppContainer {
    repository: Arc<dyn NotesRepository>,
    catalog: Arc<CatalogApi>,
    sync: Arc<SyncService>,
    dispatchers: AppDispatchers,
    notes_state_running: MutableStateFlow<bool>,
}

impl AppContainer {
    /// Builds the graph on `dispatchers` with `config`'s timings.
    pub fn new(dispatchers: AppDispatchers, config: AppConfig) -> Self {
        let store = Arc::new(NotesStore::new(starter_notes(), config.disk_latency));
        let catalog = Arc::new(CatalogApi::new(config.network_latency));
        let repository: Arc<dyn NotesRepository> = Arc::new(DefaultNotesRepository::new(
            store,
            Arc::clone(&catalog),
            dispatchers.io.clone(),
        ));
        let sync = Arc::new(SyncService::new(dispatchers.compute.clone(), config.sync));
        Self {
            repository,
            catalog,
            sync,
            dispatchers,
            notes_state_running: MutableStateFlow::new(false),
        }
    }

    /// The use cases the notes screen needs.
    pub fn notes_use_cases(&self) -> NotesUseCases {
        let repository = &self.repository;
        NotesUseCases {
            observe_notes: ObserveNotes::new(
                Arc::clone(repository),
                self.dispatchers.compute.clone(),
            ),
            observe_note: ObserveNote::new(Arc::clone(repository)),
            search_catalog: SearchCatalog::new(Arc::clone(repository)),
            add_note: AddNote::new(Arc::clone(repository)),
            toggle_pinned: TogglePinned::new(Arc::clone(repository)),
            delete_note: DeleteNote::new(Arc::clone(repository)),
            observe_sync: ObserveSync::new(Arc::clone(&self.sync) as Arc<dyn SyncRepository>),
            state_running: self.notes_state_running.clone(),
        }
    }

    /// Whether the notes screen state is being computed.
    pub fn notes_state_running(&self) -> StateFlow<bool> {
        self.notes_state_running.as_state_flow()
    }

    /// How catalog requests have ended.
    pub fn catalog_stats(&self) -> StateFlow<RequestStats> {
        self.catalog.stats()
    }

    /// Whether the background sync loop is running.
    pub fn sync_running(&self) -> StateFlow<bool> {
        self.sync.upstream_running()
    }

    /// How many times the background sync loop has started.
    pub fn sync_starts(&self) -> StateFlow<u32> {
        self.sync.upstream_starts()
    }
}
