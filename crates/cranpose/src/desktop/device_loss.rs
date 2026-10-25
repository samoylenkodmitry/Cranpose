//! Recovery of the desktop shell's GPU device. When the device is lost, the
//! next pass before the event loop waits opens another on the adapter (or on
//! a new adapter), configures every window's surface with it, hands it to
//! every window's renderer and gives the composition the new
//! [`GpuContext`](crate::GpuContext). The composition, its state and the
//! input it holds stay as they were.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_render_wgpu::DeviceLostHandler;
use winit::event_loop::{ActiveEventLoop, EventLoopProxy};

use super::{App, request_primary_device};
use crate::{app_launcher::LaunchError, gpu_loss::LossSignal};

/// What the shell knows about the losses of its devices. After
/// [`crate::gpu_loss::MAX_RECOVERIES`] recoveries without a frame, a lost
/// device ends the app with [`LaunchError::GpuDeviceLost`].
#[derive(Default)]
pub(super) struct DeviceLoss {
    signal: LossSignal,
    /// The GPU context the composition reads.
    content: Option<Rc<RefCell<crate::GpuContext>>>,
    /// The losses in a row a robot asked for, which the next pass before
    /// the event loop waits makes.
    #[cfg(feature = "robot")]
    robot_losses: u32,
}

impl DeviceLoss {
    /// What a renderer of the current device runs when that device is lost.
    pub(super) fn handler(&self, proxy: &EventLoopProxy) -> DeviceLostHandler {
        let proxy = proxy.clone();
        self.signal.handler(move || proxy.wake_up())
    }

    /// The GPU context for the composition: `gpu` until a recovery gives it
    /// the new device's.
    pub(super) fn share(
        &mut self,
        gpu: crate::GpuContext,
    ) -> impl Fn() -> crate::GpuContext + 'static {
        let shared = Rc::new(RefCell::new(gpu));
        self.content = Some(Rc::clone(&shared));
        move || shared.borrow().clone()
    }

    /// A frame reached the screen, so the device works.
    pub(super) fn presented(&mut self) {
        self.signal.presented();
    }

    /// Asks the next pass before the event loop waits to lose the device
    /// `times` times in a row.
    #[cfg(feature = "robot")]
    pub(super) fn lose_for_robot(&mut self, times: u32) {
        self.robot_losses = times;
    }
}

impl App {
    /// Destroys the device the times a robot asked for, as driver resets
    /// would lose it, and recovers after each loss before any frame; then
    /// answers the robot.
    #[cfg(feature = "robot")]
    pub(super) fn serve_robot_device_losses(&mut self, event_loop: &dyn ActiveEventLoop) {
        let times = std::mem::take(&mut self.device_loss.robot_losses);
        if times == 0 {
            return;
        }
        for _ in 0..times {
            if let Some(context) = &self.gpu_context {
                crate::gpu_loss::lose_device(&context.gpu.device);
            }
            self.recover_lost_device(event_loop);
        }
        if let Some(controller) = &self.robot_controller {
            controller.respond(crate::robot::RobotResponse::Ok);
        }
    }

    /// Replaces a lost device, or ends the app when that keeps failing.
    pub(super) fn recover_lost_device(&mut self, event_loop: &dyn ActiveEventLoop) {
        if !self.device_loss.signal.take_loss() || self.exiting {
            return;
        }
        if !self.device_loss.signal.begin_recovery() {
            self.abort_launch(event_loop, LaunchError::GpuDeviceLost);
            return;
        }
        if let Err(error) = self.replace_device() {
            self.abort_launch(event_loop, error);
        }
    }

    fn replace_device(&mut self) -> Result<(), LaunchError> {
        let (Some(context), Some(app)) = (self.gpu_context.as_mut(), self.app.as_mut()) else {
            return Ok(());
        };
        let requirements = self.settings.gpu_requirements;
        let (adapter, (device, queue)) =
            match request_primary_device(&context.gpu.adapter, requirements) {
                Ok(opened) => (context.gpu.adapter.clone(), opened),
                Err(_) => {
                    let adapter = crate::wgpu_surface::request_adapter(
                        &context.gpu.instance,
                        self.surface.as_ref(),
                    )
                    .map_err(LaunchError::NoAdapter)?;
                    let opened = request_primary_device(&adapter, requirements)
                        .map_err(LaunchError::DeviceCreate)?;
                    (adapter, opened)
                }
            };
        log::warn!("[gpu-device] opened a new device after the last one was lost");
        let backend = adapter.get_info().backend;
        let downlevel = adapter.get_downlevel_capabilities().flags;
        context.gpu.adapter = adapter;
        context.gpu.device = Arc::new(device);
        context.gpu.queue = Arc::new(queue);
        context.adapter_backend = backend;
        let handler = self.device_loss.handler(&self.event_proxy);
        let gpu = &context.gpu;
        let hand_over = |renderer: &mut cranpose_render_wgpu::WgpuRenderer, format| {
            renderer.set_device_lost_handler(Arc::clone(&handler));
            renderer.init_gpu(
                Arc::clone(&gpu.device),
                Arc::clone(&gpu.queue),
                crate::surface_format::display_surface_view_format(format),
                backend,
                downlevel,
            );
        };
        if let (Some(surface), Some(config)) = (&self.surface, &self.surface_config) {
            surface.configure(&gpu.device, config);
            hand_over(app.renderer(), config.format);
        }
        for native in self.native_windows.values() {
            native
                .surface
                .configure(&gpu.device, &native.surface_config);
            if let Some(mut surface) = super::native_surface(app, native) {
                hand_over(surface.renderer(), native.surface_config.format);
            }
        }
        if let Some(content) = &self.device_loss.content {
            *content.borrow_mut() = gpu.clone();
        }
        app.request_root_render();
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        Ok(())
    }
}
