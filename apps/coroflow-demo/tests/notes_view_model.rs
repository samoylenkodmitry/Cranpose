use std::{task::Poll, time::Duration};

use coroflow::{Flow, MainScope, TestScheduler, Turbine};
use coroflow_demo::{
    data::{catalog_api::RequestStats, sync::SyncConfig},
    di::{AppConfig, AppContainer, AppDispatchers},
    domain::{
        model::{CatalogResults, Note, NoteId, SyncStatus},
        use_cases::filter_notes,
    },
    presentation::notes_view_model::{NotesEvent, NotesViewModel},
};

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

struct Harness {
    scheduler: TestScheduler,
    container: AppContainer,
    view_model: NotesViewModel,
}

impl Harness {
    fn new() -> Self {
        let scheduler = TestScheduler::new();
        let dispatchers = AppDispatchers {
            io: scheduler.dispatcher(),
            compute: scheduler.dispatcher(),
        };
        let config = AppConfig {
            disk_latency: Duration::ZERO,
            network_latency: ms(900),
            sync: SyncConfig {
                work: ms(800),
                period: ms(2_000),
                stop_timeout: ms(5_000),
            },
        };
        let container = AppContainer::new(dispatchers, config);
        let view_model = NotesViewModel::new(
            MainScope::new(scheduler.main_dispatcher()),
            container.notes_use_cases(),
        );
        Self {
            scheduler,
            container,
            view_model,
        }
    }

    fn catalog(&self) -> CatalogResults {
        self.view_model.ui_state().value().catalog
    }

    fn requests(&self) -> RequestStats {
        self.container.catalog_stats().value()
    }
}

#[test]
fn the_first_screen_state_arrives_without_waiting_for_the_debounce() {
    let harness = Harness::new();
    let _screen = harness.view_model.ui_state().open();
    harness.scheduler.run_current();
    let state = harness.view_model.ui_state().value();
    assert_eq!(state.notes.total, 4);
    assert_eq!(state.notes.visible.len(), 4);
    assert!(state.notes.visible[0].pinned, "pinned notes come first");
    assert_eq!(state.catalog, CatalogResults::Idle);
    assert_eq!(state.sync, SyncStatus::Syncing { round: 1 });
    assert_eq!(harness.scheduler.now(), Duration::ZERO);
}

#[test]
fn local_filtering_follows_every_keystroke() {
    let harness = Harness::new();
    let _screen = harness.view_model.ui_state().open();
    harness.view_model.on_query_changed("KEEP".to_string());
    harness.scheduler.run_current();
    let notes = harness.view_model.ui_state().value().notes;
    assert_eq!(notes.query, "KEEP");
    assert_eq!(notes.visible.len(), 1);
    assert_eq!(notes.visible[0].title, "Pin me to keep me on top");
}

#[test]
fn the_catalog_is_searched_only_for_settled_queries_and_stale_requests_are_cancelled() {
    let harness = Harness::new();
    let _screen = harness.view_model.ui_state().open();
    harness.scheduler.run_current();

    harness.view_model.on_query_changed("fl".to_string());
    harness.scheduler.advance_time_by(ms(100));
    harness.view_model.on_query_changed("flow".to_string());
    harness.scheduler.advance_time_by(ms(399));
    assert_eq!(harness.catalog(), CatalogResults::Idle, "still debouncing");
    harness.scheduler.advance_time_by(ms(1));
    assert_eq!(
        harness.catalog(),
        CatalogResults::Loading {
            query: "flow".to_string()
        }
    );
    harness.scheduler.advance_time_by(ms(900));
    assert!(matches!(
        harness.catalog(),
        CatalogResults::Loaded { query, hits } if query == "flow"
            && !hits.is_empty()
            && hits.iter().all(|hit| format!("{} {}", hit.title, hit.topic).to_lowercase().contains("flow"))
    ));
    assert_eq!(
        harness.requests(),
        RequestStats {
            started: 1,
            completed: 1,
            cancelled: 0
        },
        "the debounce kept \"fl\" from ever being sent"
    );

    harness.view_model.on_query_changed("st".to_string());
    harness.scheduler.advance_time_by(ms(400));
    assert!(matches!(harness.catalog(), CatalogResults::Loading { .. }));
    harness.scheduler.advance_time_by(ms(300));
    harness.view_model.on_query_changed("sta".to_string());
    harness.scheduler.advance_time_by(ms(400 + 900));
    assert!(matches!(
        harness.catalog(),
        CatalogResults::Loaded { query, .. } if query == "sta"
    ));
    assert_eq!(
        harness.requests(),
        RequestStats {
            started: 3,
            completed: 2,
            cancelled: 1
        },
        "typing past \"st\" cancelled its request mid-flight"
    );
}

#[test]
fn a_failing_catalog_is_reported_in_the_state() {
    let harness = Harness::new();
    let _screen = harness.view_model.ui_state().open();
    harness.view_model.on_query_changed("fail".to_string());
    harness.scheduler.advance_time_by(ms(1_300));
    assert_eq!(
        harness.catalog(),
        CatalogResults::Failed {
            query: "fail".to_string(),
            message: "the catalog service is unavailable".to_string()
        }
    );
}

