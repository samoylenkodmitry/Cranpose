use cranpose::prelude::*;
use cranpose_coroflow::{viewModel, Handle, StateFlowCollect};
use cranpose_foundation::text::TextFieldState;

use crate::{
    data::tasks::{Task, TasksRepository},
    presentation::tasks_view_model::TasksViewModel,
    theme::{body_text_style, heading_text_style, Palette},
};

/// The task list. Its view model lives in this screen's view model store, so
/// it outlasts a switch to another tab and back.
#[composable]
pub(crate) fn TasksScreen(palette: Palette, repository: Handle<TasksRepository>) {
    let tasks = viewModel((), move |scope| {
        TasksViewModel::new(scope, repository.get().as_ref().clone())
    });
    let state = tasks.get().state().collectAsStateWithLifecycle().get();
    let input = remember(|| TextFieldState::new("")).with(|state| *state);
    let items = state.tasks;
    let remaining = state.remaining;

    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(16.0)),
        move || {
            Text("Tasks", Modifier::empty(), heading_text_style(palette.text));
            Text(
                format!("{remaining} left to do"),
                Modifier::empty(),
                body_text_style(palette.muted_text),
            );

            NewTaskField(palette, input, tasks);

            if items.is_empty() {
                Text(
                    "No tasks yet. Add one above.",
                    Modifier::empty(),
                    body_text_style(palette.muted_text),
                );
            } else {
                let list_state = rememberLazyListState();
                let items = items.clone();
                LazyColumn(
                    Modifier::empty().fill_max_size(),
                    list_state,
                    LazyColumnSpec::default(),
                    move |scope| {
                        let row_items = items.clone();
                        scope.items(
                            LazyItems::new(items.len()).key(move |index| items[index].id),
                            move |index| {
                                TaskRow(palette, row_items[index].clone(), tasks);
                            },
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn NewTaskField(palette: Palette, input: TextFieldState, tasks: Handle<TasksViewModel>) {
    Row(
        Modifier::empty().fill_max_width(),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::spaced_by(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Box(
                Modifier::empty()
                    .weight(1.0)
                    .padding(10.0)
                    .background(palette.surface)
                    .rounded_corners(8.0),
                BoxSpec::default(),
                move || {
                    if input.text().is_empty() {
                        Text(
                            "Add a task...",
                            Modifier::empty(),
                            body_text_style(palette.muted_text),
                        );
                    }
                    BasicTextField(
                        input,
                        Modifier::empty().fill_max_width(),
                        body_text_style(palette.text),
                    );
                },
            );

            Button(
                Modifier::empty()
                    .padding(12.0)
                    .background(palette.primary)
                    .rounded_corners(8.0),
                ButtonSpec::default(),
                move || {
                    if tasks.get().on_add(&input.text()) {
                        input.set_text("");
                    }
                },
                move || {
                    Text(
                        "Add",
                        Modifier::empty(),
                        body_text_style(palette.on_primary),
                    );
                },
            );
        },
    );
}

#[composable]
fn TaskRow(palette: Palette, task: Task, tasks: Handle<TasksViewModel>) {
    let id = task.id;
    let done = task.done;
    let title = task.title;
    let title_color = if done {
        palette.muted_text
    } else {
        palette.text
    };

    Row(
        Modifier::empty()
            .fill_max_width()
            .padding(10.0)
            .background(palette.surface)
            .rounded_corners(8.0),
        RowSpec::default()
            .horizontal_arrangement(LinearArrangement::SpaceBetween)
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            Button(
                Modifier::empty().padding(8.0),
                ButtonSpec::default(),
                move || tasks.get().on_toggle_done(id),
                move || {
                    Text(
                        if done { "[x]" } else { "[ ]" },
                        Modifier::empty(),
                        body_text_style(palette.primary),
                    );
                },
            );

            Text(
                title.clone(),
                Modifier::empty()
                    .weight(1.0)
                    .padding_each(8.0, 0.0, 8.0, 0.0),
                body_text_style(title_color),
            );

            Button(
                Modifier::empty().padding(8.0),
                ButtonSpec::default(),
                move || tasks.get().on_remove(id),
                move || {
                    Text("Remove", Modifier::empty(), body_text_style(palette.danger));
                },
            );
        },
    );
}
