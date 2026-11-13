use std::{
    collections::VecDeque,
    ops::ControlFlow,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose_render_wgpu::{GpuTarget, PresentOutcome, PublishOutcome, WgpuRenderer};

use super::{App, PrimaryPresent, next_frame_anchor, record_pacing_event};

// The frames handed to the present thread and not handed back yet, oldest
// first: each one's id and the time its redraw started.
#[derive(Default)]
pub(super) struct FramesInFlight(VecDeque<(u64, Instant)>);

impl FramesInFlight {
    // The start of frame `frame_id`, which the present thread handed back.
    // The thread hands frames back in the order it took them.
    fn take(&mut self, frame_id: u64) -> Option<Instant> {
        while let Some((id, started_at)) = self.0.pop_front() {
            if id == frame_id {
                return Some(started_at);
            }
        }
        None
    }
}

#[expect(
    clippy::multiple_inherent_impl,
    reason = "the present thread in its module"
)]
impl App {
    // Whether the primary window presents on a thread of its own: on macOS,
    // with the CRANPOSE_PRESENT_THREAD diagnostic on, for a window on the
    // screen that no robot drives.
    fn presents_primary_on_thread(&self) -> bool {
        cfg!(target_os = "macos")
            && !self.settings.headless
            && !self.robot.drives()
            && cranpose_core::DIAGNOSTICS.present_thread.flag()
    }

    // Starts the primary window's `renderer` on `target` and says where its
    // frames present: on a thread of its own, which takes `surface` over,
    // or on the loop's thread. The present thread wakes the loop each time
    // it hands a frame back.
    pub(super) fn start_primary_renderer(
        &self,
        renderer: &mut WgpuRenderer,
        target: GpuTarget,
        surface: wgpu::Surface<'static>,
        config: &wgpu::SurfaceConfiguration,
    ) -> PrimaryPresent {
        if !self.presents_primary_on_thread() {
            renderer.init_gpu(target);
            return PrimaryPresent::OnLoop(surface);
        }
        let proxy = self.event_proxy.clone();
        let waker = Arc::new(move || proxy.wake_up());
        if let Err(error) = renderer.init_gpu_threaded(target.clone(), waker, None, None) {
            log::error!("[present-thread] {error:?}; frames present on the loop's thread");
            renderer.init_gpu(target);
            return PrimaryPresent::OnLoop(surface);
        }
        let config = config.clone();
        if !renderer
            .present_thread()
            .is_some_and(|thread| thread.replace_surface(surface, config))
        {
            log::error!("[present-thread] the present thread took no surface");
        }
        log::info!("[present-thread] the primary window presents on a thread of its own");
        PrimaryPresent::OnPresentThread(FramesInFlight::default())
    }

    // Hands the primary window's frame, which started at `frame_started_at`,
    // to the present thread, which takes the surface's next image, draws
    // the frame into it and presents it. `Break` when no frame went.
    pub(super) fn publish_primary_frame(
        &mut self,
        frame_started_at: Instant,
        frame_interval: Option<Duration>,
    ) -> ControlFlow<()> {
        let (Some(app), Some(PrimaryPresent::OnPresentThread(in_flight)), Some(config)) = (
            self.app.as_mut(),
            self.surface.as_mut(),
            self.surface_config.as_ref(),
        ) else {
            return ControlFlow::Break(());
        };
        let renderer = app.renderer();
        let outcome = renderer.publish_frame(config.width, config.height);
        if outcome != PublishOutcome::Published {
            log::debug!("[present-thread] no frame published: {outcome:?}");
            return ControlFlow::Break(());
        }
        in_flight
            .0
            .push_back((renderer.last_published_frame_id(), frame_started_at));
        self.primary_surface_dirty = false;
        self.primary_initial_present_pending = false;
        self.last_frame_start_time = Some(next_frame_anchor(
            self.last_frame_start_time,
            frame_started_at,
            frame_interval,
        ));
        ControlFlow::Continue(())
    }

    // Folds the frames the present thread handed back into the renderer,
    // which frees their room. A frame on the screen ends as one the loop
    // presented; after a frame the thread refused, the next frame draws.
    pub(super) fn drain_primary_present_returns(&mut self) {
        let (Some(app), Some(PrimaryPresent::OnPresentThread(in_flight))) =
            (self.app.as_mut(), self.surface.as_mut())
        else {
            return;
        };
        let mut presented = None;
        let mut refused = false;
        app.renderer()
            .drain_present_returns_with(&mut |frame_id, outcome, _| {
                let started_at = in_flight.take(frame_id);
                match outcome {
                    PresentOutcome::Presented => presented = started_at.or(presented),
                    PresentOutcome::Unchanged => {}
                    PresentOutcome::Cancelled(_) | PresentOutcome::NotRun => refused = true,
                }
            });
        if refused {
            self.primary_surface_dirty = true;
            app.mark_dirty();
        }
        let Some(started_at) = presented else {
            return;
        };
        record_pacing_event(|diag| &mut diag.presents);
        app.record_presented_frame(started_at, Instant::now());
        self.primary_frame_shown();
    }
}
