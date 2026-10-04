#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;
use std::{num::NonZeroUsize, time::Duration};

use crate::Dispatcher;
#[cfg(target_arch = "wasm32")]
use crate::{Dispatch, Runnable, SystemClock};

#[cfg(not(target_arch = "wasm32"))]
#[path = "dispatcher_pool/native.rs"]
mod native;

/// Concurrency and retention limits for a [`DispatcherPool`].
///
/// Each lane defaults to the machine's available parallelism. Idle workers
/// retire after 30 seconds. Limits apply to native coroutine steps, not the
/// number of live coroutines; applications should bound their producers.
#[derive(Clone, Copy, Debug)]
pub struct DispatcherPoolConfig {
    /// Maximum concurrently executing CPU steps.
    pub cpu_parallelism: NonZeroUsize,
    /// Maximum concurrently executing I/O steps. CPU work has separate capacity.
    pub io_parallelism: NonZeroUsize,
    /// How long an unused worker retains its thread and thread-local resources.
    pub idle_timeout: Duration,
}

impl Default for DispatcherPoolConfig {
    fn default() -> Self {
        let parallelism = std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN);
        Self {
            cpu_parallelism: parallelism,
            io_parallelism: parallelism,
            idle_timeout: Duration::from_secs(30),
        }
    }
}

/// CPU and I/O dispatchers sharing a demand-driven worker pool.
///
/// Native workers start when work arrives, serve either lane, and retire when
/// idle. Each lane has its own concurrency limit; the total worker limit is
/// their sum. Blocking every I/O worker does not consume CPU capacity.
///
/// Dispatch never waits for queue capacity or discards coroutine wake-ups.
/// Queued steps are retained until run, including the step that drops a
/// cancelled future. Limit concurrently launched coroutines at the producer
/// when their captured data needs a memory bound. Cancellation cannot interrupt
/// a blocking call already executing inside a coroutine step.
///
/// The dispatchers keep the pool alive independently of this handle. Dropping
/// its last dispatcher shuts down idle workers. On Web both dispatchers use
/// the browser event loop; concurrency limits do not make blocking code safe.
pub struct DispatcherPool {
    dispatchers: [Dispatcher; 2],
}

impl DispatcherPool {
    /// Creates a pool without starting native worker threads.
    pub fn new(config: DispatcherPoolConfig) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let dispatchers = {
            let owner = native::Owner::new(
                "coroflow-worker",
                [config.cpu_parallelism.get(), config.io_parallelism.get()],
                Some(config.idle_timeout),
            );
            std::array::from_fn(|lane| native::dispatcher(Arc::clone(&owner), lane))
        };
        #[cfg(target_arch = "wasm32")]
        let dispatchers = {
            let _ = config;
            std::array::from_fn(|_| event_loop())
        };
        Self { dispatchers }
    }

    /// A dispatcher for CPU-bound coroutine steps.
    pub fn cpu(&self) -> Dispatcher {
        self.dispatchers[0].clone()
    }

    /// A dispatcher for steps that may block on I/O.
    pub fn io(&self) -> Dispatcher {
        self.dispatchers[1].clone()
    }
}

pub(crate) fn single_thread(name: &str) -> Dispatcher {
    #[cfg(not(target_arch = "wasm32"))]
    {
        native::dispatcher(native::Owner::new(name, [1, 0], None), 0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = name;
        event_loop()
    }
}

#[cfg(target_arch = "wasm32")]
fn event_loop() -> Dispatcher {
    struct EventLoop;
    impl Dispatch for EventLoop {
        fn dispatch(&self, runnable: Runnable) {
            wasm_bindgen_futures::spawn_local(async move { runnable.run() });
        }
    }
    Dispatcher::new(EventLoop, SystemClock::shared())
}
