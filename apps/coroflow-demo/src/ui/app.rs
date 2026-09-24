use cranpose::prelude::*;
use cranpose_coroflow::rememberHandle;
use cranpose_navigation::{NavController, NavHost, NavOptions, rememberNavController};
use cranpose_ui::widgets::{PaddingValues, Scaffold};

use super::{
    diagnostics_screen::DiagnosticsScreen,
    note_screen::NoteScreen,
    notes_screen::{ActionButton, NotesScreen},
    theme::{PALETTE, caption, heading},
};
use crate::{
    di::{AppConfig, AppContainer, AppDispatchers},
    domain::model::NoteId,
};

const TITLE: &str = "Coroflow Notes";

/// Where the demo can go. Each screen gets its own view model store from the
/// `NavHost`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    /// The notes list, the start destination.
    Notes,
    /// One note.
    Note(NoteId),
    /// Which upstreams are running.
    Diagnostics,
}

const TABS: [(Screen, &str); 2] = [
    (Screen::Notes, "Notes"),
    (Screen::Diagnostics, "Diagnostics"),
];

/// The window this demo opens.
pub fn create_app() -> AppLauncher {
    AppLauncher::new().with_title(TITLE).with_size(760, 720)
}

/// The root composable: builds the object graph once and navigates between
/// the screens.
#[composable]
pub fn CoroflowDemoApp() {
    let container =
        rememberHandle(|| AppContainer::new(AppDispatchers::standard(), AppConfig::interactive()));
    let nav = rememberNavController(Screen::Notes);

    Scaffold(
        Modifier::empty()
            .fill_max_size()
            .background(PALETTE.background)
            .safe_area_padding(),
        || TopBar(),
        move || TabBar(nav),
        move |padding: PaddingValues| {
            Box(
                padding.apply_to(Modifier::empty().fill_max_size()),
                BoxSpec::default(),
                move || {
                    NavHost(nav, move |screen| match screen {
                        Screen::Notes => NotesScreen(container, nav),
                        Screen::Note(id) => NoteScreen(container, id, nav),
                        Screen::Diagnostics => DiagnosticsScreen(container),
                    });
                },
            );
        },
    );
}

#[composable]
fn TopBar() {
    Column(
        Modifier::empty()
            .fill_max_width()
            .padding(16.0)
            .background(PALETTE.surface),
        ColumnSpec::default(),
        || {
            Text(TITLE, Modifier::empty(), heading(PALETTE.text));
            Text(
                "Repository → use case → view model → screen, on coroflow",
                Modifier::empty(),
                caption(PALETTE.muted),
            );
        },
    );
}

#[composable]
fn TabBar(nav: NavController<Screen>) {
    let current = nav.current_route();
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(PALETTE.surface),
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceEvenly),
        move || {
            for (tab, label) in TABS {
                let color = if current == Some(tab) {
                    PALETTE.primary
                } else {
                    PALETTE.raised
                };
                ActionButton(label, color, move || {
                    nav.navigate_with(
                        tab,
                        NavOptions::new()
                            .pop_up_to(Screen::Notes, false)
                            .launch_single_top(),
                    );
                });
            }
        },
    );
}
