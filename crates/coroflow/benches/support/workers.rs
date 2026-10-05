use coroflow::{Dispatchers, with_context};

pub fn serial(jobs: u64) -> u64 {
    let dispatchers = [Dispatchers::default_pool(), Dispatchers::io()];
    pollster::block_on(async {
        let mut sum = 0;
        for value in 0..jobs {
            sum += with_context(&dispatchers[(value % 2) as usize], async move { value })
                .await
                .expect("serial coroutine completes");
        }
        sum
    })
}

pub async fn blocking(work: impl FnOnce() -> u64 + Send + 'static) -> u64 {
    cranpose_core::withBlocking(work)
        .await
        .expect("blocking work completes")
}
