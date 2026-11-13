use std::sync::{atomic::Ordering, mpsc::Receiver};

use crate::{
    DeviceLostHandler, GpuTarget,
    pipeline_compiler::PipelineCompilation,
    present_runtime::{PresentControl, PresentHandle},
};

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
    pub(crate) renderer_epoch: &'a mut u64,
    pub(crate) surface_epoch: &'a mut u64,
    pub(crate) on_device_lost: Option<&'a DeviceLostHandler>,
    pub(crate) background: wgpu::Color,
}

impl PresentThread<'_> {
    fn next_surface_epoch(&mut self) -> u64 {
        *self.surface_epoch = self.surface_epoch.wrapping_add(1);
        *self.surface_epoch
    }

    /// Gives the thread a (re)created surface, configured as `config`,
    /// which first shows the renderer's background. Nothing waits for the
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
            background: self.background,
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
        self.handle
            .send_control_and_wait(
                move |ack| PresentControl::Reconfigure {
                    config,
                    surface_epoch,
                    ack: Some(ack),
                },
                "reconfigure surface",
            )
            .is_some()
    }

    /// Configures the thread's surface as `config`, a new size or present
    /// mode of a window, waiting for nothing: frames published afterwards
    /// draw on the surface so configured.
    pub fn resize(mut self, config: wgpu::SurfaceConfiguration) -> bool {
        let surface_epoch = self.next_surface_epoch();
        self.handle.send_control(PresentControl::Reconfigure {
            config,
            surface_epoch,
            ack: None,
        })
    }

    /// Takes the thread's surface back and waits until the thread hands it
    /// over, as when the window that showed it is gone or the app leaves the
    /// screen: the thread draws nothing more into it. The renderer stays for
    /// the next surface. `None` when the thread had no surface or is gone.
    pub fn take_surface(mut self) -> Option<wgpu::Surface<'static>> {
        self.next_surface_epoch();
        self.handle
            .send_control_and_wait(
                |reply| PresentControl::TakeSurface { reply },
                "take surface",
            )
            .flatten()
    }

    /// Tells the thread whether the window shows none of the surface. While
    /// it shows none, the thread takes no image and holds the newest frame
    /// back until the window shows it again.
    pub fn set_occluded(self, occluded: bool) -> bool {
        self.handle.send_control(PresentControl::Occluded(occluded))
    }

    /// Moves the thread's renderer to `target`, a new device after the last
    /// was lost. The surface stays, configured on the new device; frames
    /// built for the old renderer are refused.
    pub fn replace_device(self, target: GpuTarget) -> bool {
        *self.renderer_epoch = self.renderer_epoch.wrapping_add(1);
        let gpu = target.renderer_init(
            *self.renderer_epoch,
            PipelineCompilation::Background,
            self.on_device_lost.cloned(),
        );
        self.handle
            .send_control(PresentControl::ReplaceDevice(Box::new(gpu)))
    }

    /// Destroys the thread's device between two frames and waits until the
    /// device reported its loss, as a driver reset would lose it: a robot's
    /// device loss drill.
    #[doc(hidden)]
    pub fn lose_device(self) -> bool {
        self.handle
            .send_control_and_wait(|ack| PresentControl::LoseDevice { ack }, "lose device")
            .is_some()
    }

    #[doc(hidden)]
    pub fn attach_offscreen_for_tests(self, width: u32, height: u32, images: TestImages) -> bool {
        self.handle
            .send_control_and_wait(
                move |ack| PresentControl::AttachOffscreenTargetForTests {
                    width,
                    height,
                    images,
                    ack,
                },
                "attach offscreen target",
            )
            .is_some()
    }

    #[doc(hidden)]
    pub fn send_attach_offscreen_unacked_for_tests(
        self,
        width: u32,
        height: u32,
        images: TestImages,
    ) -> Option<Receiver<()>> {
        self.handle
            .send_control_unacked(move |ack| PresentControl::AttachOffscreenTargetForTests {
                width,
                height,
                images,
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
                ack: Some(ack),
            })
    }

    #[doc(hidden)]
    pub fn send_take_surface_unacked_for_tests(
        self,
    ) -> Option<Receiver<Option<wgpu::Surface<'static>>>> {
        self.handle
            .send_control_unacked(|reply| PresentControl::TakeSurface { reply })
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
