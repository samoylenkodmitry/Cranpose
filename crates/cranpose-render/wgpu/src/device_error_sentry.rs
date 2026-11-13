use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[derive(Default)]
pub(crate) struct DeviceErrorSentry {
    errors: AtomicU64,
    poisoned: AtomicBool,
    // The device reported its loss: nothing more is encoded on it.
    lost: AtomicBool,
}

impl DeviceErrorSentry {
    // A sentry that hears the uncaptured errors of `device` and its loss,
    // which it reports to `on_device_lost`. It hears them from now on: a
    // loss before a present thread builds the renderer still counts.
    pub(crate) fn watching(
        device: &wgpu::Device,
        on_device_lost: Option<crate::DeviceLostHandler>,
    ) -> Arc<Self> {
        let sentry = Arc::new(Self::default());
        if survive_gpu_errors_enabled() {
            let errors = Arc::clone(&sentry);
            device.on_uncaptured_error(Arc::new(move |error| errors.record(&error)));
        }
        let loss = Arc::clone(&sentry);
        device.set_device_lost_callback(move |reason, message| {
            loss.lost.store(true, Ordering::Release);
            log::error!("[gpu-device] device lost ({reason:?}): {message}");
            if let Some(on_device_lost) = &on_device_lost {
                on_device_lost();
            }
        });
        sentry
    }

    fn record(&self, error: &wgpu::Error) {
        self.poisoned.store(true, Ordering::Release);
        let count = self.errors.fetch_add(1, Ordering::Relaxed) + 1;
        if count.is_power_of_two() {
            log::error!("[gpu-device] uncaptured wgpu error #{count}: {error}");
        }
    }

    pub(crate) fn lost(&self) -> bool {
        self.lost.load(Ordering::Acquire)
    }

    pub(crate) fn take_poison(&self) -> bool {
        self.poisoned.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn error_count(&self) -> u64 {
        self.errors.load(Ordering::Relaxed)
    }
}

fn survive_gpu_errors_enabled() -> bool {
    !cranpose_core::DIAGNOSTICS.survive_gpu_errors.equals("0")
}
