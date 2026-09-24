use std::{
    future::poll_fn,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::Stream;

use crate::{
    channel::{Capacity, Receiver, Sender, channel},
    clock::Timer,
    dispatcher::{Dispatcher, Dispatchers, current_dispatcher},
    flow::Flow,
    job::Job,
    task::spawn_send,
};

pub(crate) fn poll_run<S: Stream + Unpin>(
    run: &mut S,
    cx: &mut Context<'_>,
) -> Poll<Option<S::Item>> {
    Pin::new(run).poll_next(cx)
}

/// The flow returned by [`map`](crate::FlowExt::map).
#[derive(Clone)]
pub struct Map<F, T> {
    upstream: F,
    transform: T,
}

impl<F, T> Map<F, T> {
    pub(crate) fn new(upstream: F, transform: T) -> Self {
        Self {
            upstream,
            transform,
        }
    }
}

/// One run of a [`Map`].
pub struct MapRun<S, T> {
    upstream: S,
    transform: T,
}

impl<S, T> Unpin for MapRun<S, T> {}

impl<F, T, U> Flow for Map<F, T>
where
    F: Flow,
    T: Fn(F::Item) -> U + Clone,
{
    type Item = U;
    type Run = MapRun<F::Run, T>;

    fn open(&self) -> Self::Run {
        MapRun {
            upstream: self.upstream.open(),
            transform: self.transform.clone(),
        }
    }
}

impl<S, T, U> Stream for MapRun<S, T>
where
    S: Stream + Unpin,
    T: Fn(S::Item) -> U,
{
    type Item = U;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
        let this = self.get_mut();
        poll_run(&mut this.upstream, cx).map(|value| value.map(&this.transform))
    }
}

/// The flow returned by [`filter`](crate::FlowExt::filter).
#[derive(Clone)]
pub struct Filter<F, P> {
    upstream: F,
    predicate: P,
}

impl<F, P> Filter<F, P> {
    pub(crate) fn new(upstream: F, predicate: P) -> Self {
        Self {
            upstream,
            predicate,
        }
    }
}

/// One run of a [`Filter`].
pub struct FilterRun<S, P> {
    upstream: S,
    predicate: P,
}

impl<S, P> Unpin for FilterRun<S, P> {}

impl<F, P> Flow for Filter<F, P>
where
    F: Flow,
    P: Fn(&F::Item) -> bool + Clone,
{
    type Item = F::Item;
    type Run = FilterRun<F::Run, P>;

    fn open(&self) -> Self::Run {
        FilterRun {
            upstream: self.upstream.open(),
            predicate: self.predicate.clone(),
        }
    }
}

impl<S, P> Stream for FilterRun<S, P>
where
    S: Stream + Unpin,
    P: Fn(&S::Item) -> bool,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        loop {
            match poll_run(&mut this.upstream, cx) {
                Poll::Ready(Some(value)) if !(this.predicate)(&value) => continue,
                other => return other,
            }
        }
    }
}

/// The flow returned by [`on_each`](crate::FlowExt::on_each).
#[derive(Clone)]
pub struct OnEach<F, A> {
    upstream: F,
    action: A,
}

impl<F, A> OnEach<F, A> {
    pub(crate) fn new(upstream: F, action: A) -> Self {
        Self { upstream, action }
    }
}

/// One run of an [`OnEach`].
pub struct OnEachRun<S, A> {
    upstream: S,
    action: A,
}

impl<S, A> Unpin for OnEachRun<S, A> {}

impl<F, A> Flow for OnEach<F, A>
where
    F: Flow,
    A: Fn(&F::Item) + Clone,
{
    type Item = F::Item;
    type Run = OnEachRun<F::Run, A>;

    fn open(&self) -> Self::Run {
        OnEachRun {
            upstream: self.upstream.open(),
            action: self.action.clone(),
        }
    }
}

impl<S, A> Stream for OnEachRun<S, A>
where
    S: Stream + Unpin,
    A: Fn(&S::Item),
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        let polled = poll_run(&mut this.upstream, cx);
        if let Poll::Ready(Some(value)) = &polled {
            (this.action)(value);
        }
        polled
    }
}

/// The flow returned by [`on_start`](crate::FlowExt::on_start).
#[derive(Clone)]
pub struct OnStart<F, A> {
    upstream: F,
    action: A,
}

impl<F, A> OnStart<F, A> {
    pub(crate) fn new(upstream: F, action: A) -> Self {
        Self { upstream, action }
    }
}

impl<F, A> Flow for OnStart<F, A>
where
    F: Flow,
    A: Fn(),
{
    type Item = F::Item;
    type Run = F::Run;

    fn open(&self) -> Self::Run {
        (self.action)();
        self.upstream.open()
    }
}

