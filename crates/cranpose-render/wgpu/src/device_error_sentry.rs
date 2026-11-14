use std::sync::{
    Arc, Mutex, PoisonError, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

// The sentry of each device: wgpu keeps one error handler and one loss
// callback per device, so the renderers of one device share its sentry.
static SENTRIES: Mutex<Vec<(Weak<wgpu::Device>, Weak<DeviceErrorSentry>)>> = Mutex::new(Vec::new());

#[derive(Default)]
pub(crate) struct DeviceErrorSentry {
    errors: AtomicU64,
    poisoned: AtomicBool,
    // The device reported its loss: nothing more is encoded on it.
    lost: AtomicBool,
    // What each renderer of the device, while it lives, asked to hear of
    // its loss: the renderer holds the handler.
    on_device_lost: Mutex<Vec<Weak<dyn Fn() + Send + Sync>>>,
}

impl DeviceErrorSentry {
    // The sentry that hears the uncaptured errors of `device` and its loss,
    // which it reports to `on_device_lost` too. It hears them from now on:
    // a loss before a present thread builds the renderer still counts.
    pub(crate) fn watching(
        device: &Arc<wgpu::Device>,
        on_device_lost: Option<crate::DeviceLostHandler>,
    ) -> Arc<Self> {
        let mut sentries = SENTRIES.lock().unwrap_or_else(PoisonError::into_inner);
        sentries.retain(|(device, sentry)| device.strong_count() > 0 && sentry.strong_count() > 0);
        let known = sentries
            .iter()
            .find(|(known, _)| known.as_ptr() == Arc::as_ptr(device))
            .and_then(|(_, sentry)| sentry.upgrade());
        let sentry = known.unwrap_or_else(|| {
            let sentry = Self::register(device);
            sentries.push((Arc::downgrade(device), Arc::downgrade(&sentry)));
            sentry
        });
        drop(sentries);
        if let Some(on_device_lost) = on_device_lost {
            let mut handlers = sentry
                .on_device_lost
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            handlers.retain(|handler| handler.strong_count() > 0);
            handlers.push(Arc::downgrade(&on_device_lost));
        }
        sentry
    }

    fn register(device: &wgpu::Device) -> Arc<Self> {
        let sentry = Arc::new(Self::default());
        if survive_gpu_errors_enabled() {
            let errors = Arc::downgrade(&sentry);
            device.on_uncaptured_error(Arc::new(move |error| {
                if let Some(errors) = errors.upgrade() {
                    errors.record(&error);
                }
            }));
        }
        let loss = Arc::downgrade(&sentry);
        device.set_device_lost_callback(move |reason, message| {
            log::error!("[gpu-device] device lost ({reason:?}): {message}");
            let Some(loss) = loss.upgrade() else {
                return;
            };
            loss.lost.store(true, Ordering::Release);
            let handlers = loss
                .on_device_lost
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone();
            for on_device_lost in handlers.iter().filter_map(Weak::upgrade) {
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
