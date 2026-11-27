use std::sync::{
    Arc, Condvar, Mutex, PoisonError, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

type InstallWaiter = Arc<(Mutex<Option<Weak<DeviceErrorSentry>>>, Condvar)>;

enum SentrySlot {
    Installing(InstallWaiter),
    Ready(Weak<DeviceErrorSentry>),
}

// The sentry of each device: wgpu keeps one error handler and one loss
// callback per device, so the renderers of one device share its sentry.
static SENTRIES: Mutex<Vec<(Weak<wgpu::Device>, SentrySlot)>> = Mutex::new(Vec::new());

struct InstallAbortGuard<'a> {
    device: &'a Arc<wgpu::Device>,
    waiter: Option<InstallWaiter>,
}

impl Drop for InstallAbortGuard<'_> {
    fn drop(&mut self) {
        let Some(waiter) = self.waiter.take() else {
            return;
        };
        let mut sentries = SENTRIES.lock().unwrap_or_else(PoisonError::into_inner);
        sentries.retain(|(known, slot)| {
            !(known.as_ptr() == Arc::as_ptr(self.device)
                && matches!(slot, SentrySlot::Installing(existing) if Arc::ptr_eq(existing, &waiter)))
        });
        drop(sentries);
        let (lock, cvar) = &*waiter;
        let mut ready = lock.lock().unwrap_or_else(PoisonError::into_inner);
        *ready = Some(Weak::new());
        drop(ready);
        cvar.notify_all();
    }
}

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
        let sentry = loop {
            let mut sentries = SENTRIES.lock().unwrap_or_else(PoisonError::into_inner);
            sentries.retain(|(device, slot)| {
                device.strong_count() > 0
                    && match slot {
                        SentrySlot::Installing(_) => true,
                        SentrySlot::Ready(sentry) => sentry.strong_count() > 0,
                    }
            });
            let existing = sentries
                .iter()
                .find(|(known, _)| known.as_ptr() == Arc::as_ptr(device));
            match existing {
                Some((_, SentrySlot::Ready(sentry))) => {
                    if let Some(sentry) = sentry.upgrade() {
                        drop(sentries);
                        break sentry;
                    }
                }
                Some((_, SentrySlot::Installing(waiter))) => {
                    let waiter = Arc::clone(waiter);
                    drop(sentries);
                    let (lock, cvar) = &*waiter;
                    let mut ready = lock.lock().unwrap_or_else(PoisonError::into_inner);
                    while ready.is_none() {
                        ready = cvar.wait(ready).unwrap_or_else(PoisonError::into_inner);
                    }
                    let installed = ready.as_ref().and_then(Weak::upgrade);
                    drop(ready);
                    if let Some(sentry) = installed {
                        break sentry;
                    }
                    continue;
                }
                None => {}
            }
            let waiter: InstallWaiter = Arc::new((Mutex::new(None), Condvar::new()));
            sentries.retain(|(known, _)| known.as_ptr() != Arc::as_ptr(device));
            sentries.push((
                Arc::downgrade(device),
                SentrySlot::Installing(Arc::clone(&waiter)),
            ));
            drop(sentries);

            let mut abort_guard = InstallAbortGuard {
                device,
                waiter: Some(Arc::clone(&waiter)),
            };
            let sentry = Arc::new(Self::default());
            sentry.install_callbacks(device);
            abort_guard.waiter = None;

            let mut sentries = SENTRIES.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((_, slot)) = sentries.iter_mut().find(|(known, slot)| {
                known.as_ptr() == Arc::as_ptr(device)
                    && matches!(slot, SentrySlot::Installing(existing) if Arc::ptr_eq(existing, &waiter))
            }) {
                *slot = SentrySlot::Ready(Arc::downgrade(&sentry));
            }
            drop(sentries);

            let (lock, cvar) = &*waiter;
            let mut ready = lock.lock().unwrap_or_else(PoisonError::into_inner);
            *ready = Some(Arc::downgrade(&sentry));
            drop(ready);
            cvar.notify_all();
            break sentry;
        };
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

    fn install_callbacks(self: &Arc<Self>, device: &wgpu::Device) {
        if survive_gpu_errors_enabled() {
            let errors = Arc::downgrade(self);
            device.on_uncaptured_error(Arc::new(move |error| {
                if let Some(errors) = errors.upgrade() {
                    errors.record(&error);
                }
            }));
        }
        let loss = Arc::downgrade(self);
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
