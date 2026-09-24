use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use coroflow::{SendFlow, delay, flow};

pub struct DropMarker(pub Arc<AtomicUsize>);

impl Drop for DropMarker {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

pub fn timed(steps: Vec<(u64, u32)>) -> impl SendFlow<Item = u32> + Clone {
    flow(move |emitter| {
        let steps = steps.clone();
        async move {
            for (wait, value) in steps {
                delay(Duration::from_millis(wait)).await;
                emitter.emit(value).await;
            }
        }
    })
}

pub fn endless(dropped: &Arc<AtomicUsize>) -> impl SendFlow<Item = u32> + Clone {
    let marker = Arc::clone(dropped);
    flow(move |emitter| {
        let marker = Arc::clone(&marker);
        async move {
            let _upstream = DropMarker(marker);
            for value in 1..=u32::MAX {
                delay(Duration::from_millis(10)).await;
                emitter.emit(value).await;
            }
        }
    })
}
