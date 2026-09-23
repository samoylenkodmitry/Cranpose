use cranpose::prelude::*;
use cranpose_coroflow::{Handle, StateFlowCollect};

use super::theme::{PALETTE, body, caption, heading};
use crate::{di::AppContainer, presentation::notes_view_model::NotesViewModel};

/// Live view of which upstreams are running, and why.
#[composable]
pub fn DiagnosticsScreen(container: Handle<AppContainer>, view_model: Handle<NotesViewModel>) {
    let ui_running = view_model
        .get()
        .ui_upstream_running()
        .collectAsState()
        .get();
    let graph = container.get();
    let sync_running = graph.sync_running().collectAsState().get();
    let sync_starts = graph.sync_starts().collectAsState().get();
    let requests = graph.catalog_stats().collectAsState().get();

    Column(
        Modifier::empty().fill_max_size().padding(16.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            Text(
                "Who is running right now",
                Modifier::empty(),
                heading(PALETTE.text),
            );
            StatusRow(
                "Notes screen state (view model, WhileSubscribed 5 s)",
                ui_running,
                String::new(),
            );
            StatusRow(
                "Background sync (app scope, WhileSubscribed 5 s)",
                sync_running,
                format!("started {sync_starts}×"),
            );
            Row(
                Modifier::empty()
                    .fill_max_width()
                    .padding(12.0)
                    .background(PALETTE.surface)
                    .rounded_corners(8.0),
                RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
                move || {
                    Text("Catalog requests", Modifier::empty(), body(PALETTE.text));
                    Text(
                        format!(
                            "{} sent · {} answered · {} cancelled",
                            requests.started, requests.completed, requests.cancelled
                        ),
                        Modifier::empty(),
                        body(PALETTE.muted),
                    );
                },
            );
            Text(
                "This screen does not collect the notes state. Switch here from Notes and \
                 the view model's state stops 5 s later; that releases the sync, which \
                 stops 5 s after that. Go back within 5 s and nothing restarts: the \
                 start counter stays put. Type quickly in Notes and watch stale catalog \
                 requests get cancelled by flatMapLatest.",
                Modifier::empty(),
                caption(PALETTE.muted),
            );
        },
    );
}

#[composable]
fn StatusRow(label: &'static str, running: bool, detail: String) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(PALETTE.surface)
            .rounded_corners(8.0),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Text(label, Modifier::empty().weight(1.0), body(PALETTE.text));
            Text(
                detail.clone(),
                Modifier::empty().padding(8.0),
                caption(PALETTE.muted),
            );
            Text(
                if running { "running" } else { "stopped" },
                Modifier::empty(),
                body(if running {
                    PALETTE.good
                } else {
                    PALETTE.danger
                }),
            );
        },
    );
}
