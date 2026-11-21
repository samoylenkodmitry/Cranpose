use super::*;

#[test]
fn concurrent_arming_never_loses_a_wake_up() {
    let rounds = 40;
    let threads: Vec<_> = (0..8)
        .map(|worker| {
            std::thread::spawn(move || {
                for round in 0..rounds {
                    let millis = 1 + ((worker + round) % 5) as u64;
                    pollster::block_on(delay(Duration::from_millis(millis)));
                }
            })
        })
        .collect();
    for thread in threads {
        thread.join().expect("every waiter is woken");
    }
}
