use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use coroflow::{MutableStateFlow, StateFlow};

/// One task.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Task {
    /// Never reused, even after the task is removed.
    pub id: u64,
    /// What the user typed.
    pub title: String,
    /// Whether it is ticked off.
    pub done: bool,
}

/// The tasks, kept for as long as the app runs: a repository. Screens read
/// it through view models and never hold the list themselves.
#[derive(Clone)]
pub struct TasksRepository {
    tasks: MutableStateFlow<Vec<Task>>,
    next_id: Arc<AtomicU64>,
}

impl Default for TasksRepository {
    fn default() -> Self {
        let starter = starter_tasks();
        Self {
            next_id: Arc::new(AtomicU64::new(starter.len() as u64)),
            tasks: MutableStateFlow::new(starter),
        }
    }
}

impl TasksRepository {
    /// The tasks, in the order they were added.
    pub fn tasks(&self) -> StateFlow<Vec<Task>> {
        self.tasks.as_state_flow()
    }

    /// Adds a task titled `title`, trimmed, and returns whether it did: a
    /// blank title adds nothing.
    pub fn add(&self, title: &str) -> bool {
        let title = title.trim();
        if title.is_empty() {
            return false;
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.tasks.update(|tasks| {
            let mut next = tasks.clone();
            next.push(Task {
                id,
                title: title.to_owned(),
                done: false,
            });
            next
        });
        true
    }

    /// Ticks task `id` off, or back on.
    pub fn toggle_done(&self, id: u64) {
        self.tasks.update(|tasks| {
            tasks
                .iter()
                .map(|task| Task {
                    done: task.done != (task.id == id),
                    ..task.clone()
                })
                .collect()
        });
    }

    /// Removes task `id`.
    pub fn remove(&self, id: u64) {
        self.tasks
            .update(|tasks| tasks.iter().filter(|task| task.id != id).cloned().collect());
    }
}

fn starter_tasks() -> Vec<Task> {
    [
        ("Copy this template", true),
        ("Rename the package in Cargo.toml", false),
        ("Replace these tasks with your own screens", false),
    ]
    .into_iter()
    .zip(0..)
    .map(|((title, done), id)| Task {
        id,
        title: title.to_owned(),
        done,
    })
    .collect()
}
