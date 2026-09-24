use coroflow::FlowExt;
use cranpose::prelude::*;
use cranpose_coroflow::{CollectFlow, Handle, StateFlowCollect, viewModel};
use cranpose_navigation::NavController;

use super::{
    app::Screen,
    notes_screen::ActionButton,
    theme::{PALETTE, body, caption, heading},
};
use crate::{
    di::AppContainer, domain::model::NoteId, presentation::note_view_model::NoteViewModel,
};

/// One note, with its own view model for as long as this visit of the screen
/// stays on the back stack.
#[composable]
pub fn NoteScreen(container: Handle<AppContainer>, id: NoteId, nav: NavController<Screen>) {
    let view_model = viewModel((), move |scope| {
        NoteViewModel::new(scope, id, container.get().notes_use_cases())
    });
    let note = view_model.get().note().collectAsStateWithLifecycle().get();
    CollectFlow(
        (),
        view_model.get().deleted().filter(|deleted| *deleted),
        move |_| {
            nav.navigate_up();
        },
    );

    Column(
        Modifier::empty().fill_max_size().padding(16.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(12.0)),
        move || {
            ActionButton("Back", PALETTE.raised, move || {
                nav.navigate_up();
            });
            let Some(note) = note.clone() else {
                Text("Loading…", Modifier::empty(), body(PALETTE.muted));
                return;
            };
            Text(note.title.clone(), Modifier::empty(), heading(PALETTE.text));
            Text(
                format!(
                    "Note #{}{}",
                    note.id.0,
                    if note.pinned { " · pinned" } else { "" }
                ),
                Modifier::empty(),
                caption(PALETTE.muted),
            );
            Row(
                Modifier::empty(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(8.0)),
                move || {
                    ActionButton(
                        if note.pinned { "Unpin" } else { "Pin" },
                        PALETTE.primary,
                        move || view_model.get().on_toggle_pinned(),
                    );
                    ActionButton("Delete", PALETTE.danger, move || {
                        view_model.get().on_delete();
                    });
                },
            );
        },
    );
}