/// The flow returned by [`on_completion`](crate::FlowExt::on_completion).
#[derive(Clone)]
pub struct OnCompletion<F, A> {
    upstream: F,
    action: A,
}

impl<F, A> OnCompletion<F, A> {
    pub(crate) fn new(upstream: F, action: A) -> Self {
        Self { upstream, action }
    }
}

/// One run of an [`OnCompletion`]; its action runs when the run ends or is
/// dropped.
pub struct OnCompletionRun<S, A: Fn()> {
    upstream: S,
    action: Option<A>,
}

impl<S, A: Fn()> Unpin for OnCompletionRun<S, A> {}

impl<F, A> Flow for OnCompletion<F, A>
where
    F: Flow,
    A: Fn() + Clone,
{
    type Item = F::Item;
    type Run = OnCompletionRun<F::Run, A>;

    fn open(&self) -> Self::Run {
        OnCompletionRun {
            upstream: self.upstream.open(),
            action: Some(self.action.clone()),
        }
    }
}

impl<S: Stream + Unpin, A: Fn()> Stream for OnCompletionRun<S, A> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        let polled = poll_run(&mut this.upstream, cx);
        if let Poll::Ready(None) = polled
            && let Some(action) = this.action.take()
        {
            action();
        }
        polled
    }
}

impl<S, A: Fn()> Drop for OnCompletionRun<S, A> {
    fn drop(&mut self) {
        if let Some(action) = self.action.take() {
            action();
        }
    }
}

/// The flow returned by [`start_with`](crate::FlowExt::start_with).
#[derive(Clone)]
pub struct StartWith<F: Flow> {
    upstream: F,
    first: F::Item,
}

impl<F: Flow> StartWith<F> {
    pub(crate) fn new(upstream: F, first: F::Item) -> Self {
        Self { upstream, first }
    }
}

/// One run of a [`StartWith`].
pub struct StartWithRun<S: Stream> {
    first: Option<S::Item>,
    upstream: S,
}

impl<S: Stream> Unpin for StartWithRun<S> {}

impl<F> Flow for StartWith<F>
where
    F: Flow,
    F::Item: Clone,
{
    type Item = F::Item;
    type Run = StartWithRun<F::Run>;

    fn open(&self) -> Self::Run {
        StartWithRun {
            first: Some(self.first.clone()),
            upstream: self.upstream.open(),
        }
    }
}

impl<S: Stream + Unpin> Stream for StartWithRun<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        match this.first.take() {
            Some(first) => Poll::Ready(Some(first)),
            None => poll_run(&mut this.upstream, cx),
        }
    }
}

/// The flow returned by [`take`](crate::FlowExt::take).
#[derive(Clone)]
pub struct Take<F> {
    upstream: F,
    count: usize,
}

impl<F> Take<F> {
    pub(crate) fn new(upstream: F, count: usize) -> Self {
        Self { upstream, count }
    }
}

/// One run of a [`Take`].
pub struct TakeRun<S> {
    upstream: Option<S>,
    remaining: usize,
}

impl<F: Flow> Flow for Take<F> {
    type Item = F::Item;
    type Run = TakeRun<F::Run>;

    fn open(&self) -> Self::Run {
        TakeRun {
            upstream: (self.count > 0).then(|| self.upstream.open()),
            remaining: self.count,
        }
    }
}

impl<S: Stream + Unpin> Stream for TakeRun<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        let Some(upstream) = this.upstream.as_mut() else {
            return Poll::Ready(None);
        };
        let polled = poll_run(upstream, cx);
        match &polled {
            Poll::Ready(Some(_)) => {
                this.remaining -= 1;
                if this.remaining == 0 {
                    this.upstream = None;
                }
            }
            Poll::Ready(None) => this.upstream = None,
            Poll::Pending => {}
        }
        polled
    }
}

/// How [`DistinctUntilChanged`] decides that a value changed: by the whole
/// value ([`WholeValue`]) or by a key closure `Fn(&T) -> K`.
pub trait ChangeKey<T> {
    /// What is compared between consecutive values.
    type Key: PartialEq;

    /// Remembers the key of `value` in `last` and reports whether it differs
    /// from the key remembered before.
    fn changed(&self, last: &mut Option<Self::Key>, value: &T) -> bool;
}

/// Compares whole values — plain
/// [`distinct_until_changed`](crate::FlowExt::distinct_until_changed).
#[derive(Clone, Copy)]
pub struct WholeValue;

impl<T: PartialEq + Clone> ChangeKey<T> for WholeValue {
    type Key = T;

