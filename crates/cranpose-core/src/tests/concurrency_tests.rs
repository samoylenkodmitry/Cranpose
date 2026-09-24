use super::*;

#[test]
fn a_delay_resolves_after_its_deadline() {
    let started = Instant::now();
    pollster::block_on(delay(Duration::from_millis(30)));
    assert!(started.elapsed() >= Duration::from_millis(25));
}

#[test]
fn many_delays_share_one_timer_and_all_fire() {
    let started = Instant::now();
    pollster::block_on(async {
        for _ in 0..4 {
            delay(Duration::from_millis(5)).await;
        }
    });
    assert!(started.elapsed() >= Duration::from_millis(15));
}

#[test]
fn an_elapsed_delay_is_ready_without_arming_the_timer() {
    let mut future = Box::pin(Delay {
        deadline: Instant::now() - Duration::from_millis(1),
        armed: false,
        fired: Arc::new(AtomicBool::new(false)),
    });
    let waker = Waker::noop().clone();
    assert!(
        future
            .as_mut()
            .poll(&mut Context::from_waker(&waker))
            .is_ready()
    );
}
