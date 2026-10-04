use std::sync::{Condvar, mpsc};

use super::*;

struct Gate {
    busy: Mutex<bool>,
    wake: Condvar,
}

fn gate() -> &'static Gate {
    static GATE: OnceLock<Gate> = OnceLock::new();
    GATE.get_or_init(|| Gate {
        busy: Mutex::new(false),
        wake: Condvar::new(),
    })
}

struct Permit;

impl Drop for Permit {
    fn drop(&mut self) {
        *gate().busy.lock().unwrap_or_else(PoisonError::into_inner) = false;
        gate().wake.notify_all();
    }
}

fn acquire(deadline: Duration) -> Option<Permit> {
    let busy = gate().busy.lock().unwrap_or_else(PoisonError::into_inner);
    let (mut busy, _) = gate()
        .wake
        .wait_timeout_while(busy, deadline, |busy| *busy)
        .unwrap_or_else(PoisonError::into_inner);
    if *busy {
        return None;
    }
    *busy = true;
    Some(Permit)
}

pub(super) fn run(deadline: Duration) -> DurableSaveOutcome {
    let started = web_time::Instant::now();
    let Some(permit) = acquire(deadline) else {
        return DurableSaveOutcome::TimedOut;
    };
    let Ok(batch) = catch_unwind(AssertUnwindSafe(SaveBatch::collect)) else {
        return DurableSaveOutcome::Failed;
    };
    if batch.is_empty() {
        return DurableSaveOutcome::Nothing;
    }
    let lease = crate::background::acquire_background_work();
    let (send, receive) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("cranpose-durable-save".into())
        .spawn(move || {
            let outcome = batch.run();
            drop(lease);
            drop(permit);
            let _ = send.send(outcome);
        });
    if spawned.is_err() {
        return DurableSaveOutcome::Failed;
    }
    match receive.recv_timeout(deadline.saturating_sub(started.elapsed())) {
        Ok(outcome) => outcome,
        Err(mpsc::RecvTimeoutError::Timeout) => DurableSaveOutcome::TimedOut,
        Err(mpsc::RecvTimeoutError::Disconnected) => DurableSaveOutcome::Failed,
    }
}
