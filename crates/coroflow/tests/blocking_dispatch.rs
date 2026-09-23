use std::{
    sync::{
        Arc,
        mpsc::{self, Receiver, RecvTimeoutError, Sender, SyncSender},
    },
    thread::{self, ThreadId},
    time::{Duration, Instant},
};

use coroflow::{
    ConfinedDispatcher, Dispatch, Flow, FlowExt, MainScope, MutableSharedFlow, MutableStateFlow,
    Runnable, Stream, SystemClock, flow_of,
};

const VALUES: u32 = 300;
const PATIENCE: Duration = Duration::from_secs(10);

struct UiLikeExecutor {
    main: ThreadId,
    local: Sender<Runnable>,
    handoff: SyncSender<Runnable>,
}

impl Dispatch for UiLikeExecutor {
    fn dispatch(&self, runnable: Runnable) {
        if thread::current().id() == self.main {
            let _ = self.local.send(runnable);
        } else {
            let _ = self.handoff.send(runnable);
        }
    }
}

struct MainQueue {
    local: Receiver<Runnable>,
    handoff: Receiver<Runnable>,
}

impl MainQueue {
    fn next(&self) -> Option<Runnable> {
        self.local
            .try_recv()
            .ok()
            .or_else(|| self.handoff.recv_timeout(Duration::from_micros(200)).ok())
    }
}

fn blocking_main_dispatcher() -> (ConfinedDispatcher, MainQueue) {
    let (local, local_runnables) = mpsc::channel();
    let (handoff, handed_off) = mpsc::sync_channel(0);
    let executor = UiLikeExecutor {
        main: thread::current().id(),
        local,
        handoff,
    };
    let dispatcher = ConfinedDispatcher::for_current_thread(executor, SystemClock::shared());
    (
        dispatcher,
        MainQueue {
            local: local_runnables,
            handoff: handed_off,
        },
    )
}

fn finishes_within_patience(scenario: impl FnOnce() + Send + 'static) -> bool {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        scenario();
        let _ = done.send(());
    });
    !matches!(
        finished.recv_timeout(PATIENCE),
        Err(RecvTimeoutError::Timeout)
    )
}

fn run_main_loop(runnables: &MainQueue, mut touch: impl FnMut() -> bool) {
    let deadline = Instant::now() + PATIENCE;
    while Instant::now() < deadline {
        if touch() {
            return;
        }
        if let Some(runnable) = runnables.next() {
            runnable.run();
        }
    }
}

#[test]
fn a_background_state_write_never_blocks_a_main_thread_that_reads_the_same_flow() {
    let finished = finishes_within_patience(|| {
        let (dispatcher, runnables) = blocking_main_dispatcher();
        let scope = MainScope::new(dispatcher);
        let state = MutableStateFlow::new(0_u32);
        let collected = std::rc::Rc::new(std::cell::Cell::new(0));
        let sink = std::rc::Rc::clone(&collected);
        let observed = state.as_state_flow();
        scope.launch(async move {
            observed.collect(|value| sink.set(value)).await;
        });
        let writer = state.clone();
        thread::spawn(move || {
            for value in 1..=VALUES {
                writer.set(value);
            }
        });
        run_main_loop(&runnables, || {
            let _ = state.value();
            collected.get() == VALUES
        });
    });
    assert!(finished, "the main thread and the writer deadlocked");
}

#[test]
fn a_background_event_never_blocks_a_main_thread_that_reads_the_same_flow() {
    let finished = finishes_within_patience(|| {
        let (dispatcher, runnables) = blocking_main_dispatcher();
        let scope = MainScope::new(dispatcher);
        let events = MutableSharedFlow::new(0, VALUES as usize);
        let last = std::rc::Rc::new(std::cell::Cell::new(0));
        let sink = std::rc::Rc::clone(&last);
        let observed = events.as_shared_flow();
        scope.launch(async move {
            observed.collect(|value| sink.set(value)).await;
        });
        let emitter = events.clone();
        let started = Arc::new(std::sync::Barrier::new(2));
        let go = Arc::clone(&started);
        thread::spawn(move || {
            go.wait();
            for value in 1..=VALUES {
                emitter.emit(value);
            }
        });
        let mut released = false;
        run_main_loop(&runnables, || {
            if !released && events.subscription_count() == 1 {
                started.wait();
                released = true;
            }
            let _ = events.subscription_count();
            last.get() == VALUES
        });
    });
    assert!(finished, "the main thread and the emitter deadlocked");
}

#[test]
fn a_flow_on_upstream_never_blocks_a_main_thread_that_drains_it() {
    let finished = finishes_within_patience(|| {
        let (dispatcher, runnables) = blocking_main_dispatcher();
        let scope = MainScope::new(dispatcher);
        let total = std::rc::Rc::new(std::cell::Cell::new(0));
        let sink = std::rc::Rc::clone(&total);
        let numbers = flow_of((1..=VALUES).collect::<Vec<_>>())
            .flow_on(coroflow::Dispatchers::default_pool());
        scope.launch(async move {
            let mut run = numbers.open();
            while let Some(value) =
                std::future::poll_fn(|cx| std::pin::Pin::new(&mut run).poll_next(cx)).await
            {
                sink.set(value);
            }
        });
        run_main_loop(&runnables, || total.get() == VALUES);
    });
    assert!(
        finished,
        "the main thread and the flow_on worker deadlocked"
    );
}
