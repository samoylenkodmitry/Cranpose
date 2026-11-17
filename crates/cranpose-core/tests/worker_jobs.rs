// `withBlocking` runs a worker job off the UI thread and hands its output to
// the UI thread, and a cancelled effect drops the output.

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use cranpose_core::{
    BlockingWork, Composition, Key, LaunchedEffect, MemoryApplier, launchBlocking, location_key,
    withBlocking,
};
use cranpose_macros::worker_job;

type Seen = Rc<RefCell<Vec<String>>>;

// The longest a wait for another thread lasts.
const LIMIT: Duration = Duration::from_secs(5);

// How long the UI thread drains to show that nothing arrives.
const QUIET: Duration = Duration::from_millis(300);

// The thread the job ran on, after the words of its input in reverse order.
#[worker_job]
fn reversed_words(text: String) -> String {
    let words: Vec<&str> = text.split_whitespace().rev().collect();
    format!("{} on {:?}", words.join(" "), std::thread::current().id())
}

// A job's function captures nothing. The job and the test pass their signals
// through a static.
struct Hold {
    entered: AtomicBool,
    released: Mutex<bool>,
    release_made: Condvar,
    returned: AtomicBool,
}

impl Hold {
    const fn new() -> Self {
        Self {
            entered: AtomicBool::new(false),
            released: Mutex::new(false),
            release_made: Condvar::new(),
            returned: AtomicBool::new(false),
        }
    }

    // The job's body: it stays until the test releases it. A test that fails
    // before it releases the job must not keep a pool thread, so the wait ends
    // after `LIMIT`.
    fn pass(&self, text: String) -> String {
        self.entered.store(true, Ordering::SeqCst);
        drop(self.release_made.wait_timeout_while(
            self.released.lock().unwrap_or_else(PoisonError::into_inner),
            LIMIT,
            |released| !*released,
        ));
        self.returned.store(true, Ordering::SeqCst);
        text
    }

    fn has_entered(&self) -> bool {
        self.entered.load(Ordering::SeqCst)
    }

    fn release(&self) {
        *self.released.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.release_made.notify_all();
    }

    fn has_returned(&self) -> bool {
        self.returned.load(Ordering::SeqCst)
    }
}

// The job is still running when its effect is cancelled.
static RUNNING: Hold = Hold::new();

#[worker_job]
fn running_words(text: String) -> String {
    RUNNING.pass(text)
}

// The job has answered when its effect is cancelled.
static ANSWERED: Hold = Hold::new();

#[worker_job]
fn answered_words(text: String) -> String {
    ANSWERED.pass(text)
}

// Renders an effect that runs `job` on the words "one two three" and notes its
// output in `seen`, or no effect.
fn render<W: BlockingWork<String>>(
    composition: &mut Composition<MemoryApplier>,
    root: Key,
    seen: &Seen,
    shown: bool,
    job: fn(String) -> W,
) {
    let seen = Rc::clone(seen);
    composition
        .render(root, move || {
            if shown {
                let seen = Rc::clone(&seen);
                LaunchedEffect((), async move |scope| {
                    scope.launch(async move {
                        let words = "one two three".to_string();
                        if let Ok(out) = withBlocking(job(words)).await {
                            seen.borrow_mut().push(out);
                        }
                    });
                });
            }
        })
        .expect("render succeeds");
    composition.runtime_handle().drain_ui();
}

// Drains the runtime until `done` holds, for up to `limit`.
fn drain(composition: &Composition<MemoryApplier>, limit: Duration, done: impl Fn() -> bool) {
    let runtime = composition.runtime_handle();
    let until = Instant::now() + limit;
    while !done() && Instant::now() < until {
        runtime.drain_ui();
        std::thread::yield_now();
    }
}

// Waits for `done` to hold, for up to `LIMIT`, and leaves the runtime's queues
// as they are.
fn wait_until(done: impl Fn() -> bool) -> bool {
    let until = Instant::now() + LIMIT;
    while !done() && Instant::now() < until {
        std::thread::yield_now();
    }
    done()
}

#[test]
fn a_worker_job_runs_off_the_ui_thread_and_answers_on_it() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let seen = Seen::default();
    render(&mut composition, root, &seen, true, |words| {
        reversed_words.with(words)
    });
    drain(&composition, LIMIT, || !seen.borrow().is_empty());

    let seen = seen.borrow();
    let [answer] = seen.as_slice() else {
        panic!("one answer was expected: {seen:?}");
    };
    let here = format!("{:?}", std::thread::current().id());
    assert!(
        answer.starts_with("three two one on ") && !answer.ends_with(&here),
        "the job did not run on another thread: {answer}, the UI thread is {here}"
    );
}

#[test]
fn a_cancelled_effect_drops_the_job_s_output() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let seen = Seen::default();
    let job = |words| running_words.with(words);
    render(&mut composition, root, &seen, true, job);
    drain(&composition, LIMIT, || RUNNING.has_entered());
    assert!(RUNNING.has_entered(), "the job did not start");
    render(&mut composition, root, &seen, false, job);
    RUNNING.release();
    assert!(
        wait_until(|| RUNNING.has_returned()),
        "the job did not finish"
    );
    drain(&composition, QUIET, || false);

    assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
}

// The pass that cancels an effect drains the UI thread before it composes, so
// an output that arrived earlier reaches the effect while it is still live.
// This one arrives while the pass composes.
#[test]
fn a_cancelled_effect_drops_an_output_that_arrives_in_the_pass_that_cancels_it() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let seen = Seen::default();
    render(&mut composition, root, &seen, true, |words| {
        answered_words.with(words)
    });
    drain(&composition, LIMIT, || ANSWERED.has_entered());
    assert!(ANSWERED.has_entered(), "the job did not start");
    let runtime = composition.runtime_handle();
    assert!(!runtime.has_pending_ui(), "nothing waits for the UI thread");
    composition
        .render(root, || {
            ANSWERED.release();
            assert!(
                wait_until(|| runtime.has_pending_ui()),
                "the output did not reach the runtime"
            );
        })
        .expect("render succeeds");
    drain(&composition, QUIET, || false);

    assert!(seen.borrow().is_empty(), "{:?}", seen.borrow());
}

#[test]
fn launch_blocking_takes_a_worker_job_too() {
    let mut composition = Composition::new(MemoryApplier::new());
    let seen = Seen::default();
    let answered = Rc::clone(&seen);
    composition
        .render(location_key(file!(), line!(), column!()), move || {
            let answered = Rc::clone(&answered);
            launchBlocking(reversed_words.with("a b".to_string()), move |out| {
                answered.borrow_mut().push(out.expect("the job ran"));
            })
            .expect("a runtime is live");
        })
        .expect("render succeeds");
    drain(&composition, LIMIT, || !seen.borrow().is_empty());

    assert!(
        seen.borrow()[0].starts_with("b a on "),
        "{:?}",
        seen.borrow()
    );
}
