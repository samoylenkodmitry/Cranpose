use std::time::Duration;

use coroflow::{
    CoroutineScope, Dispatcher, FlowExt, MutableStateFlow, SharingStarted, StateFlow, delay, flow,
};

use crate::domain::{model::SyncStatus, repository::SyncRepository};

/// Timing of the background sync.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyncConfig {
    /// How long one sync round works.
    pub work: Duration,
    /// The pause between rounds.
    pub period: Duration,
    /// How long the sync keeps running after its last collector leaves.
    pub stop_timeout: Duration,
}

/// An application-wide background sync — a hot flow shared by every screen.
///
/// It owns a [`CoroutineScope`] on a background dispatcher and runs only while
/// something collects [`status`](SyncRepository::status), stopping
/// `stop_timeout` after the last collector leaves.
pub struct SyncService {
    status: StateFlow<SyncStatus>,
    running: MutableStateFlow<bool>,
    starts: MutableStateFlow<u32>,
    _scope: CoroutineScope,
}

impl SyncService {
    /// A sync that runs on `dispatcher` with `config`'s timing.
    pub fn new(dispatcher: Dispatcher, config: SyncConfig) -> Self {
        let scope = CoroutineScope::new(dispatcher);
        let running = MutableStateFlow::new(false);
        let starts = MutableStateFlow::new(0);
        let (running_on_start, starts_on_start, running_on_stop) =
            (running.clone(), starts.clone(), running.clone());
        let status = flow(move |emitter| async move {
            let mut round = 0;
            loop {
                round += 1;
                emitter.emit(SyncStatus::Syncing { round }).await;
                delay(config.work).await;
                emitter.emit(SyncStatus::Synced { round }).await;
                delay(config.period).await;
            }
        })
        .on_start(move || {
            running_on_start.set(true);
            starts_on_start.update(|starts| starts + 1);
        })
        .on_completion(move || running_on_stop.set(false))
        .state_in(
            &scope,
            SharingStarted::while_subscribed(config.stop_timeout),
            SyncStatus::Idle,
        );
        Self {
            status,
            running,
            starts,
            _scope: scope,
        }
    }

    /// Whether the sync loop is running right now.
    pub fn upstream_running(&self) -> StateFlow<bool> {
        self.running.as_state_flow()
    }

    /// How many times the sync loop has started.
    pub fn upstream_starts(&self) -> StateFlow<u32> {
        self.starts.as_state_flow()
    }
}

impl SyncRepository for SyncService {
    fn status(&self) -> StateFlow<SyncStatus> {
        self.status.clone()
    }
}
