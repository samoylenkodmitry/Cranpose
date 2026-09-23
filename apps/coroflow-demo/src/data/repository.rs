use std::sync::Arc;

use coroflow::{BoxFlow, BoxFuture, Dispatcher, FlowExt, TaskFailed, flow, with_context};

use super::{catalog_api::CatalogApi, notes_store::NotesStore};
use crate::domain::{
    model::{CatalogResults, Note, NoteId, NotesError},
    repository::NotesRepository,
};

/// The production [`NotesRepository`]: a local store plus the remote catalog.
///
/// Blocking store writes run on `io` through `with_context`; catalog requests
/// and their parsing run on `io` through `flow_on`. Callers may use it from
/// any thread.
pub struct DefaultNotesRepository {
    store: Arc<NotesStore>,
    catalog: Arc<CatalogApi>,
    io: Dispatcher,
}

impl DefaultNotesRepository {
    /// A repository over `store` and `catalog` that blocks only on `io`.
    pub fn new(store: Arc<NotesStore>, catalog: Arc<CatalogApi>, io: Dispatcher) -> Self {
        Self { store, catalog, io }
    }

    fn write<T, W>(&self, write: W) -> BoxFuture<Result<T, NotesError>>
    where
        T: Send + 'static,
        W: FnOnce(&NotesStore) -> Result<T, NotesError> + Send + 'static,
    {
        let store = Arc::clone(&self.store);
        let io = self.io.clone();
        Box::pin(async move {
            with_context(&io, async move { write(&store) })
                .await
                .map_err(|TaskFailed| NotesError::Storage)
                .and_then(|result| result)
        })
    }
}

impl NotesRepository for DefaultNotesRepository {
    fn observe_notes(&self) -> BoxFlow<Vec<Note>> {
        self.store.observe().boxed()
    }

    fn search_catalog(&self, query: String) -> BoxFlow<CatalogResults> {
        let catalog = Arc::clone(&self.catalog);
        flow(move |emitter| {
            let catalog = Arc::clone(&catalog);
            let query = query.clone();
            async move {
                emitter
                    .emit(CatalogResults::Loading {
                        query: query.clone(),
                    })
                    .await;
                let outcome = match catalog.search(&query).await {
                    Ok(hits) => CatalogResults::Loaded { query, hits },
                    Err(error) => CatalogResults::Failed {
                        query,
                        message: error.to_string(),
                    },
                };
                emitter.emit(outcome).await;
            }
        })
        .flow_on(self.io.clone())
        .boxed()
    }

    fn add_note(&self, title: String) -> BoxFuture<Result<Note, NotesError>> {
        self.write(move |store| Ok(store.insert(title)))
    }

    fn set_pinned(&self, id: NoteId, pinned: bool) -> BoxFuture<Result<(), NotesError>> {
        self.write(move |store| store.set_pinned(id, pinned))
    }

    fn delete_note(&self, id: NoteId) -> BoxFuture<Result<Note, NotesError>> {
        self.write(move |store| store.delete(id))
    }
}
