use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant},
};

use cranpose_render_wgpu::{
    CancelReason, GpuTarget, PresentOutcome, PresentTimings, PublishOutcome, WgpuRenderer,
};
use winit::{event::WindowEvent, event_loop::EventLoopProxy, window::Window};

// Where a window's frames present: on the loop's thread, into the surface
// the loop holds, or on the window renderer's present thread, which holds
// the surface, with the frames handed to it and not handed back and whether
// the window server shows none of the window.
pub(crate) enum WindowPresent {
    OnLoop(wgpu::Surface<'static>),
    OnPresentThread {
        frames: FramesInFlight,
        occluded: bool,
    },
}

// A frame handed to the present thread: its id, when its redraw started and
// when its update ended.
struct FrameInFlight {
    id: u64,
    started_at: Instant,
    after_update: Instant,
}

// The frames handed to the present thread and not handed back yet, oldest
// first, with the instant its clock counts from.
pub(crate) struct FramesInFlight {
    frames: VecDeque<FrameInFlight>,
    clock_origin: Instant,
}

impl FramesInFlight {
    // The frame `frame_id`, which the present thread handed back. The
    // thread hands frames back in the order it took them.
    fn take(&mut self, frame_id: u64) -> Option<FrameInFlight> {
        while let Some(frame) = self.frames.pop_front() {
            if frame.id == frame_id {
                return Some(frame);
            }
        }
        None
    }

    fn instant(&self, nanos: i64) -> Instant {
        self.clock_origin + Duration::from_nanos(u64::try_from(nanos).unwrap_or_default())
    }

    // When `frame` passed each stage, the present thread's `timings` among
    // them.
    fn presented(&self, frame: &FrameInFlight, timings: PresentTimings) -> PresentedFrame {
        PresentedFrame {
            started_at: frame.started_at,
            after_update: frame.after_update,
            after_acquire: self.instant(timings.after_acquire_ns),
            after_render: self.instant(timings.after_render_ns),
            after_present: self.instant(timings.after_present_ns),
        }
    }
}

// When a frame on the screen passed each stage: its redraw started, its
// update ended, and the present thread had its image, had drawn it and had
// presented it.
#[derive(Clone, Copy)]
#[cfg_attr(
    not(feature = "desktop-shell"),
    expect(dead_code, reason = "the desktop frame telemetry reads the stages")
)]
pub(crate) struct PresentedFrame {
    pub(crate) started_at: Instant,
    pub(crate) after_update: Instant,
    pub(crate) after_acquire: Instant,
    pub(crate) after_render: Instant,
    pub(crate) after_present: Instant,
}

// What the present thread handed back since the loop last looked.
#[derive(Default)]
pub(crate) struct PresentReturns {
    // The newest frame that reached the screen.
    pub(crate) presented: Option<PresentedFrame>,
    // A frame went back undrawn for a reason the next frame fixes: it draws
    // at once.
    pub(crate) redraw: bool,
    // A frame found no image to draw into: the window is occluded or
    // minimized, or the app is off the screen.
    pub(crate) unavailable: bool,
}

// The thread a window's frames present on.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum PresentSite {
    // The loop's thread: the loop configures the surface and draws into it.
    Loop,
    // A thread of the window's renderer, when the device has the cores.
    PresentThread,
}

impl PresentSite {
    pub(crate) fn of_this_device() -> Self {
        if crate::present_thread::presents_on_thread() {
            Self::PresentThread
        } else {
            Self::Loop
        }
    }
}

impl WindowPresent {
    // Starts `renderer` on `target` for `window`, with its frames presenting
    // on `site`. A present thread takes `surface` over and configures it
    // with `config`; it starts with the window's occlusion and wakes the
    // loop through `proxy` each time it hands a frame back. A loop's surface
    // is configured already.
    pub(crate) fn start(
        site: PresentSite,
        renderer: &mut WgpuRenderer,
        target: GpuTarget,
        surface: wgpu::Surface<'static>,
        config: &wgpu::SurfaceConfiguration,
        window: &dyn Window,
        proxy: &EventLoopProxy,
    ) -> Self {
        if site == PresentSite::Loop {
            renderer.init_gpu(target);
            return Self::OnLoop(surface);
        }
        let proxy = proxy.clone();
        let clock_origin = Instant::now();
        let clock =
            Arc::new(move || i64::try_from(clock_origin.elapsed().as_nanos()).unwrap_or(i64::MAX));
        let waker = Arc::new(move || proxy.wake_up());
        if let Err(error) = renderer.init_gpu_threaded(target.clone(), waker, Some(clock), None) {
            log::error!("[present-thread] {error:?}; frames present on the loop's thread");
            surface.configure(&target.device, config);
            renderer.init_gpu(target);
            return Self::OnLoop(surface);
        }
        let occluded = window_occluded(window);
        if let Some(thread) = renderer.present_thread() {
            thread.set_occluded(occluded);
        }
        let config = config.clone();
        if !renderer
            .present_thread()
            .is_some_and(|thread| thread.replace_surface(surface, config))
        {
            log::error!("[present-thread] the present thread took no surface");
        }
        Self::OnPresentThread {
            frames: FramesInFlight {
                frames: VecDeque::new(),
                clock_origin,
            },
            occluded,
        }
    }

