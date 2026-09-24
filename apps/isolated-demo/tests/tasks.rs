use std::time::Duration;

use coroflow::{MainScope, TestScheduler};
use isolated_demo::{
    data::tasks::TasksRepository,
    presentation::tasks_view_model::{TasksUiState, TasksViewModel},
};

fn titles(repository: &TasksRepository) -> Vec<String> {
    repository
        .tasks()
        .value()
        .into_iter()
        .map(|task| task.title)
        .collect()
}

#[test]
fn a_fresh_repository_holds_the_three_starter_tasks() {
    assert_eq!(
        titles(&TasksRepository::default()),
        [
            "Copy this template",
            "Rename the package in Cargo.toml",
            "Replace these tasks with your own screens",
        ]
    );
}

#[test]
fn adding_trims_the_title_and_never_reuses_an_id() {
    let repository = TasksRepository::default();
    let removed = repository.tasks().value()[0].id;
    repository.remove(removed);
    assert!(repository.add("  Write the README  "));
    let tasks = repository.tasks().value();
    let added = tasks.last().expect("just added a task");
    assert_eq!(added.title, "Write the README");
    assert!(!added.done);
    assert!(
        tasks.iter().all(|task| task.id != removed),
        "a removed id is never handed out again"
    );
    assert!(!repository.add("   "), "a blank title adds nothing");
    assert_eq!(repository.tasks().value().len(), 3);
}

#[test]
fn toggling_and_removing_touch_only_their_task() {
    let repository = TasksRepository::default();
    let tasks = repository.tasks().value();
    let (target, untouched) = (tasks[1].id, tasks[2].id);
    repository.toggle_done(target);
    let tasks = repository.tasks().value();
    assert!(tasks.iter().any(|task| task.id == target && task.done));
    assert!(tasks.iter().any(|task| task.id == untouched && !task.done));
    repository.remove(target);
    assert!(repository
        .tasks()
        .value()
        .iter()
        .all(|task| task.id != target));
}

#[test]
fn the_view_model_counts_what_is_left_and_follows_the_repository() {
    let scheduler = TestScheduler::new();
    let repository = TasksRepository::default();
    let tasks = TasksViewModel::new(
        MainScope::new(scheduler.main_dispatcher()),
        repository.clone(),
    );
    let mut screen = scheduler.turbine(&tasks.state());
    let first: TasksUiState = screen.await_item();
    assert_eq!((first.tasks.len(), first.remaining), (3, 2));

    assert!(tasks.on_add("Ship it"));
    assert_eq!(screen.await_item().remaining, 3);
    let shipped = repository.tasks().value()[3].id;
    tasks.on_toggle_done(shipped);
    assert_eq!(screen.await_item().remaining, 2);
    tasks.on_remove(shipped);
    let last = screen.await_item();
    assert_eq!((last.tasks.len(), last.remaining), (3, 2));
    scheduler.advance_time_by(Duration::from_secs(1));
    screen.expect_no_events();
}
