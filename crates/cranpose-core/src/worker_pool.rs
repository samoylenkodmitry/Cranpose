//! The Web Workers that run worker jobs in a browser. Each loads the page's
//! own wasm module again, with a memory of its own, and runs the job the page
//! names on the bytes the page sends.
//!
//! A worker starts with the first job that finds none idle, so a page that
//! launches no job starts none. An idle worker is closed after
//! [`IDLE_MILLIS`], which gives its memory back.

use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use wasm_bindgen::{JsCast, JsValue, closure::Closure, prelude::wasm_bindgen};
use web_sys::{Blob, BlobPropertyBag, MessageEvent, Url, Worker, WorkerOptions, WorkerType};

use crate::BlockingError;

const MOST_WORKERS: usize = 4;
const IDLE_MILLIS: i32 = 30_000;

#[wasm_bindgen]
extern "C" {
    // The address of the JavaScript module that loaded this wasm, which a
    // worker imports again.
    #[wasm_bindgen(thread_local_v2, js_namespace = ["import", "meta"], js_name = url)]
    static MODULE_URL: String;
}

// What a worker runs: it loads the module once, then answers each message
// with the job's output, or with the error that stopped it.
const WORKER_SOURCE: &str = r#"
let ready;
self.onmessage = async (event) => {
  const { id, glue, module, job, input } = event.data;
  try {
    ready ??= import(glue).then(async (app) => {
      await app.default({ module_or_path: module });
      return app;
    });
    const run = (await ready)["cranpose_worker_job_" + job];
    if (!run) throw new Error("no worker job named " + job);
    const output = run(input);
    self.postMessage({ id, output }, [output.buffer]);
  } catch (error) {
    self.postMessage({ id, error: String(error) });
  }
};
"#;

type Answer = Result<Vec<u8>, BlockingError>;
type Done = Box<dyn FnOnce(Answer)>;

struct Job {
    ticket: u64,
    name: &'static str,
    input: Vec<u8>,
    done: Done,
}

struct Slot {
    worker: Worker,
    // Kept for as long as the worker may call it.
    _on_message: Closure<dyn FnMut(MessageEvent)>,
    // What the job the worker runs hands its output to.
    running: Option<Done>,
    // Counts the times the worker went idle, so a closing timer knows whether
    // the worker worked again since the timer started.
    idle_turn: u32,
}

// A worker ends with its slot: when its job failed, when it sat idle, and when
// the thread's last runtime ended.
impl Drop for Slot {
    fn drop(&mut self) {
        self.worker.terminate();
    }
}

pub(crate) struct Pool {
    slots: Vec<Option<Slot>>,
    waiting: VecDeque<Job>,
    next_ticket: u64,
    // The address of the script every worker runs, made with the first.
    source_url: Option<String>,
}

// Runs `f` on the page's pool. No caller holds the pool while it calls out.
fn with_pool<R>(f: impl FnOnce(&mut Pool) -> R) -> R {
    crate::thread_state::runtimes(|state| f(&mut state.worker_pool.borrow_mut()))
}

// The answer of a job, for the future that waits for it.
#[derive(Default)]
struct Shared {
    answer: Option<Answer>,
    waker: Option<Waker>,
}

// A job on its way through the pool. Dropping it takes a job that still
// waits out of the line; a job a worker already runs goes on to its end.
pub(crate) struct Running {
    ticket: u64,
    shared: Rc<RefCell<Shared>>,
}

