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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn blocking_work_reuses_its_threads_instead_of_one_per_call() {
    use std::{collections::HashSet, sync::mpsc};

    let pool = BlockingPool::new();

    let (sender, receiver) = mpsc::channel();
    for _ in 0..16 {
        let done = Arc::new((Mutex::new(false), Condvar::new()));
        let waiter = Arc::clone(&done);
        let sender = sender.clone();
        pool.submit(Box::new(move || {
            let _ = sender.send(std::thread::current().id());
            let (lock, signal) = &*done;
            *lock.lock().unwrap_or_else(PoisonError::into_inner) = true;
            signal.notify_all();
        }));
        let (lock, signal) = &*waiter;
        let mut finished = lock.lock().unwrap_or_else(PoisonError::into_inner);
        while !*finished {
            finished = signal
                .wait(finished)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    drop(sender);

    let threads: HashSet<_> = receiver.iter().collect();
    assert!(
        threads.len() < 16,
        "sixteen serial jobs used {} threads; the pool is not reusing them",
        threads.len()
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn blocking_work_grows_so_one_slow_job_cannot_hold_up_another() {
    let pool = BlockingPool::new();
    let started = Arc::new((Mutex::new(0usize), Condvar::new()));
    let release = Arc::new((Mutex::new(false), Condvar::new()));

    for _ in 0..4 {
        let started = Arc::clone(&started);
        let release = Arc::clone(&release);
        pool.submit(Box::new(move || {
            {
                let (count, signal) = &*started;
                *count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
                signal.notify_all();
            }
            let (held, signal) = &*release;
            let mut go = held.lock().unwrap_or_else(PoisonError::into_inner);
            while !*go {
                go = signal.wait(go).unwrap_or_else(PoisonError::into_inner);
            }
        }));
    }

    let (count, signal) = &*started;
    let mut running = count.lock().unwrap_or_else(PoisonError::into_inner);
    while *running < 4 {
        running = signal.wait(running).unwrap_or_else(PoisonError::into_inner);
    }
    drop(running);

    let (held, signal) = &*release;
    *held.lock().unwrap_or_else(PoisonError::into_inner) = true;
    signal.notify_all();
}