#[test]
fn intents_update_storage_and_emit_one_shot_events() {
    let harness = Harness::new();
    let _screen = harness.view_model.ui_state().open();
    let mut events = Turbine::of(&harness.view_model.events());
    harness.scheduler.run_current();

    harness.view_model.on_add_note("  Buy milk  ".to_string());
    harness.scheduler.run_current();
    assert_eq!(
        events.next_now(),
        Poll::Ready(Some(NotesEvent::Added("Buy milk".to_string())))
    );
    let state = harness.view_model.ui_state().value();
    assert_eq!(state.notes.total, 5);
    let added = state
        .notes
        .visible
        .iter()
        .find(|note| note.title == "Buy milk")
        .cloned();

    harness.view_model.on_add_note("   ".to_string());
    harness.scheduler.run_current();
    assert_eq!(
        events.next_now(),
        Poll::Ready(Some(NotesEvent::Failed("a note needs a title".to_string())))
    );

    if let Some(note) = added {
        harness.view_model.on_toggle_pinned(note.clone());
        harness.scheduler.run_current();
        let first = harness.view_model.ui_state().value().notes.visible[0].clone();
        assert_eq!(
            (first.id, first.pinned),
            (note.id, true),
            "pinning moves it to the top"
        );

        harness.view_model.on_delete(note.id);
        harness.scheduler.run_current();
        assert_eq!(
            events.next_now(),
            Poll::Ready(Some(NotesEvent::Deleted("Buy milk".to_string())))
        );
    }
    harness.view_model.on_delete(NoteId(999));
    harness.scheduler.run_current();
    assert_eq!(
        events.next_now(),
        Poll::Ready(Some(NotesEvent::Failed(
            "that note no longer exists".to_string()
        )))
    );
    assert_eq!(harness.view_model.ui_state().value().notes.total, 4);
}

#[test]
fn leaving_the_screen_stops_its_state_and_then_the_app_wide_sync() {
    let harness = Harness::new();
    let ui_running = harness.view_model.ui_upstream_running();
    let sync_running = harness.container.sync_running();
    let sync_starts = harness.container.sync_starts();

    let screen = harness.view_model.ui_state().open();
    harness.scheduler.advance_time_by(ms(3_000));
    assert!(ui_running.value() && sync_running.value());
    assert_eq!(
        harness.view_model.ui_state().value().sync,
        SyncStatus::Syncing { round: 2 }
    );

    drop(screen);
    harness.scheduler.advance_time_by(ms(4_999));
    assert!(ui_running.value(), "the view model keeps its state for 5 s");
    harness.scheduler.advance_time_by(ms(1));
    assert!(!ui_running.value(), "then stops computing it");
    assert!(
        sync_running.value(),
        "the sync has its own 5 s grace period"
    );
    harness.scheduler.advance_time_by(ms(5_000));
    assert!(
        !sync_running.value(),
        "with no collector left, the sync stops"
    );

    let _back = harness.view_model.ui_state().open();
    harness.scheduler.run_current();
    assert!(ui_running.value() && sync_running.value());
    assert_eq!(
        sync_starts.value(),
        2,
        "coming back after both stopped restarts the sync"
    );
}

#[test]
fn returning_within_the_timeout_restarts_nothing() {
    let harness = Harness::new();
    let screen = harness.view_model.ui_state().open();
    harness.scheduler.advance_time_by(ms(500));
    drop(screen);
    harness.scheduler.advance_time_by(ms(3_000));
    let _back = harness.view_model.ui_state().open();
    harness.scheduler.advance_time_by(ms(20_000));
    assert_eq!(harness.container.sync_starts().value(), 1);
    assert!(harness.view_model.ui_upstream_running().value());
}

#[test]
fn dropping_the_view_model_cancels_its_work() {
    let harness = Harness::new();
    let screen = harness.view_model.ui_state().open();
    harness.scheduler.run_current();
    let sync_running = harness.container.sync_running();
    let Harness {
        scheduler,
        container,
        view_model,
    } = harness;
    drop(screen);
    drop(view_model);
    scheduler.run_current();
    scheduler.advance_time_by(ms(5_000));
    assert!(
        !sync_running.value(),
        "no view model left to keep the sync alive"
    );
    drop(container);
}

#[test]
fn filter_notes_matches_case_insensitively_and_sorts_pinned_then_newest() {
    let notes = vec![
        Note {
            id: NoteId(1),
            title: "Flow basics".to_string(),
            pinned: false,
        },
        Note {
            id: NoteId(2),
            title: "Unrelated".to_string(),
            pinned: false,
        },
        Note {
            id: NoteId(3),
            title: "More FLOW".to_string(),
            pinned: false,
        },
        Note {
            id: NoteId(0),
            title: "pinned flow".to_string(),
            pinned: true,
        },
    ];
    let list = filter_notes(" flow ", &notes);
    let ids: Vec<u64> = list.visible.iter().map(|note| note.id.0).collect();
    assert_eq!(ids, vec![0, 3, 1]);
    assert_eq!(list.total, 4);
    assert_eq!(filter_notes("", &notes).visible.len(), 4);
}