    fn changed(&self, last: &mut Option<T>, value: &T) -> bool {
        match last {
            Some(previous) if previous == value => false,
            Some(previous) => {
                previous.clone_from(value);
                true
            }
            None => {
                *last = Some(value.clone());
                true
            }
        }
    }
}

impl<T, K: PartialEq, F: Fn(&T) -> K> ChangeKey<T> for F {
    type Key = K;

    fn changed(&self, last: &mut Option<K>, value: &T) -> bool {
        let key = self(value);
        if last.as_ref() == Some(&key) {
            return false;
        }
        *last = Some(key);
        true
    }
}

/// The flow returned by
/// [`distinct_until_changed`](crate::FlowExt::distinct_until_changed) and
/// [`distinct_until_changed_by`](crate::FlowExt::distinct_until_changed_by).
#[derive(Clone)]
pub struct DistinctUntilChanged<F, K> {
    upstream: F,
    key: K,
}

impl<F, K> DistinctUntilChanged<F, K> {
    pub(crate) fn new(upstream: F, key: K) -> Self {
        Self { upstream, key }
    }
}

/// One run of a [`DistinctUntilChanged`].
pub struct DistinctRun<S: Stream, K: ChangeKey<S::Item>> {
    upstream: S,
    key: K,
    last: Option<K::Key>,
}

impl<S: Stream, K: ChangeKey<S::Item>> Unpin for DistinctRun<S, K> {}

impl<F, K> Flow for DistinctUntilChanged<F, K>
where
    F: Flow,
    K: ChangeKey<F::Item> + Clone,
{
    type Item = F::Item;
    type Run = DistinctRun<F::Run, K>;

    fn open(&self) -> Self::Run {
        DistinctRun {
            upstream: self.upstream.open(),
            key: self.key.clone(),
            last: None,
        }
    }
}

impl<S, K> Stream for DistinctRun<S, K>
where
    S: Stream + Unpin,
    K: ChangeKey<S::Item>,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        loop {
            match poll_run(&mut this.upstream, cx) {
                Poll::Ready(Some(value)) if !this.key.changed(&mut this.last, &value) => {}
                other => return other,
            }
        }
    }
}

/// The flow returned by [`debounce`](crate::FlowExt::debounce).
#[derive(Clone)]
pub struct Debounce<F> {
    upstream: F,
    timeout: Duration,
}

impl<F> Debounce<F> {
    pub(crate) fn new(upstream: F, timeout: Duration) -> Self {
        Self { upstream, timeout }
    }
}

/// One run of a [`Debounce`].
pub struct DebounceRun<S: Stream> {
    upstream: Option<S>,
    pending: Option<S::Item>,
    timeout: Duration,
    timer: Timer,
}

impl<S: Stream> Unpin for DebounceRun<S> {}

impl<F: Flow> Flow for Debounce<F> {
    type Item = F::Item;
    type Run = DebounceRun<F::Run>;

    fn open(&self) -> Self::Run {
        DebounceRun {
            upstream: Some(self.upstream.open()),
            pending: None,
            timeout: self.timeout,
            timer: Timer::default(),
        }
    }
}

impl<S: Stream + Unpin> Stream for DebounceRun<S> {
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        while let Some(upstream) = this.upstream.as_mut() {
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) => {
                    this.pending = Some(value);
                    this.timer.start(this.timeout);
                }
                Poll::Ready(None) => {
                    this.upstream = None;
                    this.timer.cancel();
                }
                Poll::Pending => break,
            }
        }
        if this.upstream.is_none() {
            return Poll::Ready(this.pending.take());
        }
        if this.pending.is_none() {
            return Poll::Pending;
        }
        this.timer.poll_elapsed(cx).map(|()| this.pending.take())
    }
}

/// The flow returned by [`combine`](crate::FlowExt::combine).
#[derive(Clone)]
pub struct Combine<A, B, F> {
    first: A,
    second: B,
    combiner: F,
}

impl<A, B, F> Combine<A, B, F> {
    pub(crate) fn new(first: A, second: B, combiner: F) -> Self {
        Self {
            first,
            second,
            combiner,
        }
    }
}

/// One run of a [`Combine`].
pub struct CombineRun<A: Stream, B: Stream, F> {
    first: Latest<A>,
    second: Latest<B>,
    combiner: F,
}

impl<A: Stream, B: Stream, F> Unpin for CombineRun<A, B, F> {}

impl<A, B, F, U> Flow for Combine<A, B, F>
where
    A: Flow,
    B: Flow,
    F: Fn(&A::Item, &B::Item) -> U + Clone,
{
    type Item = U;
    type Run = CombineRun<A::Run, B::Run, F>;

    fn open(&self) -> Self::Run {
        CombineRun {
            first: Latest::new(self.first.open()),
            second: Latest::new(self.second.open()),
            combiner: self.combiner.clone(),
        }
    }
}

