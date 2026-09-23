use std::rc::Rc;

use cranpose::prelude::*;
use cranpose_coroflow::rememberViewModel;
use cranpose_ui::widgets::{PaddingValues, Scaffold};

use super::{
    Handle,
    diagnostics_screen::DiagnosticsScreen,
    notes_screen::{ActionButton, NotesScreen},
    theme::{PALETTE, caption, heading},
};
use crate::{
    di::{AppConfig, AppContainer, AppDispatchers},
    presentation::notes_view_model::NotesViewModel,
};

const TITLE: &str = "Coroflow Notes";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Notes,
    Diagnostics,
}

impl Tab {
    const ALL: [Tab; 2] = [Tab::Notes, Tab::Diagnostics];

    fn label(self) -> &'static str {
        match self {
            Tab::Notes => "Notes",
            Tab::Diagnostics => "Diagnostics",
        }
    }
}

/// The desktop window this demo opens.
pub fn create_app() -> AppLauncher {
    AppLauncher::new().with_title(TITLE).with_size(760, 720)
}

/// The root composable: builds the object graph once, keeps one view model
/// for the whole window, and switches between the two screens.
#[composable]
pub fn CoroflowDemoApp() {
    let container = remember(|| {
        Rc::new(AppContainer::new(
            AppDispatchers::standard(),
            AppConfig::interactive(),
        ))
    })
    .with(Rc::clone);
    let graph = Rc::clone(&container);
    let view_model = Handle(rememberViewModel(move |scope| {
        NotesViewModel::new(scope, graph.notes_use_cases())
    }));
    let container = Handle(container);
    let tab = rememberMutableStateOf(|| Tab::Notes);

    Scaffold(
        Modifier::empty()
            .fill_max_size()
            .background(PALETTE.background),
        || TopBar(),
        move || TabBar(tab),
        move |padding: PaddingValues| {
            let (container, view_model) = (container.clone(), view_model.clone());
            Box(
                padding.apply_to(Modifier::empty().fill_max_size()),
                BoxSpec::default(),
                move || match tab.value() {
                    Tab::Notes => NotesScreen(view_model.clone()),
                    Tab::Diagnostics => DiagnosticsScreen(container.clone(), view_model.clone()),
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
fn TabBar(tab: MutableState<Tab>) {
    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(12.0)
            .background(PALETTE.surface),
        RowSpec::default().horizontal_arrangement(LinearArrangement::SpaceEvenly),
        move || {
            for candidate in Tab::ALL {
                let color = if tab.value() == candidate {
                    PALETTE.primary
                } else {
                    PALETTE.raised
                };
                ActionButton(candidate.label(), color, move || tab.set(candidate));
            }
        },
    );
}
