use std::time::Duration;

use coroflow::{FlowExt, MainScope, SharingStarted, StateFlow};

use crate::data::tasks::{Task, TasksRepository};

/// How long the screen state keeps following the repository after the
/// screen stops showing it, so a quick tab switch restarts nothing.
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Everything the Tasks screen draws.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TasksUiState {
    /// The tasks, in the order they were added.
    pub tasks: Vec<Task>,
    /// How many are not done yet.
    pub remaining: usize,
}

impl TasksUiState {
    fn of(tasks: &[Task]) -> Self {
        Self {
            tasks: tasks.to_vec(),
            remaining: tasks.iter().filter(|task| !task.done).count(),
        }
    }
}

/// The Tasks screen's view model. The `NavHost` keeps one per visit of the
/// screen in the screen's view model store.
pub struct TasksViewModel {
    repository: TasksRepository,
    state: StateFlow<TasksUiState>,
    _scope: MainScope,
}

impl TasksViewModel {
    /// A view model over `repository` whose coroutines run in `scope`.
    pub fn new(scope: MainScope, repository: TasksRepository) -> Self {
        let initial = TasksUiState::of(&repository.tasks().value());
        let state = repository
            .tasks()
            .map(|tasks| TasksUiState::of(&tasks))
            .state_in(
                &scope,
                SharingStarted::while_subscribed(STOP_TIMEOUT),
                initial,
            );
        Self {
            repository,
            state,
            _scope: scope,
        }
    }

    /// The screen state.
    pub fn state(&self) -> StateFlow<TasksUiState> {
        self.state.clone()
    }

    /// The user asked to add `title`; returns whether it was added, so the
    /// screen knows to clear its input.
    pub fn on_add(&self, title: &str) -> bool {
        self.repository.add(title)
    }

    /// The user ticked task `id`.
    pub fn on_toggle_done(&self, id: u64) {
        self.repository.toggle_done(id);
    }

    /// The user removed task `id`.
    pub fn on_remove(&self, id: u64) {
        self.repository.remove(id);
    }
}
