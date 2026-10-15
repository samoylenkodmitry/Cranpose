//! Whether a host's GPU device was lost, for the host to open another.
//!
//! The renderer's loss callback marks the generation of the device it ran
//! on, so the loss that a replaced device reports as it is dropped is told
//! apart from the loss of the device in use. A host gives up after
//! [`MAX_RECOVERIES`] recoveries in a row without a presented frame.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use cranpose_render_wgpu::DeviceLostHandler;

/// Recoveries in a row, without a frame presented between them, after which
/// a host stops replacing its device.
pub(crate) const MAX_RECOVERIES: u32 = 3;

/// The losses of one host's devices.
pub(crate) struct LossSignal {
    /// The generation of the device whose loss is pending; 0 for none.
    lost: Arc<AtomicU64>,
    generation: u64,
    recoveries: u32,
}

impl Default for LossSignal {
    fn default() -> Self {
        Self {
            lost: Arc::default(),
            generation: 1,
            recoveries: 0,
        }
    }
}

impl LossSignal {
    /// What a renderer of the current device runs when that device is lost:
    /// marks the loss and calls `wake`, from whatever thread wgpu reports it
    /// on.
    pub(crate) fn handler(&self, wake: impl Fn() + Send + Sync + 'static) -> DeviceLostHandler {
        let (lost, generation) = (Arc::clone(&self.lost), self.generation);
        Arc::new(move || {
            lost.store(generation, Ordering::SeqCst);
            wake();
        })
    }

    /// A frame reached the screen, so the device works.
    pub(crate) fn presented(&mut self) {
        self.recoveries = 0;
    }

    /// Whether the current device was lost since the last call.
    pub(crate) fn take_loss(&self) -> bool {
        self.lost.swap(0, Ordering::SeqCst) == self.generation
    }

    /// Marks the current device lost, as its loss callback does.
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    pub(crate) fn report_loss(&self) {
        self.lost.store(self.generation, Ordering::SeqCst);
    }

    /// Starts replacing the lost device, whose successor takes the next
    /// generation. False once [`MAX_RECOVERIES`] in a row brought no frame:
    /// the host stops then.
    pub(crate) fn begin_recovery(&mut self) -> bool {
        if self.recoveries >= MAX_RECOVERIES {
            return false;
        }
        self.recoveries += 1;
        self.generation += 1;
        true
    }
}

/// How long after its first presented frame a drill reports its device lost.
#[cfg(any(
    all(feature = "android", target_os = "android"),
    all(feature = "ios", target_os = "ios")
))]
const DRILL_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// A debuggable build launched with `cranpose_lose_gpu_device` (an Android
/// extra or a `--cranpose_lose_gpu_device` argument) reports its GPU device
/// lost once, [`DRILL_DELAY`] after its first presented frame, to try the
/// recovery on a device. A timer rather than a frame count fires it, so an
/// app that draws no frames while idle runs the drill too. It reports the
/// loss as the device's loss callback does rather than destroying the
/// device: a destroy while the present thread draws freezes some drivers,
/// where a driver reset reports a loss without that.
#[cfg(any(
    all(feature = "android", target_os = "android"),
    all(feature = "ios", target_os = "ios")
))]
pub(crate) struct LossDrill {
    /// The loss callback of the first device, until the drill starts.
    lose: Option<DeviceLostHandler>,
}

#[cfg(any(
    all(feature = "android", target_os = "android"),
    all(feature = "ios", target_os = "ios")
))]
impl LossDrill {
    /// The drill the launch asks for, which reports its loss through
    /// `signal` and then calls `wake`.
    pub(crate) fn from_launch_args(
        signal: &LossSignal,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let asked = cranpose_services::is_debuggable()
            && cranpose_services::launch_args()
                .boolean("cranpose_lose_gpu_device")
                .unwrap_or(false);
        Self {
            lose: asked.then(|| signal.handler(wake)),
        }
    }

    /// A frame presented; the first one starts the drill's timer.
    pub(crate) fn presented(&mut self) {
        let Some(lose) = self.lose.take() else {
            return;
        };
        let timer = std::thread::Builder::new()
            .name("cranpose-loss-drill".into())
            .spawn(move || {
                std::thread::sleep(DRILL_DELAY);
                log::warn!(
                    "[gpu-device] reporting a device loss, as cranpose_lose_gpu_device asks"
                );
                lose();
            });
        if let Err(error) = timer {
            log::warn!("[gpu-device] the device loss drill could not start: {error}");
        }
    }
}

/// Destroys `device`, as a driver reset would lose it, for a robot test of
/// the recovery.
#[cfg(feature = "robot")]
pub(crate) fn lose_device(device: &wgpu::Device) {
    device.destroy();
    // A device reports its loss when it is polled, as the next frame's
    // submission would.
    let _ = device.poll(wgpu::PollType::Poll);
}