pub(crate) struct Latest<S: Stream> {
    run: Option<S>,
    value: Option<S::Item>,
}

impl<S: Stream + Unpin> Latest<S> {
    pub(crate) fn new(run: S) -> Self {
        Self {
            run: Some(run),
            value: None,
        }
    }

    pub(crate) fn drain(&mut self, cx: &mut Context<'_>) -> bool {
        let mut changed = false;
        while let Some(run) = self.run.as_mut() {
            match poll_run(run, cx) {
                Poll::Ready(Some(value)) => {
                    self.value = Some(value);
                    changed = true;
                }
                Poll::Ready(None) => self.run = None,
                Poll::Pending => break,
            }
        }
        changed
    }

    pub(crate) fn value(&self) -> Option<&S::Item> {
        self.value.as_ref()
    }

    pub(crate) fn status(&self) -> (bool, bool) {
        let done = self.run.is_none();
        (done && self.value.is_none(), done)
    }
}

pub(crate) fn latest_ended<T>(statuses: &[(bool, bool)]) -> Poll<Option<T>> {
    let starved = statuses.iter().any(|(starved, _)| *starved);
    let done = statuses.iter().all(|(_, done)| *done);
    if starved || done {
        Poll::Ready(None)
    } else {
        Poll::Pending
    }
}

impl<A, B, F, U> Stream for CombineRun<A, B, F>
where
    A: Stream + Unpin,
    B: Stream + Unpin,
    F: Fn(&A::Item, &B::Item) -> U,
{
    type Item = U;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<U>> {
        let this = self.get_mut();
        let changed = this.first.drain(cx) | this.second.drain(cx);
        if changed && let (Some(first), Some(second)) = (this.first.value(), this.second.value()) {
            return Poll::Ready(Some((this.combiner)(first, second)));
        }
        latest_ended(&[this.first.status(), this.second.status()])
    }
}

/// How many values [`flow_on`](crate::FlowExt::flow_on) buffers between the
/// upstream's dispatcher and the collector — Kotlin's default channel size.
pub const FLOW_ON_BUFFER: usize = 64;

/// The flow returned by [`flow_on`](crate::FlowExt::flow_on),
/// [`buffer`](crate::FlowExt::buffer) and [`conflate`](crate::FlowExt::conflate):
/// the upstream runs in its own coroutine and hands values over through a
/// channel.
#[derive(Clone)]
pub struct Buffered<F> {
    upstream: F,
    dispatcher: Option<Dispatcher>,
    capacity: Capacity,
}

impl<F> Buffered<F> {
    pub(crate) fn new(upstream: F, dispatcher: Option<Dispatcher>, capacity: Capacity) -> Self {
        Self {
            upstream,
            dispatcher,
            capacity,
        }
    }
}

/// One run of a [`Buffered`]; the upstream starts on the first poll and is
/// cancelled when the run is dropped.
pub struct BufferedRun<S: Stream> {
    pending: Option<(S, Sender<S::Item>)>,
    dispatcher: Option<Dispatcher>,
    receiver: Receiver<S::Item>,
    job: Option<Job>,
}

impl<S: Stream> Unpin for BufferedRun<S> {}

impl<F> Flow for Buffered<F>
where
    F: Flow,
    F::Run: Send + 'static,
    F::Item: Send + 'static,
{
    type Item = F::Item;
    type Run = BufferedRun<F::Run>;

    fn open(&self) -> Self::Run {
        let (sender, receiver) = channel(self.capacity);
        BufferedRun {
            pending: Some((self.upstream.open(), sender)),
            dispatcher: self.dispatcher.clone(),
            receiver,
            job: None,
        }
    }
}

impl<S> Stream for BufferedRun<S>
where
    S: Stream + Unpin + Send + 'static,
    S::Item: Send + 'static,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        if let Some((mut upstream, sender)) = this.pending.take() {
            let dispatcher = this
                .dispatcher
                .take()
                .or_else(current_dispatcher)
                .unwrap_or_else(Dispatchers::default_pool);
            this.job = Some(spawn_send(&dispatcher, async move {
                while let Some(value) = poll_fn(|cx| poll_run(&mut upstream, cx)).await {
                    if sender.send(value).await.is_err() {
                        return;
                    }
                }
            }));
        }
        Pin::new(&mut this.receiver).poll_next(cx)
    }
}

impl<S: Stream> Drop for BufferedRun<S> {
    fn drop(&mut self) {
        if let Some(job) = &self.job {
            job.cancel();
        }
    }
}
