use std::time::Duration;

use coroflow::FlowExt;
use cranpose::prelude::*;
use cranpose_coroflow::{CollectFlow, Handle, StateFlowCollect, snapshotFlow, viewModel};
use cranpose_foundation::text::TextFieldState;
use cranpose_navigation::NavController;

use super::{
    app::Screen,
    theme::{PALETTE, body, caption},
};
use crate::{
    di::AppContainer,
    domain::model::{CatalogResults, Note, NotesList, SyncStatus},
    presentation::{
        note_row_view_model::NoteRowViewModel,
        notes_messages::{NotesEvent, NotesMessages},
        notes_view_model::NotesViewModel,
    },
};

/// How long a snackbar message stays up.
pub const SNACKBAR_DURATION: Duration = Duration::from_millis(2_500);

/// The most catalog hits shown at once.
pub const MAX_VISIBLE_HITS: usize = 4;

/// The notes screen: search, catalog results, the note list and the sync bar.
#[composable]
pub fn NotesScreen(container: Handle<AppContainer>, nav: NavController<Screen>) {
    let messages = viewModel((), |_| NotesMessages::default());
    let view_model = viewModel((), move |scope| {
        NotesViewModel::new(scope, container.get().notes_use_cases(), messages.get())
    });
    let state = view_model
        .get()
        .ui_state()
        .collectAsStateWithLifecycle()
        .get();
    // The view model outlives this screen while another covers it; the
    // search box starts from its query so coming back shows the same list.
    let search = remember(|| TextFieldState::new(view_model.get().query())).with(|state| *state);
    let draft = remember(|| TextFieldState::new("")).with(|state| *state);
    let snackbar = rememberMutableStateOf(|| None::<String>);

    CollectFlow((), snapshotFlow(move || search.text()), move |text| {
        view_model.get().on_query_changed(text);
    });
    CollectFlow(
        (),
        messages
            .get()
            .events()
            .transform_latest(async |event: NotesEvent, emitter| {
                emitter.emit(Some(event.message())).await;
                coroflow::delay(SNACKBAR_DURATION).await;
                emitter.emit(None).await;
            }),
        move |message| snackbar.set(message),
    );

    Column(
        Modifier::empty().fill_max_size().padding(16.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            TextInput(search, "Search notes and the catalog…");
            CatalogPanel(state.catalog.clone());
            NewNoteRow(draft, view_model);
            NotesColumn(state.notes.clone(), container, messages, nav);
            SyncBar(state.sync, state.notes.visible.len(), state.notes.total);
            if let Some(message) = snackbar.value() {
                Snackbar(message);
            }
        },
    );
}

#[composable]
fn TextInput(field: TextFieldState, placeholder: &'static str) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .padding(10.0)
            .background(PALETTE.raised)
            .rounded_corners(8.0),
        BoxSpec::default(),
        move || {
            if field.text().is_empty() {
                Text(placeholder, Modifier::empty(), body(PALETTE.muted));
            }
            BasicTextField(
                field,
                Modifier::empty().fill_max_width(),
                body(PALETTE.text),
            );
        },
    );
}

#[composable]
fn CatalogPanel(catalog: CatalogResults) {
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(PALETTE.surface)
            .rounded_corners(8.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(4.0)),
        move || {
            Text("Remote catalog", Modifier::empty(), caption(PALETTE.muted));
            match &catalog {
                CatalogResults::Idle => {
                    Text(
                        "Type two or more characters to search.",
                        Modifier::empty(),
                        body(PALETTE.muted),
                    );
                }
                CatalogResults::Loading { query } => {
                    Text(
                        format!("Searching \u{201c}{query}\u{201d}…"),
                        Modifier::empty(),
                        body(PALETTE.primary),
                    );
                }
                CatalogResults::Failed { query, message } => {
                    Text(
                        format!("\u{201c}{query}\u{201d}: {message}"),
                        Modifier::empty(),
                        body(PALETTE.danger),
                    );
                }
                CatalogResults::Loaded { query, hits } if hits.is_empty() => {
                    Text(
                        format!("Nothing about \u{201c}{query}\u{201d}."),
                        Modifier::empty(),
                        body(PALETTE.muted),
                    );
                }
                CatalogResults::Loaded { hits, .. } => {
                    for hit in hits.iter().take(MAX_VISIBLE_HITS) {
                        Text(
                            format!("{} · {}", hit.title, hit.topic),
                            Modifier::empty(),
                            body(PALETTE.text),
                        );
                    }
                    if hits.len() > MAX_VISIBLE_HITS {
                        Text(
                            format!("and {} more", hits.len() - MAX_VISIBLE_HITS),
                            Modifier::empty(),
                            caption(PALETTE.muted),
                        );
                    }
                }
            }
        },
    );
}

