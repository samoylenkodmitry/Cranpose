use std::{
    cell::{Cell, RefCell},
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

use coroflow::{Flow, Stream};
use cranpose_core::SnapshotStateObserver;

/// A flow of the value `block` computes from Cranpose state — Android's
/// `snapshotFlow { }`.
///
/// Every run evaluates `block` once, then again whenever state it read
/// changes, and emits the result when it differs from the previous one. It
/// reads snapshot state, so it runs on the main thread only: collect it from a
/// [`MainScope`](coroflow::MainScope).
///
/// ```compile_fail
/// use coroflow::{CoroutineScope, Dispatchers, FlowExt};
/// let scope = CoroutineScope::new(Dispatchers::default_pool());
/// let texts = cranpose_coroflow::snapshotFlow(|| String::new());
/// scope.launch(async move { texts.collect(|_| {}).await; });
/// ```
pub fn snapshotFlow<T, F>(block: F) -> SnapshotFlow<F>
where
    F: Fn() -> T + Clone + 'static,
    T: PartialEq + Clone + 'static,
{
    SnapshotFlow { block }
}

/// The flow returned by [`snapshotFlow`].
pub struct SnapshotFlow<F> {
    block: F,
}

/// One run of a [`SnapshotFlow`].
pub struct SnapshotRun<F, T> {
    block: F,
    observer: Option<SnapshotStateObserver>,
    changed: Rc<Cell<bool>>,
    waker: Rc<RefCell<Option<Waker>>>,
    last: Option<T>,
}

impl<T, F> Flow for SnapshotFlow<F>
where
    F: Fn() -> T + Clone + 'static,
    T: PartialEq + Clone + 'static,
{
    type Item = T;
    type Run = SnapshotRun<F, T>;

    fn open(&self) -> Self::Run {
        SnapshotRun {
            block: self.block.clone(),
            observer: None,
            changed: Rc::new(Cell::new(true)),
            waker: Rc::default(),
            last: None,
        }
    }
}

impl<F, T> Unpin for SnapshotRun<F, T> {}

impl<F, T> Stream for SnapshotRun<F, T>
where
    F: Fn() -> T,
    T: PartialEq + Clone + 'static,
{
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        let this = self.get_mut();
        {
            let mut waker = this.waker.borrow_mut();
            if !waker
                .as_ref()
                .is_some_and(|stored| stored.will_wake(cx.waker()))
            {
                *waker = Some(cx.waker().clone());
            }
        }
        while this.changed.replace(false) {
            let observer = this.observer.get_or_insert_with(|| {
                let observer = SnapshotStateObserver::new(|job| job());
                observer.start();
                observer
            });
            let changed = Rc::clone(&this.changed);
            let waker = Rc::clone(&this.waker);
            let value = observer.observe_reads(
                (),
                move |()| {
                    changed.set(true);
                    if let Some(waker) = waker.borrow().as_ref() {
                        waker.wake_by_ref();
                    }
                },
                &this.block,
            );
            if this.last.as_ref() != Some(&value) {
                this.last = Some(value.clone());
                return Poll::Ready(Some(value));
            }
        }
        Poll::Pending
    }
}

impl<F, T> Drop for SnapshotRun<F, T> {
    fn drop(&mut self) {
        if let Some(observer) = self.observer.take() {
            observer.stop();
        }
    }
}
