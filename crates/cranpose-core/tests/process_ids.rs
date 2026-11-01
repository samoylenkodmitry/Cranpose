use std::{sync::Arc, thread};

use cranpose_core::ProcessIds;

// Ids taken at once from several threads never repeat and start where asked.
#[test]
fn ids_from_many_threads_never_repeat() {
    let ids = Arc::new(ProcessIds::starting_at(1));
    let takers: Vec<_> = (0..4)
        .map(|_| {
            let ids = Arc::clone(&ids);
            thread::spawn(move || (0..1_000).map(|_| ids.next()).collect::<Vec<_>>())
        })
        .collect();
    let mut taken: Vec<u64> = takers
        .into_iter()
        .flat_map(|taker| taker.join().expect("a taker finished"))
        .collect();
    taken.sort_unstable();
    assert_eq!(taken, (1..=4_000).collect::<Vec<u64>>());
}
