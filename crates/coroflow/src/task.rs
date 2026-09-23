use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    panic::{AssertUnwindSafe, catch_unwind},
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, ThreadId},
};

use crate::{
    dispatcher::{ConfinedDispatcher, Dispatcher, Runnable, Schedule, enter},
    job::{Job, JobOutcome},
    sync::lock,
};

type LocalFuture = Pin<Box<dyn Future<Output = ()>>>;

enum Step {
    Pending,
    Finished(JobOutcome),
}

fn poll_step<F: Future<Output = ()> + ?Sized>(
    future: Pin<&mut F>,
    dispatcher: &Dispatcher,
    waker: &Waker,
) -> Step {
    let _entered = enter(dispatcher);
    let mut cx = Context::from_waker(waker);
    match catch_unwind(AssertUnwindSafe(|| future.poll(&mut cx))) {
        Ok(Poll::Pending) => Step::Pending,
        Ok(Poll::Ready(())) => Step::Finished(JobOutcome::Completed),
        Err(_) => {
            log::error!("coroflow: a coroutine panicked");
            Step::Finished(JobOutcome::Panicked)
        }
    }
}

struct SendTask<F> {
    future: Mutex<Option<Pin<Box<F>>>>,
    scheduled: AtomicBool,
    dispatcher: Dispatcher,
    job: Job,
}

pub(crate) fn spawn_send<F>(dispatcher: &Dispatcher, future: F) -> Job
where
    F: Future<Output = ()> + Send + 'static,
{
    let job = Job::new();
    let task = Arc::new(SendTask {
        future: Mutex::new(Some(Box::pin(future))),
        scheduled: AtomicBool::new(false),
        dispatcher: dispatcher.clone(),
        job: job.clone(),
    });
    job.set_task_waker(Waker::from(Arc::clone(&task)));
    task.schedule();
    job
}

impl<F: Future<Output = ()> + Send + 'static> SendTask<F> {
    fn schedule(self: &Arc<Self>) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            self.dispatcher
                .dispatch(Runnable::new(Arc::clone(self) as Arc<dyn Schedule>));
        }
    }
}

impl<F: Future<Output = ()> + Send + 'static> Wake for SendTask<F> {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}

impl<F: Future<Output = ()> + Send + 'static> Schedule for SendTask<F> {
    fn run(self: Arc<Self>) {
        self.scheduled.store(false, Ordering::Release);
        let mut slot = lock(&self.future);
        let Some(future) = slot.as_mut() else {
            return;
        };
        if self.job.cancel_requested() {
            *slot = None;
            drop(slot);
            self.job.finish(JobOutcome::Cancelled);
            return;
        }
        let waker = Waker::from(Arc::clone(&self));
        if let Step::Finished(outcome) = poll_step(future.as_mut(), &self.dispatcher, &waker) {
            *slot = None;
            drop(slot);
            self.job.finish(outcome);
        }
    }
}

thread_local! {
    static LOCAL_TASKS: RefCell<HashMap<u64, LocalFuture>> = RefCell::new(HashMap::new());
}

static NEXT_LOCAL_TASK: AtomicU64 = AtomicU64::new(0);

struct LocalHandle {
    id: u64,
    owner: ThreadId,
    scheduled: AtomicBool,
    dispatcher: Dispatcher,
    job: Job,
}

pub(crate) fn spawn_local<F>(dispatcher: &ConfinedDispatcher, future: F) -> Job
where
    F: Future<Output = ()> + 'static,
{
    let job = Job::new();
    let id = NEXT_LOCAL_TASK.fetch_add(1, Ordering::Relaxed);
    LOCAL_TASKS.with(|tasks| {
        tasks
            .borrow_mut()
            .insert(id, Box::pin(future) as LocalFuture)
    });
    let handle = Arc::new(LocalHandle {
        id,
        owner: dispatcher.thread(),
        scheduled: AtomicBool::new(false),
        dispatcher: dispatcher.dispatcher().clone(),
        job: job.clone(),
    });
    job.set_task_waker(Waker::from(Arc::clone(&handle)));
    handle.schedule();
    job
}

impl LocalHandle {
    fn schedule(self: &Arc<Self>) {
        if !self.scheduled.swap(true, Ordering::AcqRel) {
            self.dispatcher
                .dispatch(Runnable::new(Arc::clone(self) as Arc<dyn Schedule>));
        }
    }
}

impl Wake for LocalHandle {
    fn wake(self: Arc<Self>) {
        self.schedule();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.schedule();
    }
}

impl Schedule for LocalHandle {
    fn run(self: Arc<Self>) {
        self.scheduled.store(false, Ordering::Release);
        if thread::current().id() != self.owner {
            log::error!("coroflow: a confined dispatcher ran a main-thread task on another thread");
            return;
        }
        let Some(mut future) = LOCAL_TASKS.with(|tasks| tasks.borrow_mut().remove(&self.id)) else {
            return;
        };
        if self.job.cancel_requested() {
            drop(future);
            self.job.finish(JobOutcome::Cancelled);
            return;
        }
        let waker = Waker::from(Arc::clone(&self));
        match poll_step(future.as_mut(), &self.dispatcher, &waker) {
            Step::Pending => {
                LOCAL_TASKS.with(|tasks| tasks.borrow_mut().insert(self.id, future));
            }
            Step::Finished(outcome) => {
                drop(future);
                self.job.finish(outcome);
            }
        }
    }
}