impl Future for Running {
    type Output = Answer;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Answer> {
        let mut shared = self.shared.borrow_mut();
        match shared.answer.take() {
            Some(answer) => Poll::Ready(answer),
            None => {
                shared.waker = Some(context.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let ticket = self.ticket;
        // The line's jobs drop outside the pool's borrow: a job's end may
        // drop another job's future.
        let left = crate::thread_state::try_runtimes(|state| {
            let mut pool = state.worker_pool.borrow_mut();
            let position = pool.waiting.iter().position(|job| job.ticket == ticket)?;
            pool.waiting.remove(position)
        });
        drop(left);
    }
}

// Runs job `name` on `input` in a worker.
pub(crate) fn run(name: &'static str, input: Vec<u8>) -> Running {
    let shared = Rc::new(RefCell::new(Shared::default()));
    let answered = Rc::clone(&shared);
    let ticket = with_pool(|pool| {
        pool.next_ticket += 1;
        pool.waiting.push_back(Job {
            ticket: pool.next_ticket,
            name,
            input,
            done: Box::new(move |answer| {
                let waker = {
                    let mut shared = answered.borrow_mut();
                    shared.answer = Some(answer);
                    shared.waker.take()
                };
                if let Some(waker) = waker {
                    waker.wake();
                }
            }),
        });
        pool.next_ticket
    });
    start_waiting();
    Running { ticket, shared }
}

// Hands waiting jobs to idle workers, and starts workers while jobs wait and
// the pool has room. Jobs that no worker can take, since none starts, end
// with `WorkerUnavailable`.
fn start_waiting() {
    loop {
        let refused = with_pool(|pool| {
            if pool.waiting.is_empty() {
                return None;
            }
            let Some(index) = pool.idle_slot().or_else(|| pool.new_slot()) else {
                // A running worker takes the line when it is done; with no
                // worker at all nothing ever would.
                let stranded = pool.slots.iter().all(Option::is_none);
                return stranded.then(|| std::mem::take(&mut pool.waiting));
            };
            let job = pool.waiting.pop_front()?;
            let slot = pool.slots[index].as_mut()?;
            match post(&slot.worker, index, &job) {
                Ok(()) => {
                    slot.running = Some(job.done);
                    Some(VecDeque::new())
                }
                Err(error) => {
                    log::error!("worker job {} did not start: {error:?}", job.name);
                    Some(VecDeque::from([job]))
                }
            }
        });
        let Some(refused) = refused else {
            break;
        };
        for job in refused {
            (job.done)(Err(BlockingError::WorkerUnavailable));
        }
    }
}

fn post(worker: &Worker, index: usize, job: &Job) -> Result<(), JsValue> {
    let message = js_sys::Object::new();
    let input = js_sys::Uint8Array::from(job.input.as_slice());
    let glue = MODULE_URL.with(String::clone);
    for (key, value) in [
        ("id", JsValue::from(index as u32)),
        ("glue", JsValue::from(glue)),
        ("module", wasm_bindgen::module()),
        ("job", JsValue::from(job.name)),
        ("input", input.clone().into()),
    ] {
        js_sys::Reflect::set(&message, &JsValue::from(key), &value)?;
    }
    worker.post_message_with_transfer(&message, &js_sys::Array::of1(&input.buffer()))
}

impl Pool {
    pub(crate) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            waiting: VecDeque::new(),
            next_ticket: 0,
            source_url: None,
        }
    }

    fn idle_slot(&self) -> Option<usize> {
        self.slots
            .iter()
            .position(|slot| slot.as_ref().is_some_and(|slot| slot.running.is_none()))
    }

    // A new worker in a free place of the pool, when the pool has room.
    fn new_slot(&mut self) -> Option<usize> {
        let index = match self.slots.iter().position(Option::is_none) {
            Some(index) => index,
            None if self.slots.len() < most_workers() => {
                self.slots.push(None);
                self.slots.len() - 1
            }
            None => return None,
        };
        match self.start_worker(index) {
            Ok(slot) => {
                self.slots[index] = Some(slot);
                Some(index)
            }
            Err(error) => {
                log::error!("a worker did not start: {error:?}");
                None
            }
        }
    }

    fn start_worker(&mut self, index: usize) -> Result<Slot, JsValue> {
        let source_url = match &self.source_url {
            Some(url) => url.clone(),
            None => {
                let options = BlobPropertyBag::new();
                options.set_type("text/javascript");
                let parts = js_sys::Array::of1(&JsValue::from(WORKER_SOURCE));
                let blob = Blob::new_with_str_sequence_and_options(&parts, &options)?;
                self.source_url
                    .insert(Url::create_object_url_with_blob(&blob)?)
                    .clone()
            }
        };
        let options = WorkerOptions::new();
        options.set_type(WorkerType::Module);
        let worker = Worker::new_with_options(&source_url, &options)?;
        let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
            finished(index, &event.data());
        });
        worker.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        Ok(Slot {
            worker,
            _on_message: on_message,
            running: None,
            idle_turn: 0,
        })
    }
}

fn most_workers() -> usize {
    let cores = web_sys::window().map_or(1.0, |window| window.navigator().hardware_concurrency());
    // The page keeps a core.
    if cores >= (MOST_WORKERS + 1) as f64 {
        MOST_WORKERS
    } else if cores >= 3.0 {
        2
    } else {
        1
    }
}

// The worker in slot `index` answered: hands the output on, or closes a
// worker whose job failed, since a wasm instance that trapped runs no more.
fn finished(index: usize, answer: &JsValue) {
    let read = |key: &str| js_sys::Reflect::get(answer, &JsValue::from(key)).ok();
    let output = read("output")
        .filter(|output| !output.is_undefined())
        .map(|output| js_sys::Uint8Array::new(&output).to_vec());
    let done = with_pool(|pool| {
        let slot = pool.slots.get_mut(index)?;
        let done = slot.as_mut()?.running.take();
        if output.is_none() {
            *slot = None;
        } else if let Some(slot) = slot {
            slot.idle_turn = slot.idle_turn.wrapping_add(1);
            close_when_idle(index, slot.idle_turn);
        }
        done
    });
    match (done, output) {
        (Some(done), Some(output)) => done(Ok(output)),
        (done, None) => {
            log::error!(
                "a worker job failed: {}",
                read("error")
                    .and_then(|error| error.as_string())
                    .unwrap_or_default()
            );
            if let Some(done) = done {
                done(Err(BlockingError::Panicked));
            }
        }
        (None, Some(_)) => {}
    }
    start_waiting();
}

// Closes the worker in slot `index` after `IDLE_MILLIS`, unless it worked
// again since idle turn `turn`.
fn close_when_idle(index: usize, turn: u32) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let close = Closure::once_into_js(move || {
        with_pool(|pool| {
            let Some(place) = pool.slots.get_mut(index) else {
                return;
            };
            if place
                .as_ref()
                .is_some_and(|slot| slot.running.is_none() && slot.idle_turn == turn)
            {
                *place = None;
            }
        });
    });
    if let Err(error) = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(close.unchecked_ref(), IDLE_MILLIS)
    {
        log::warn!("no timer closes an idle worker: {error:?}");
    }
}
