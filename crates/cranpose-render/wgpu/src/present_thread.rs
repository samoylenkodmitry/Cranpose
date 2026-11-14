use std::sync::{atomic::Ordering, mpsc::Receiver};

use crate::{present_handle::PresentHandle, present_runtime::PresentControl};

/// How a test target hands out the images its frames draw into.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestImages {
    /// A new texture for each frame, which holds nothing known.
    Fresh,
    /// So many images in turn, each holding what it presented, as a
    /// swapchain that keeps presented images does.
    Kept(usize),
}

/// The thread a [`WgpuRenderer`](crate::WgpuRenderer) hands its frames to,
/// from [`WgpuRenderer::present_thread`](crate::WgpuRenderer::present_thread).
/// The thread takes the surface's images, draws the frames into them and
/// presents them. Every change of the surface counts a new surface epoch, so
/// a frame built for the surface as it was is refused instead of drawn.
pub struct PresentThread<'a> {
    pub(crate) handle: &'a mut PresentHandle,
    pub(crate) surface_epoch: &'a mut u64,
}

impl PresentThread<'_> {
    fn next_surface_epoch(&mut self) -> u64 {
        *self.surface_epoch = self.surface_epoch.wrapping_add(1);
        *self.surface_epoch
    }

    /// Gives the thread a (re)created surface, configured as `config`,
    /// which first shows the default background. Nothing waits for the
    /// thread to set it up: frames published afterwards queue behind it.
    /// False when the thread is gone.
    pub fn replace_surface(
        mut self,
        surface: wgpu::Surface<'static>,
        config: wgpu::SurfaceConfiguration,
    ) -> bool {
        let surface_epoch = self.next_surface_epoch();
        let sent = self.handle.send_control(PresentControl::ReplaceSurface {
            surface,
            config,
            surface_epoch,
        });
        if !sent {
            log::error!("[present-runtime] replace surface: runtime is gone");
        }
        sent
    }

    /// Configures the thread's surface as `config` and waits until the
    /// thread has done it, as a surface the platform hands over anew needs.
    pub fn reconfigure(mut self, config: wgpu::SurfaceConfiguration) -> bool {
        let surface_epoch = self.next_surface_epoch();
        self.handle.send_control_and_wait(
            move |ack| PresentControl::Reconfigure {
                config,
                surface_epoch,
                ack,
            },
            "reconfigure surface",
        )
    }

    /// Drops the thread's surface and waits until the thread has done it,
    /// as when the window that showed it is gone: the thread draws nothing
    /// more into it. The renderer stays for the next surface.
    pub fn drop_surface(mut self) -> bool {
        self.next_surface_epoch();
        self.handle
            .send_control_and_wait(|ack| PresentControl::DropSurface { ack }, "drop surface")
    }

    #[doc(hidden)]
    pub fn attach_offscreen_for_tests(
        mut self,
        width: u32,
        height: u32,
        images: TestImages,
    ) -> bool {
        let surface_epoch = self.next_surface_epoch();
        self.handle.send_control_and_wait(
            move |ack| PresentControl::AttachOffscreenTargetForTests {
                width,
                height,
                images,
                surface_epoch,
                ack,
            },
            "attach offscreen target",
        )
    }

    #[doc(hidden)]
    pub fn send_attach_offscreen_unacked_for_tests(
        mut self,
        width: u32,
        height: u32,
        images: TestImages,
    ) -> Option<Receiver<()>> {
        let surface_epoch = self.next_surface_epoch();
        self.handle
            .send_control_unacked(move |ack| PresentControl::AttachOffscreenTargetForTests {
                width,
                height,
                images,
                surface_epoch,
                ack,
            })
    }

    #[doc(hidden)]
    pub fn send_reconfigure_unacked_for_tests(
        self,
        config: wgpu::SurfaceConfiguration,
    ) -> Option<Receiver<()>> {
        let surface_epoch = *self.surface_epoch;
        self.handle
            .send_control_unacked(move |ack| PresentControl::Reconfigure {
                config,
                surface_epoch,
                ack,
            })
    }

    #[doc(hidden)]
    pub fn send_drop_surface_unacked_for_tests(self) -> Option<Receiver<()>> {
        self.handle
            .send_control_unacked(|ack| PresentControl::DropSurface { ack })
    }

    #[doc(hidden)]
    pub fn status_snapshot_for_tests(&self) -> (bool, u64, u64) {
        let status = self.handle.status();
        (
            status.warmup_due(),
            status.presented_frames.load(Ordering::Relaxed),
            status.placeholder_frames.load(Ordering::Relaxed),
        )
    }
}
