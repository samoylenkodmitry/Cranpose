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
    #[cfg(any(
        all(feature = "android", target_os = "android"),
        all(feature = "ios", target_os = "ios"),
        all(feature = "web", target_arch = "wasm32")
    ))]
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

/// A debuggable build launched with `cranpose_lose_gpu_device` (an Android
/// extra or a `--cranpose_lose_gpu_device` argument) reports its GPU device
/// lost once, after its first 120 presented frames, to try the recovery on a
/// device. It reports the loss as the device's loss callback does rather than
/// destroying the device: a destroy while the present thread draws freezes
/// some drivers, where a driver reset reports a loss without that.
#[cfg(any(
    all(feature = "android", target_os = "android"),
    all(feature = "ios", target_os = "ios")
))]
pub(crate) struct LossDrill {
    frames_left: Option<u32>,
}

#[cfg(any(
    all(feature = "android", target_os = "android"),
    all(feature = "ios", target_os = "ios")
))]
impl LossDrill {
    pub(crate) fn from_launch_args() -> Self {
        let asked = cranpose_services::is_debuggable()
            && cranpose_services::launch_args()
                .boolean("cranpose_lose_gpu_device")
                .unwrap_or(false);
        Self {
            frames_left: asked.then_some(120),
        }
    }

    /// A frame presented; after the drill's last one, `signal` reports a
    /// loss.
    pub(crate) fn presented(&mut self, signal: &LossSignal) {
        let Some(left) = self.frames_left.as_mut() else {
            return;
        };
        *left = left.saturating_sub(1);
        if *left > 0 {
            return;
        }
        self.frames_left = None;
        log::warn!("[gpu-device] reporting a device loss, as cranpose_lose_gpu_device asks");
        signal.report_loss();
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