    // Configures the surface with `config`; false when `renderer` has no
    // device yet. The present thread takes the configuration without the
    // loop waiting: the frames drawn for the one before go back undrawn.
    pub(crate) fn configure(
        &self,
        renderer: &mut WgpuRenderer,
        config: &wgpu::SurfaceConfiguration,
    ) -> bool {
        let configured = match self {
            Self::OnLoop(surface) => {
                let device = renderer.try_device();
                if let Some(device) = device {
                    surface.configure(device, config);
                }
                device.is_some()
            }
            Self::OnPresentThread { .. } => renderer
                .present_thread()
                .is_some_and(|thread| thread.resize(config.clone())),
        };
        if !configured {
            log::error!("surface configuration skipped: GPU renderer is not initialized");
        }
        configured
    }

    // Starts `renderer` on the new device of `target` and configures the
    // surface with `config` on it.
    pub(crate) fn replace_device(
        &self,
        renderer: &mut WgpuRenderer,
        target: GpuTarget,
        config: &wgpu::SurfaceConfiguration,
    ) {
        match self {
            Self::OnLoop(surface) => {
                surface.configure(&target.device, config);
                renderer.init_gpu(target);
            }
            Self::OnPresentThread { .. } => {
                if let Some(thread) = renderer.present_thread() {
                    thread.replace_device(target);
                }
            }
        }
    }

    // Notes whether an `event` says the window server shows none of the
    // window, and tells the present thread of `renderer`: AppKit answers
    // that on the main thread only.
    pub(crate) fn observe_occlusion(&mut self, renderer: &mut WgpuRenderer, event: &WindowEvent) {
        if let (Self::OnPresentThread { occluded, .. }, WindowEvent::Occluded(now)) = (self, event)
        {
            *occluded = *now;
            if let Some(thread) = renderer.present_thread() {
                thread.set_occluded(*now);
            }
        }
    }

    // Whether the window server shows none of a window whose frames present
    // on a thread of their own: a frame for it would wait unseen, as a loop's
    // surface gives no image then.
    pub(crate) fn occluded(&self) -> bool {
        matches!(self, Self::OnPresentThread { occluded: true, .. })
    }

    // Hands the frame `renderer` holds to the present thread, which draws
    // it into the surface's next image of `config`'s size and presents it.
    // The frame started at `started_at` and its update ended at
    // `after_update`. False when no frame went.
    pub(crate) fn publish(
        &mut self,
        renderer: &mut WgpuRenderer,
        config: &wgpu::SurfaceConfiguration,
        (started_at, after_update): (Instant, Instant),
    ) -> bool {
        let Self::OnPresentThread { frames, .. } = self else {
            return false;
        };
        let outcome = renderer.publish_frame(config.width, config.height);
        if outcome != PublishOutcome::Published {
            log::debug!("[present-thread] no frame published: {outcome:?}");
            return false;
        }
        frames.frames.push_back(FrameInFlight {
            id: renderer.last_published_frame_id(),
            started_at,
            after_update,
        });
        true
    }

    // Folds the frames the present thread handed back into `renderer`,
    // which frees their room, and says what became of them.
    pub(crate) fn drain(&mut self, renderer: &mut WgpuRenderer) -> PresentReturns {
        let mut returns = PresentReturns::default();
        let Self::OnPresentThread { frames, .. } = self else {
            return returns;
        };
        renderer.drain_present_returns_with(&mut |frame_id, outcome, timings| {
            let frame = frames.take(frame_id);
            match outcome {
                PresentOutcome::Presented => {
                    if let Some(frame) = frame {
                        returns.presented = Some(frames.presented(&frame, timings));
                    }
                }
                PresentOutcome::Unchanged => {}
                PresentOutcome::Cancelled(CancelReason::SurfaceUnavailable) => {
                    returns.unavailable = true;
                }
                PresentOutcome::Cancelled(_) | PresentOutcome::NotRun => returns.redraw = true,
            }
        });
        returns
    }
}

// Whether the window server shows no part of `window`. Only macOS reads it
// before the window's first occlusion event.
fn window_occluded(window: &dyn Window) -> bool {
    #[cfg(target_os = "macos")]
    return crate::desktop::macos_window::occluded(window);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = window;
        false
    }
}
