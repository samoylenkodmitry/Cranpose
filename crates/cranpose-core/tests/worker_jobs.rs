// `withBlocking` runs a worker job off the UI thread and hands its output to
// the UI thread, and a cancelled effect drops the output.

use std::{
    cell::RefCell,
    rc::Rc,
    time::{Duration, Instant},
};

use cranpose_core::{
    Composition, Key, LaunchedEffect, MemoryApplier, launchBlocking, location_key, withBlocking,
};
use cranpose_macros::worker_job;

type Seen = Rc<RefCell<Vec<String>>>;

// The thread the job ran on, after the words of its input in reverse order.
#[worker_job]
fn reversed_words(text: String) -> String {
    let words: Vec<&str> = text.split_whitespace().rev().collect();
    format!("{} on {:?}", words.join(" "), std::thread::current().id())
}

fn render(composition: &mut Composition<MemoryApplier>, root: Key, seen: &Seen, shown: bool) {
    let seen = Rc::clone(seen);
    composition
        .render(root, move || {
            if shown {
                let seen = Rc::clone(&seen);
                LaunchedEffect((), async move |scope| {
                    scope.launch(async move {
                        let words = "one two three".to_string();
                        if let Ok(out) = withBlocking(reversed_words.with(words)).await {
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

#[test]
fn a_worker_job_runs_off_the_ui_thread_and_answers_on_it() {
    let mut composition = Composition::new(MemoryApplier::new());
    let root = location_key(file!(), line!(), column!());
    let seen = Seen::default();
    render(&mut composition, root, &seen, true);
    drain(&composition, Duration::from_secs(5), || {
        !seen.borrow().is_empty()
    });

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
    render(&mut composition, root, &seen, true);
    render(&mut composition, root, &seen, false);
    drain(&composition, Duration::from_millis(300), || false);

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
    drain(&composition, Duration::from_secs(5), || {
        !seen.borrow().is_empty()
    });

    assert!(
        seen.borrow()[0].starts_with("b a on "),
        "{:?}",
        seen.borrow()
    );
}
