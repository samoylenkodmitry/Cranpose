use std::{
    future::ready,
    hint::black_box,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use coroflow::{
    CoroutineScope, Flow, FlowExt, MutableSharedFlow, MutableStateFlow, Stream, TestScheduler,
    flow_of,
};
use criterion::{Criterion, criterion_group, criterion_main};
use futures_util::{FutureExt, StreamExt, stream};

const CHAIN_VALUES: u64 = 10_000;

fn poll_now<S: Stream + Unpin>(run: &mut S) -> Poll<Option<S::Item>> {
    Pin::new(run).poll_next(&mut Context::from_waker(Waker::noop()))
}

fn launch(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("launch a coroutine and wait for it");
    let scheduler = TestScheduler::new();
    let scope = CoroutineScope::new(scheduler.dispatcher());
    group.bench_function("coroflow", |bench| {
        bench.iter(|| {
            let job = scope.launch(async {
                black_box(1);
            });
            black_box(scheduler.block_on(job.join()))
        });
    });
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread().build() else {
        return;
    };
    group.bench_function("tokio current_thread", |bench| {
        bench.iter(|| {
            black_box(runtime.block_on(async {
                tokio::spawn(async {
                    black_box(1);
                })
                .await
            }))
        });
    });
    group.finish();
}

fn state(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("set a state and read the change");
    let state = MutableStateFlow::new(0_u64);
    let mut run = state.open();
    let mut next = 0_u64;
    group.bench_function("coroflow StateFlow", |bench| {
        bench.iter(|| {
            next += 1;
            state.set(next);
            black_box(poll_now(&mut run))
        });
    });
    let (sender, mut receiver) = tokio::sync::watch::channel(0_u64);
    group.bench_function("tokio watch", |bench| {
        bench.iter(|| {
            next += 1;
            sender.send_replace(next);
            black_box(receiver.changed().now_or_never());
            black_box(*receiver.borrow_and_update())
        });
    });
    group.finish();
}

fn shared(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("broadcast an event to one collector");
    let events = MutableSharedFlow::new(0, 64);
    let mut run = events.open();
    group.bench_function("coroflow SharedFlow", |bench| {
        bench.iter(|| {
            events.try_emit(black_box(7_u64));
            black_box(poll_now(&mut run))
        });
    });
    let (sender, mut receiver) = tokio::sync::broadcast::channel(64);
    group.bench_function("tokio broadcast", |bench| {
        bench.iter(|| {
            black_box(sender.send(black_box(7_u64)).ok());
            black_box(receiver.try_recv().ok())
        });
    });
    group.finish();
}

fn chain(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("run 10k values through five operators");
    let values: Vec<u64> = (0..CHAIN_VALUES).collect();
    group.bench_function("coroflow", |bench| {
        bench.iter(|| {
            let sum = flow_of(values.clone())
                .map(|value| value * 3)
                .filter(|value| value % 2 == 0)
                .map(|value| value + 1)
                .filter_map(|value| (value % 3 != 0).then_some(value))
                .take(usize::MAX)
                .fold(0_u64, |sum, value| sum + value);
            black_box(pollster::block_on(sum))
        });
    });
    group.bench_function("futures stream", |bench| {
        bench.iter(|| {
            let sum = stream::iter(values.clone())
                .map(|value| value * 3)
                .filter(|value| ready(value % 2 == 0))
                .map(|value| value + 1)
                .filter_map(|value| ready((value % 3 != 0).then_some(value)))
                .take(usize::MAX)
                .fold(0_u64, |sum, value| ready(sum + value));
            black_box(pollster::block_on(sum))
        });
    });
    group.finish();
}

criterion_group!(comparisons, launch, state, shared, chain);
criterion_main!(comparisons);
