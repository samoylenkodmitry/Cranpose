#![allow(non_snake_case)]

use cranpose::prelude::*;
use cranpose_core::mutableStateListOf;
use cranpose_foundation::text::TextFieldState;

use crate::theme::{body_text_style, heading_text_style, Palette};

#[derive(Clone, PartialEq)]
struct Task {
    id: u64,
    title: String,
    done: bool,
}

#[derive(Clone)]
pub(crate) struct TasksState {
    items: SnapshotStateList<Task>,
    next_id: MutableState<u64>,
}

impl PartialEq for TasksState {
    fn eq(&self, other: &Self) -> bool {
        self.next_id == other.next_id
    }
}

impl TasksState {
    fn snapshot(&self) -> Vec<Task> {
        self.items.to_vec()
    }

    fn add(&self, title: &str) {
        let title = title.trim();
        if title.is_empty() {
            return;
        }
        let id = self.next_id.value();
        self.next_id.set(id + 1);
        self.items.push(Task {
            id,
            title: title.to_string(),
            done: false,
        });
    }

    fn remove(&self, id: u64) {
        self.items.retain(|task| task.id != id);
    }

    fn toggle_done(&self, id: u64) {
        let items = self.items.to_vec();
        if let Some(index) = items.iter().position(|task| task.id == id) {
            let mut task = items[index].clone();
            task.done = !task.done;
            self.items.set(index, task);
        }
    }
}

fn starter_tasks() -> Vec<Task> {
    vec![
        Task {
            id: 0,
            title: "Copy this template".to_string(),
            done: true,
        },
        Task {
            id: 1,
            title: "Rename the package in Cargo.toml".to_string(),
            done: false,
        },
        Task {
            id: 2,
            title: "Replace these tasks with your own screens".to_string(),
            done: false,
        },
    ]
}

pub(crate) fn rememberTasksState() -> TasksState {
    let items = remember(|| mutableStateListOf(starter_tasks())).with(|list| list.clone());
    let next_id = rememberMutableStateOf(|| starter_tasks().len() as u64);
    TasksState { items, next_id }
}

#[composable]
pub(crate) fn TasksScreen(palette: Palette, tasks: TasksState) {
    let input = remember(|| TextFieldState::new("")).with(|state| *state);
    let items = tasks.snapshot();

    Column(
        Modifier::empty().fill_max_size().padding(24.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(16.0)),
        move || {
            Text("Tasks", Modifier::empty(), heading_text_style(palette.text));

            NewTaskField(palette, input, tasks.clone());

            if items.is_empty() {
                Text(
                    "No tasks yet. Add one above.",
                    Modifier::empty(),
                    body_text_style(palette.muted_text),
                );
            } else {
                let list_state = rememberLazyListState();
                let items = items.clone();
                let tasks = tasks.clone();
                LazyColumn(
                    Modifier::empty().fill_max_size(),
                    list_state,
                    LazyColumnSpec::default(),
                    move |scope| {
                        let row_items = items.clone();
                        let tasks = tasks.clone();
                        scope.items(
                            LazyItems::new(items.len()).key(move |index| items[index].id),
                            move |index| {
                                TaskRow(palette, row_items[index].clone(), tasks.clone());
                            },
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn NewTaskField(palette: Palette, input: TextFieldState, tasks: TasksState) {
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

            let tasks_for_add = tasks.clone();
            Button(
                Modifier::empty()
                    .padding(12.0)
                    .background(palette.primary)
                    .rounded_corners(8.0),
                ButtonSpec::default(),
                move || {
                    tasks_for_add.add(&input.text());
                    input.set_text("");
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
fn TaskRow(palette: Palette, task: Task, tasks: TasksState) {
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
            let tasks_for_toggle = tasks.clone();
            Button(
                Modifier::empty().padding(8.0),
                ButtonSpec::default(),
                move || tasks_for_toggle.toggle_done(id),
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

            let tasks_for_remove = tasks.clone();
            Button(
                Modifier::empty().padding(8.0),
                ButtonSpec::default(),
                move || tasks_for_remove.remove(id),
                move || {
                    Text("Remove", Modifier::empty(), body_text_style(palette.danger));
                },
            );
        },
    );
}

#[cfg(test)]
mod tests {
    use cranpose_ui::run_test_composition;

    use super::rememberTasksState;

    #[test]
    fn a_fresh_tasks_state_holds_the_three_starter_tasks() {
        run_test_composition(|| {
            let tasks = rememberTasksState();
            let titles: Vec<String> = tasks
                .snapshot()
                .into_iter()
                .map(|task| task.title)
                .collect();
            assert_eq!(
                titles,
                vec![
                    "Copy this template",
                    "Rename the package in Cargo.toml",
                    "Replace these tasks with your own screens",
                ]
            );
        });
    }

    #[test]
    fn adding_a_task_appends_it_with_a_fresh_id() {
        run_test_composition(|| {
            let tasks = rememberTasksState();
            let starter_count = tasks.snapshot().len();

            tasks.add("Write the README");

            let snapshot = tasks.snapshot();
            assert_eq!(snapshot.len(), starter_count + 1);
            let added = snapshot.last().expect("just added a task");
            assert_eq!(added.title, "Write the README");
            assert!(!added.done);
            assert!(
                snapshot[..starter_count]
                    .iter()
                    .all(|task| task.id != added.id),
                "a fresh task must not reuse an existing id"
            );
        });
    }

    #[test]
    fn adding_a_blank_or_whitespace_title_does_nothing() {
        run_test_composition(|| {
            let tasks = rememberTasksState();
            let starter_count = tasks.snapshot().len();

            tasks.add("   ");
            tasks.add("");

            assert_eq!(tasks.snapshot().len(), starter_count);
        });
    }

    #[test]
    fn toggling_done_flips_only_the_targeted_task() {
        run_test_composition(|| {
            let tasks = rememberTasksState();
            let target_id = tasks.snapshot()[1].id;
            let untouched_id = tasks.snapshot()[2].id;

            tasks.toggle_done(target_id);

            let snapshot = tasks.snapshot();
            let target = snapshot.iter().find(|task| task.id == target_id).unwrap();
            let untouched = snapshot
                .iter()
                .find(|task| task.id == untouched_id)
                .unwrap();
            assert!(target.done, "toggling should have marked the task done");
            assert!(!untouched.done, "toggling one task must not affect another");
        });
    }

    #[test]
    fn removing_a_task_drops_it_by_id_regardless_of_position() {
        run_test_composition(|| {
            let tasks = rememberTasksState();
            let removed_id = tasks.snapshot()[0].id;
            let starter_count = tasks.snapshot().len();

            tasks.remove(removed_id);

            let snapshot = tasks.snapshot();
            assert_eq!(snapshot.len(), starter_count - 1);
            assert!(snapshot.iter().all(|task| task.id != removed_id));
        });
    }

    #[test]
    fn removed_ids_are_never_reused() {
        run_test_composition(|| {
            let tasks = rememberTasksState();
            let removed_id = tasks.snapshot()[0].id;
            tasks.remove(removed_id);

            tasks.add("Replacement task");

            let snapshot = tasks.snapshot();
            assert!(
                snapshot.iter().filter(|task| task.id == removed_id).count() <= 1,
                "a removed id must not be handed to a new task"
            );
        });
    }
}
