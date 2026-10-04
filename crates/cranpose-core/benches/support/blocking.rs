use cranpose_core::withBlocking;

#[derive(Debug)]
pub enum Failure {
    Saturated,
    Other,
}

pub async fn run(work: impl FnOnce() -> u64 + Send + 'static) -> Result<u64, Failure> {
    withBlocking(work).await.map_err(|error| match error {
        cranpose_core::BlockingError::Saturated => Failure::Saturated,
        _ => Failure::Other,
    })
}

pub fn serial(iterations: u64) -> u64 {
    (0..iterations).fold(0_u64, |sum, value| {
        sum.wrapping_add(
            pollster::block_on(run(move || std::hint::black_box(value))).expect("serial job"),
        )
    })
}
