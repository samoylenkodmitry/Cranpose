use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::{AppShell, FrameUpdateResult};
use cranpose_render_wgpu::WgpuRenderer;

use super::pipeline_landing::PipelineLanding;
use crate::wgpu_surface::{
    PresentStep, SurfaceFrame, current_surface_texture, surface_present_step, surface_view,
};

// Draws frames into the canvas.
pub(crate) struct CanvasPresenter {
    surface: Rc<wgpu::Surface<'static>>,
    config: Rc<RefCell<wgpu::SurfaceConfiguration>>,
    // Whether the canvas shows less than the app holds.
    dirty: Rc<Cell<bool>>,
    request_frame: Rc<dyn Fn()>,
    landing: Rc<PipelineLanding>,
    // Asks for a frame that draws again.
    redraw: Rc<dyn Fn()>,
    // Draws the app's current frame at once, outside an animation frame:
    // once the pipelines left out of a first frame landed, the frame drawn
    // with them shows without waiting for the next animation frame and the
    // update that runs first.
    draw_now: Rc<dyn Fn()>,
}

impl CanvasPresenter {
    pub(crate) fn new(
        surface: Rc<wgpu::Surface<'static>>,
        config: Rc<RefCell<wgpu::SurfaceConfiguration>>,
        dirty: Rc<Cell<bool>>,
        request_frame: Rc<dyn Fn()>,
        landing: Rc<PipelineLanding>,
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
                landing,
                redraw,
                draw_now,
            }
        })
    }

    // Whether the frame after `update` of `app` presents: when the canvas
    // shows less than the app holds, or the update changed what it draws.
    // No frame presents while the first frame waits for the pipelines
    // compiled so far, nor while a first frame that showed none of itself
    // waits for the pipelines left out of it. No frame asks then whether
    // the canvas shows the scene: the question builds the frame's packet.
    pub(crate) fn presents(
        &self,
        app: &RefCell<AppShell<WgpuRenderer>>,
        update: FrameUpdateResult,
    ) -> bool {
        self.landing.may_draw()
            && surface_present_step(
                self.dirty.get(),
                update.visual_changed,
                &mut app.borrow_mut().primary(),
            ) == PresentStep::Present
    }

    // Draws `app` into the canvas's next image and presents it, or
    // reconfigures a surface the browser reports lost. The first frame waits
    // until the page holds the pipelines compiled so far. A first frame that
    // left out draws presents what it clears to.
    // The canvas keeps that while its pipelines compile. A presented frame
    // counts in the frame statistics from `started_at`.
    pub(crate) fn present(
        &self,
        app: &RefCell<AppShell<WgpuRenderer>>,
        started_at: web_time::Instant,
    ) -> bool {
        if !self.landing.may_draw() || self.landing.yields_first_frame(&self.draw_now) {
            return false;
        }
        let config = self.config.borrow();
        match current_surface_texture(&self.surface, "web") {
            SurfaceFrame::Ready(output) => {
                let size = (output.texture.width(), output.texture.height());
                {
                    let mut app_mut = app.borrow_mut();
                    self.landing.begin_frame(app_mut.renderer());
                    if let Err(err) = crate::wgpu_surface::render_surface_frame(
                        app_mut.renderer(),
                        &output,
                        config.format,
                        size,
                    ) {
                        log::error!("render failed: {err:?}");
                    }
                    if self.landing.end_frame(&self.redraw, &self.draw_now) {
                        app_mut
                            .renderer()
                            .hide_drawn_frame(&surface_view(&output, config.format));
                    }
                    app_mut.renderer().present(output);
                    app_mut.record_presented_frame(started_at, web_time::Instant::now());
                }
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
