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

/// CPU and I/O dispatchers with coordinated, demand-driven worker budgets.
///
/// Each lane has its own queue and native workers, which start when work
/// arrives and retire when idle. The total worker limit is the sum of the
/// lane limits. Blocking every I/O worker does not consume CPU capacity or
/// contend with CPU scheduling on the same queue lock.
///
/// Dispatch never waits for queue capacity or discards coroutine wake-ups.
/// Queued steps are retained until run, including the step that drops a
/// cancelled future. Limit concurrently launched coroutines at the producer
/// when their captured data needs a memory bound. Cancellation cannot interrupt
/// a blocking call already executing inside a coroutine step.
///
/// Each dispatcher keeps its lane alive independently of this handle. Dropping
/// a lane's last dispatcher shuts down its idle workers. On Web both lanes use
/// the browser event loop; concurrency limits do not make blocking code safe.
pub struct DispatcherPool {
    dispatchers: [Dispatcher; 2],
}

impl DispatcherPool {
    /// Creates a pool without starting native worker threads.
    pub fn new(config: DispatcherPoolConfig) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let dispatchers = [
            ("coroflow-cpu", config.cpu_parallelism),
            ("coroflow-io", config.io_parallelism),
        ]
        .map(|(name, parallelism)| {
            native::dispatcher(name, parallelism.get(), Some(config.idle_timeout))
        });
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
        native::dispatcher(name, 1, None)
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