#[composable]
fn NewNoteRow(draft: TextFieldState, view_model: Handle<NotesViewModel>) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Box(
                Modifier::empty().weight(1.0),
                BoxSpec::default(),
                move || {
                    TextInput(draft, "New note…");
                },
            );
            ActionButton("Add", PALETTE.primary, move || {
                view_model.get().on_add_note(draft.text());
                draft.set_text("");
            });
        },
    );
}

#[composable]
fn NotesColumn(
    notes: NotesList,
    container: Handle<AppContainer>,
    messages: Handle<NotesMessages>,
    nav: NavController<Screen>,
) {
    if notes.visible.is_empty() {
        Text(
            if notes.query.trim().is_empty() {
                "No notes yet."
            } else {
                "No note matches the search."
            },
            Modifier::empty(),
            body(PALETTE.muted),
        );
        return;
    }
    let list_state = rememberLazyListState();
    let rows = notes.visible;
    LazyColumn(
        Modifier::empty().fill_max_width().weight(1.0),
        list_state,
        LazyColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(6.0)),
        move |scope| {
            let keys = rows.clone();
            let items = rows.clone();
            scope.items(
                LazyItems::new(items.len()).key(move |index| keys[index].id.0),
                move |index| NoteRow(items[index].clone(), container, messages, nav),
            );
        },
    );
}

/// One note. The row asks for its own view model, keyed by the note, so the
/// list above it only hands it the note and knows nothing about pinning or
/// deleting.
#[composable]
fn NoteRow(
    note: Note,
    container: Handle<AppContainer>,
    messages: Handle<NotesMessages>,
    nav: NavController<Screen>,
) {
    let row = viewModel(note.id, move |scope| {
        NoteRowViewModel::new(scope, container.get().notes_use_cases(), messages.get())
    });
    let busy = row.get().busy().collectAsState().get();
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(10.0)
            .background(PALETTE.surface)
            .rounded_corners(8.0),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let pinned_note = note.clone();
            ActionButton(
                if note.pinned { "Unpin" } else { "Pin" },
                if note.pinned {
                    PALETTE.primary
                } else {
                    PALETTE.raised
                },
                move || row.get().on_toggle_pinned(pinned_note.clone()),
            );
            let id = note.id;
            Text(
                if busy {
                    format!("{}…", note.title)
                } else {
                    note.title.clone()
                },
                Modifier::empty()
                    .weight(1.0)
                    .clickable(move |_| nav.navigate(Screen::Note(id))),
                body(PALETTE.text),
            );
            let deleted_note = note.clone();
            ActionButton("Delete", PALETTE.danger, move || {
                row.get().on_delete(deleted_note.clone());
            });
        },
    );
}

#[composable]
fn SyncBar(sync: SyncStatus, visible: usize, total: usize) {
    let (label, color) = match sync {
        SyncStatus::Idle => ("Sync idle".to_string(), PALETTE.muted),
        SyncStatus::Syncing { round } => (format!("Syncing, round {round}…"), PALETTE.primary),
        SyncStatus::Synced { round } => (format!("Synced, round {round}"), PALETTE.good),
    };
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(10.0)
            .background(PALETTE.surface)
            .rounded_corners(8.0),
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceBetween),
        move || {
            Text(label.clone(), Modifier::empty(), caption(color));
            Text(
                format!("{visible} of {total} notes"),
                Modifier::empty(),
                caption(PALETTE.muted),
            );
        },
    );
}

#[composable]
fn Snackbar(message: String) {
    Box(
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(PALETTE.raised)
            .rounded_corners(8.0),
        BoxSpec::default(),
        move || {
            Text(message.clone(), Modifier::empty(), body(PALETTE.text));
        },
    );
}

/// A rounded text button.
#[composable]
pub fn ActionButton(label: &'static str, color: Color, on_click: impl Fn() + 'static) {
    let text_color = if color == PALETTE.raised {
        PALETTE.text
    } else {
        PALETTE.on_primary
    };
    Button(
        Modifier::empty()
            .padding(8.0)
            .background(color)
            .rounded_corners(6.0),
        ButtonSpec::default(),
        on_click,
        move || {
            Text(label, Modifier::empty(), body(text_color));
        },
    );
}
