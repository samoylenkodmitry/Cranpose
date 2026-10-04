use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex, PoisonError, Weak},
    task::{Context, Poll, Waker},
};

use event_listener::{Event, IntoNotification};

use super::{BlockingError, BlockingExecutorConfig};

pub(super) struct Executor {
    pool: Arc<Pool>,
}

impl Executor {
    pub(super) fn new(config: BlockingExecutorConfig) -> Self {
        Self {
            pool: Arc::new(Pool {
                config,
                state: Mutex::new(PoolState::default()),
                available: Condvar::new(),
                capacity: Event::new(),
            }),
        }
    }

    pub(super) fn try_submit<T, F>(&self, work: F) -> Result<Pending<T>, BlockingError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        self.pool.try_submit(&mut Some(work))
    }

    pub(super) async fn submit<T, F>(&self, work: F) -> Result<Pending<T>, BlockingError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let mut work = Some(work);
        loop {
            match self.pool.try_submit(&mut work) {
                Err(BlockingError::Saturated) => {}
                result => return result,
            }
            let listener = self.pool.capacity.listen();
            match self.pool.try_submit(&mut work) {
                Err(BlockingError::Saturated) => listener.await,
                result => return result,
            }
        }
    }

    pub(super) fn shutdown(&self) {
        let waiting = {
            let mut state = self
                .pool
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            state.closed = true;
            std::mem::take(&mut state.queue)
        };
        self.pool.available.notify_all();
        self.pool.notify_capacity(usize::MAX);
        for job in waiting {
            let _ = catch_unwind(AssertUnwindSafe(|| job.work.fail(BlockingError::Shutdown)));
        }
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Pool {
    config: BlockingExecutorConfig,
    state: Mutex<PoolState>,
    available: Condvar,
    capacity: Event,
}

#[derive(Default)]
struct PoolState {
    queue: VecDeque<QueuedJob>,
    workers: usize,
    running: usize,
    closed: bool,
}

struct QueuedJob {
    completion_key: usize,
    work: Box<dyn Job>,
}

trait Job: Send {
    fn run(self: Box<Self>, running: Running<'_>);
    fn fail(self: Box<Self>, error: BlockingError);
}

struct Work<T, F> {
    work: F,
    completion: Arc<Mutex<Completion<T>>>,
}

impl<T, F> Job for Work<T, F>
where
    T: Send,
    F: FnOnce() -> T + Send,
{
    fn run(self: Box<Self>, running: Running<'_>) {
        let Self { work, completion } = *self;
        let result = catch_unwind(AssertUnwindSafe(work)).map_err(|_| BlockingError::Panicked);
        drop(running);
        complete(&completion, result);
    }

    fn fail(self: Box<Self>, error: BlockingError) {
        complete(&self.completion, Err(error));
    }
}

impl Pool {
    fn try_submit<T, F>(self: &Arc<Self>, work: &mut Option<F>) -> Result<Pending<T>, BlockingError>
    where
        T: Send + 'static,
        F: FnOnce() -> T + Send + 'static,
    {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.closed {
            return Err(BlockingError::Shutdown);
        }
        if state.queue.len() >= self.config.max_queued.get() {
            return Err(BlockingError::Saturated);
        }
        if state.workers < self.config.max_threads.get()
            && state.workers <= state.running + state.queue.len()
        {
            let pool = Arc::clone(self);
            match std::thread::Builder::new()
                .name("cranpose-blocking".into())
                .spawn(move || pool.run())
            {
                Ok(_) => state.workers += 1,
                Err(_) if state.workers == 0 => return Err(BlockingError::WorkerUnavailable),
                Err(error) => log::warn!("cranpose: blocking worker could not start: {error}"),
            }
        }
        let completion = Arc::new(Mutex::new(Completion {
            result: None,
            waker: None,
        }));
        let completion_key = Arc::as_ptr(&completion) as usize;
        state.queue.push_back(QueuedJob {
            completion_key,
            work: Box::new(Work {
                work: work.take().expect("work submitted once"),
                completion: Arc::clone(&completion),
            }),
        });
        drop(state);
        self.available.notify_one();
        Ok(Pending {
            completion,
            queue: Some(QueueTicket {
                pool: Arc::downgrade(self),
                completion_key,
            }),
        })
    }

    fn next(&self) -> Option<QueuedJob> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(job) = state.queue.pop_front() {
                state.running += 1;
                drop(state);
                self.notify_capacity(1);
                return Some(job);
            }
            if state.closed {
                state.workers -= 1;
                return None;
            }
            let (new_state, timeout) = self
                .available
                .wait_timeout(state, self.config.idle_timeout)
                .unwrap_or_else(PoisonError::into_inner);
            state = new_state;
            if timeout.timed_out() && state.queue.is_empty() {
                state.workers -= 1;
                return None;
            }
        }
    }

    fn run(&self) {
        while let Some(job) = self.next() {
            let _ = catch_unwind(AssertUnwindSafe(|| job.work.run(Running(self))));
        }
    }

    fn notify_capacity(&self, count: usize) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            self.capacity.notify(count.additional())
        }));
    }

    fn cancel(&self, completion_key: usize) {
        let removed = {
            let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            state
                .queue
                .iter()
                .position(|job| job.completion_key == completion_key)
                .and_then(|index| state.queue.remove(index))
        };
        if removed.is_some() {
            self.notify_capacity(1);
        }
        drop(removed);
    }
}

struct Running<'a>(&'a Pool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .running -= 1;
    }
}

struct Completion<T> {
    result: Option<Result<T, BlockingError>>,
    waker: Option<Waker>,
}

fn complete<T>(completion: &Mutex<Completion<T>>, result: Result<T, BlockingError>) {
    let waker = {
        let mut completion = completion.lock().unwrap_or_else(PoisonError::into_inner);
        completion.result = Some(result);
        completion.waker.take()
    };
    if let Some(waker) = waker {
        waker.wake();
    }
}

struct QueueTicket {
    pool: Weak<Pool>,
    completion_key: usize,
}

pub(super) struct Pending<T> {
    completion: Arc<Mutex<Completion<T>>>,
    queue: Option<QueueTicket>,
}

impl<T> Pending<T> {
    pub(super) fn poll(&mut self, context: &mut Context<'_>) -> Poll<Result<T, BlockingError>> {
        let mut completion = self
            .completion
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(result) = completion.result.take() {
            self.queue = None;
            return Poll::Ready(result);
        }
        let old = if completion
            .waker
            .as_ref()
            .is_none_or(|waker| !waker.will_wake(context.waker()))
        {
            completion.waker.replace(context.waker().clone())
        } else {
            None
        };
        drop(completion);
        drop(old);
        Poll::Pending
    }
}

impl<T> Drop for Pending<T> {
    fn drop(&mut self) {
        let Some(ticket) = self.queue.take() else {
            return;
        };
        let waker = self
            .completion
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .waker
            .take();
        drop(waker);
        if let Some(pool) = ticket.pool.upgrade() {
            pool.cancel(ticket.completion_key);
        }
    }
}
