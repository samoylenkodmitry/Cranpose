use std::{
    future::poll_fn,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use futures_core::Stream;

use crate::{
    clock::Timer,
    dispatcher::Dispatcher,
    flow::Flow,
    job::Job,
    sync::{PipeReceiver, pipe},
    task::spawn_send,
};

fn poll_run<S: Stream + Unpin>(run: &mut S, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
    Pin::new(run).poll_next(cx)
}

/// The flow returned by [`map`](crate::FlowExt::map).
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

/// The flow returned by
/// [`distinct_until_changed`](crate::FlowExt::distinct_until_changed).
pub struct DistinctUntilChanged<F> {
    upstream: F,
}

impl<F> DistinctUntilChanged<F> {
    pub(crate) fn new(upstream: F) -> Self {
        Self { upstream }
    }
}

/// One run of a [`DistinctUntilChanged`].
pub struct DistinctRun<S: Stream> {
    upstream: S,
    last: Option<S::Item>,
}

impl<S: Stream> Unpin for DistinctRun<S> {}

impl<F> Flow for DistinctUntilChanged<F>
where
    F: Flow,
    F::Item: PartialEq + Clone,
{
    type Item = F::Item;
    type Run = DistinctRun<F::Run>;

    fn open(&self) -> Self::Run {
        DistinctRun {
            upstream: self.upstream.open(),
            last: None,
        }
    }
}

impl<S> Stream for DistinctRun<S>
where
    S: Stream + Unpin,
    S::Item: PartialEq + Clone,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<S::Item>> {
        let this = self.get_mut();
        loop {
            match poll_run(&mut this.upstream, cx) {
                Poll::Ready(Some(value)) if this.last.as_ref() == Some(&value) => continue,
                Poll::Ready(Some(value)) => {
                    match &mut this.last {
                        Some(last) => last.clone_from(&value),
                        None => this.last = Some(value.clone()),
                    }
                    return Poll::Ready(Some(value));
                }
                other => return other,
            }
        }
    }
}

/// The flow returned by [`debounce`](crate::FlowExt::debounce).
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

/// The flow returned by [`flat_map_latest`](crate::FlowExt::flat_map_latest).
pub struct FlatMapLatest<F, T> {
    upstream: F,
    transform: T,
}

impl<F, T> FlatMapLatest<F, T> {
    pub(crate) fn new(upstream: F, transform: T) -> Self {
        Self {
            upstream,
            transform,
        }
    }
}

/// One run of a [`FlatMapLatest`].
pub struct FlatMapLatestRun<S, T, G: Flow> {
    upstream: Option<S>,
    transform: T,
    inner: Option<G::Run>,
}

impl<S, T, G: Flow> Unpin for FlatMapLatestRun<S, T, G> {}

impl<F, T, G> Flow for FlatMapLatest<F, T>
where
    F: Flow,
    G: Flow,
    T: Fn(F::Item) -> G + Clone,
{
    type Item = G::Item;
    type Run = FlatMapLatestRun<F::Run, T, G>;

    fn open(&self) -> Self::Run {
        FlatMapLatestRun {
            upstream: Some(self.upstream.open()),
            transform: self.transform.clone(),
            inner: None,
        }
    }
}

impl<S, T, G> Stream for FlatMapLatestRun<S, T, G>
where
    S: Stream + Unpin,
    G: Flow,
    T: Fn(S::Item) -> G,
{
    type Item = G::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<G::Item>> {
        let this = self.get_mut();
        while let Some(upstream) = this.upstream.as_mut() {
            match poll_run(upstream, cx) {
                Poll::Ready(Some(value)) => {
                    this.inner = None;
                    this.inner = Some((this.transform)(value).open());
                }
                Poll::Ready(None) => this.upstream = None,
                Poll::Pending => break,
            }
        }
        if let Some(inner) = this.inner.as_mut() {
            match poll_run(inner, cx) {
                Poll::Ready(None) => this.inner = None,
                other => return other,
            }
        }
        if this.upstream.is_none() {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

/// The flow returned by [`combine`](crate::FlowExt::combine).
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
    first: Option<A>,
    second: Option<B>,
    latest_first: Option<A::Item>,
    latest_second: Option<B::Item>,
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
            first: Some(self.first.open()),
            second: Some(self.second.open()),
            latest_first: None,
            latest_second: None,
            combiner: self.combiner.clone(),
        }
    }
}

fn drain_latest<S: Stream + Unpin>(
    run: &mut Option<S>,
    latest: &mut Option<S::Item>,
    cx: &mut Context<'_>,
) -> bool {
    let mut changed = false;
    while let Some(active) = run.as_mut() {
        match poll_run(active, cx) {
            Poll::Ready(Some(value)) => {
                *latest = Some(value);
                changed = true;
            }
            Poll::Ready(None) => *run = None,
            Poll::Pending => break,
        }
    }
    changed
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
        let first_changed = drain_latest(&mut this.first, &mut this.latest_first, cx);
        let second_changed = drain_latest(&mut this.second, &mut this.latest_second, cx);
        if (first_changed || second_changed)
            && let (Some(first), Some(second)) = (&this.latest_first, &this.latest_second)
        {
            return Poll::Ready(Some((this.combiner)(first, second)));
        }
        let first_done = this.first.is_none();
        let second_done = this.second.is_none();
        let starved = (first_done && this.latest_first.is_none())
            || (second_done && this.latest_second.is_none());
        if starved || (first_done && second_done) {
            Poll::Ready(None)
        } else {
            Poll::Pending
        }
    }
}

/// How many values [`flow_on`](crate::FlowExt::flow_on) buffers between the
/// upstream's dispatcher and the collector — Kotlin's default channel size.
pub const FLOW_ON_BUFFER: usize = 64;

/// The flow returned by [`flow_on`](crate::FlowExt::flow_on).
pub struct FlowOn<F> {
    upstream: F,
    dispatcher: Dispatcher,
}

impl<F> FlowOn<F> {
    pub(crate) fn new(upstream: F, dispatcher: Dispatcher) -> Self {
        Self {
            upstream,
            dispatcher,
        }
    }
}

/// One run of a [`FlowOn`]; dropping it cancels the upstream.
pub struct FlowOnRun<T> {
    receiver: PipeReceiver<T>,
    job: Job,
}

impl<F> Flow for FlowOn<F>
where
    F: Flow,
    F::Run: Send + 'static,
    F::Item: Send + 'static,
{
    type Item = F::Item;
    type Run = FlowOnRun<F::Item>;

    fn open(&self) -> Self::Run {
        let (sender, receiver) = pipe(FLOW_ON_BUFFER);
        let mut upstream = self.upstream.open();
        let job = spawn_send(&self.dispatcher, async move {
            while let Some(value) = poll_fn(|cx| poll_run(&mut upstream, cx)).await {
                let mut slot = Some(value);
                if !poll_fn(|cx| sender.poll_send(&mut slot, cx)).await {
                    return;
                }
            }
        });
        FlowOnRun { receiver, job }
    }
}

impl<T> Stream for FlowOnRun<T> {
    type Item = T;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<T>> {
        self.receiver.poll_recv(cx)
    }
}

impl<T> Drop for FlowOnRun<T> {
    fn drop(&mut self) {
        self.job.cancel();
    }
}
