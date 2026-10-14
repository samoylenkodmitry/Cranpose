//! A process-wide queue between platform hosts and compositions.
//!
//! A host publishes from whatever thread the platform called it on, and
//! every registered observer gets the item. An item published while no one
//! observes waits in a backlog, which the next observer to register
//! receives, so what arrives during launch reaches the first screen that
//! listens.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, PoisonError},
};

type Observer<T> = Arc<dyn Fn(T) + Send + Sync>;

struct State<T> {
    observers: Vec<(u64, Observer<T>)>,
    backlog: VecDeque<T>,
    next_id: u64,
}

/// One kind of item that platform hosts hand to the application.
pub(crate) struct Inbox<T> {
    /// What the log calls the items.
    name: &'static str,
    state: Mutex<State<T>>,
}

impl<T: Clone + Send + 'static> Inbox<T> {
    pub(crate) const fn new(name: &'static str) -> Self {
        Self {
            name,
            state: Mutex::new(State {
                observers: Vec::new(),
                backlog: VecDeque::new(),
                next_id: 0,
            }),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Registers `observer` and hands it the backlog.
    pub(crate) fn observe(
        &'static self,
        observer: impl Fn(T) + Send + Sync + 'static,
    ) -> InboxObserver {
        let observer: Observer<T> = Arc::new(observer);
        let (id, replay) = {
            let mut state = self.state();
            let id = state.next_id;
            state.next_id += 1;
            state.observers.push((id, Arc::clone(&observer)));
            (id, std::mem::take(&mut state.backlog))
        };
        log::info!(
            "{}: observer {id} registered, replays {} item(s)",
            self.name,
            replay.len()
        );
        for item in replay {
            observer(item);
        }
        InboxObserver { id, inbox: self }
    }

    /// Hands `item` to every observer, or keeps it for the next one.
    pub(crate) fn publish(&self, item: T) {
        let observers: Vec<Observer<T>> = {
            let mut state = self.state();
            if state.observers.is_empty() {
                state.backlog.push_back(item);
                log::info!("{}: no observer yet, the item waits", self.name);
                return;
            }
            state
                .observers
                .iter()
                .map(|(_, observer)| Arc::clone(observer))
                .collect()
        };
        log::info!(
            "{}: an item goes to {} observer(s)",
            self.name,
            observers.len()
        );
        for observer in observers {
            observer(item.clone());
        }
    }

    /// Drops the backlog.
    pub(crate) fn clear(&self) {
        self.state().backlog.clear();
    }
}

trait Unobserve: Sync {
    fn unobserve(&self, id: u64);
}

impl<T: Send> Unobserve for Inbox<T> {
    fn unobserve(&self, id: u64) {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .observers
            .retain(|(existing, _)| *existing != id);
    }
}

/// Keeps an observer of a platform inbox, such as incoming content or opened
/// links, registered until it is dropped.
pub struct InboxObserver {
    id: u64,
    inbox: &'static dyn Unobserve,
}

impl Drop for InboxObserver {
    fn drop(&mut self) {
        self.inbox.unobserve(self.id);
    }
}
