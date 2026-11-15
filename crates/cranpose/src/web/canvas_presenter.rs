use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;

use super::pipeline_discovery::PipelineDiscovery;
use crate::wgpu_surface::{SurfaceFrame, current_surface_texture};

/// Draws frames into the canvas.
pub(crate) struct CanvasPresenter {
    surface: Rc<wgpu::Surface<'static>>,
    config: Rc<RefCell<wgpu::SurfaceConfiguration>>,
    /// Whether the canvas shows less than the app holds.
    dirty: Rc<Cell<bool>>,
    request_frame: Rc<dyn Fn()>,
    discovery: Rc<PipelineDiscovery>,
    /// Asks for a frame that draws again.
    redraw: Rc<dyn Fn()>,
    /// Draws the frame the app holds at once, outside an animation frame:
    /// once the first frame's pipelines compiled, the frame they were
    /// discovered for shows without waiting for the next animation frame
    /// and the update it runs first.
    draw_now: Rc<dyn Fn()>,
}

impl CanvasPresenter {
    pub(crate) fn new(
        surface: Rc<wgpu::Surface<'static>>,
        config: Rc<RefCell<wgpu::SurfaceConfiguration>>,
        dirty: Rc<Cell<bool>>,
        request_frame: Rc<dyn Fn()>,
        discovery: Rc<PipelineDiscovery>,
        app: std::rc::Weak<RefCell<AppShell<WgpuRenderer>>>,
    ) -> Rc<Self> {
        let redraw: Rc<dyn Fn()> = {
            let dirty = Rc::clone(&dirty);
            let request_frame = Rc::clone(&request_frame);
            Rc::new(move || {
                dirty.set(true);
                request_frame();
            })
        };
        Rc::new_cyclic(|presenter: &std::rc::Weak<Self>| {
            let presenter = std::rc::Weak::clone(presenter);
            let request_redraw = Rc::clone(&redraw);
            let draw_now: Rc<dyn Fn()> =
                Rc::new(move || match (presenter.upgrade(), app.upgrade()) {
                    (Some(presenter), Some(app)) if app.try_borrow_mut().is_ok() => {
                        presenter.present(&app, web_time::Instant::now());
                    }
                    _ => request_redraw(),
                });
            Self {
                surface,
                config,
                dirty,
                request_frame,
                discovery,
                redraw,
                draw_now,
            }
        })
    }

    /// Draws `app` into the canvas's next image and presents it, or
    /// reconfigures a surface the browser reports lost. While the first
    /// frame's pipelines compile, the canvas keeps what it shows. A
    /// presented frame counts in the frame statistics from `started_at`.
    pub(crate) fn present(
        &self,
        app: &RefCell<AppShell<WgpuRenderer>>,
        started_at: web_time::Instant,
    ) -> bool {
        if !self.discovery.may_draw() {
            return false;
        }
        let config = self.config.borrow();
        match current_surface_texture(&self.surface, "web") {
            SurfaceFrame::Ready(output) => {
                let size = (output.texture.width(), output.texture.height());
                {
                    let mut app_mut = app.borrow_mut();
                    self.discovery.begin_frame(app_mut.renderer());
                    if let Err(err) = crate::wgpu_surface::render_surface_frame(
                        app_mut.renderer(),
                        &output,
                        config.format,
                        size,
                    ) {
                        log::error!("render failed: {err:?}");
                    }
                    app_mut.renderer().present(output);
                    app_mut.record_presented_frame(started_at, web_time::Instant::now());
                }
                self.discovery.end_frame(&self.redraw, &self.draw_now);
                self.dirty.set(false);
                true
            }
            SurfaceFrame::Reconfigure => {
                {
                    let mut app_mut = app.borrow_mut();
                    if let Some(device) = app_mut.renderer().try_device() {
                        self.surface.configure(device, &config);
                    } else {
                        log::error!(
                            "web surface reconfigure skipped: GPU renderer is not initialized"
                        );
                    }
                }
                self.dirty.set(true);
                (self.request_frame)();
                false
            }
            SurfaceFrame::Skip => {
                self.dirty.set(true);
                false
            }
        }
    }
}
