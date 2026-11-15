// A WebGPU page creates every render pipeline through
// `createRenderPipelineAsync` (`wgpu::Device::compile_pipelines_in_background`).
// Created one after another through `createRenderPipeline`, the twelve
// pipelines of the gauntlet's first frame held Chrome's GPU process for 0.85
// to 0.96 s on an Apple M5 with no Metal shader cached. Through
// `createRenderPipelineAsync` the same twelve compiled side by side in 0.21
// to 0.25 s. A frame leaves out the draws of a pipeline that has not
// compiled yet. The page draws again once that pipeline landed.
//
// The first screen's pipelines, recorded by an earlier run of the app (see
// `FIRST_SCREEN_PIPELINES`), start to compile as the renderer starts: of the
// sets recorded on this backend, the one drawn at the canvas size nearest
// this one's. On WebGL Chrome's GPU process links each program one at a
// time, about 40 ms on a Mate 20 X. It would otherwise sit idle while the
// page composes, then link all thirteen of the gauntlet's while its first
// frame waits: that frame ended at 1.11-1.21 s with them built at start,
// 1.45-1.57 s without. On WebGPU the browser compiles them while the page
// composes.
//
// The first frame shows only once it drew whole. Chrome hands the page a
// pipeline compiled during the first composition only after the page sent
// its GPU process a call. While pipelines compile, the first frame sends
// one and waits for the answer: without it the frame right after a long
// first composition finds none of them. On the still wall at tier 20 in
// Chrome on an Apple M3 Pro, a first frame drawn without the wait left out
// pipelines that had compiled during the first composition. The page then
// drew it a second time, 100 ms later. With the wait the answer came 0.5 to
// 2.5 ms after the call. The first frame then drew whole.
//
// A first frame that left out a draw clears its image to the color a frame
// clears to before it draws, which the canvas showed before. No frame draws
// until the left-out pipelines landed. The frame drawn then shows at once,
// without waiting for the next animation frame and the update that runs
// first.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_render_wgpu::WgpuRenderer;

// The pipelines the app's first screen drew with in a recording run of an
// earlier build of the same app (`__cranposeRecordFirstScreen`). The build
// embeds them from the file named by `CRANPOSE_FIRST_SCREEN_PIPELINES`. A
// build that names no file embeds none.
const FIRST_SCREEN_PIPELINES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/first_screen_pipelines.bin"));

// The global a recording run sets before the page loads, and the globals
// the page then leaves its first screen's pipelines and WebGL stages in.
const RECORD_REQUEST: &str = "__cranposeRecordFirstScreen";
const RECORDED: &str = "__cranposeFirstScreenPipelines";
#[cfg(not(cranpose_first_screen_glsl))]
const RECORDED_STAGES: &str = "__cranposeFirstScreenStages";

// What the renderer draws with.
pub(crate) struct GpuTarget {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) backend: wgpu::Backend,
    pub(crate) downlevel: wgpu::DownlevelFlags,
    // The size of the canvas's drawing buffer, in pixels.
    pub(crate) surface_size: (u32, u32),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    // No frame drew yet.
    Cold,
    // The first frame waits until the page holds the pipelines compiled so
    // far.
    Yielding,
    // No frame showed whole yet: a frame that leaves out a draw shows none
    // of itself.
    Unshown,
    // The last frame showed none of itself: no frame draws until the
    // pipelines left out of it landed.
    Waiting,
    // A frame showed whole.
    Shown,
}

// Builds the first screen's recorded pipelines as the page starts. Keeps the
// first frame from showing until it drew whole. Draws again once the
// pipelines left out of a frame landed.
pub(crate) struct PipelineLanding {
    // The device the pipelines are for; a device recovery replaces it.
    target: RefCell<GpuTarget>,
    stage: Rc<RefCell<Stage>>,
    // Whether this is a recording run whose first screen is still to be
    // left in `RECORDED`.
    recording: std::cell::Cell<bool>,
    // Whether pipelines left out of a frame landed since the last frame.
    landed: Rc<std::cell::Cell<bool>>,
}

impl PipelineLanding {
    pub(crate) fn new(target: GpuTarget) -> Self {
        let recording = js_sys::Reflect::get(&js_sys::global(), &RECORD_REQUEST.into())
            .is_ok_and(|requested| requested.is_truthy());
        // A recording run opens the build without records, so a build with
        // them carries no listing (`first_screen_glsl`).
        #[cfg(not(cranpose_first_screen_glsl))]
        if recording {
            cranpose_render_wgpu::list_first_screen_stages();
        }
        Self {
            target: RefCell::new(target),
            stage: Rc::new(RefCell::new(Stage::Shown)),
            recording: std::cell::Cell::new(recording),
            landed: Rc::default(),
        }
    }

    // Installs the first renderer and builds the recorded first screen's
    // pipelines, which the browser compiles beside the page's start.
    pub(crate) async fn start(&self, renderer: &mut WgpuRenderer) {
        self.install(renderer);
        let surface_size = self.target.borrow().surface_size;
        if self.target.borrow().backend == wgpu::Backend::Gl {
            #[cfg(cranpose_first_screen_glsl)]
            super::first_screen_glsl::take().await;
            renderer.build_first_screen_pipelines(FIRST_SCREEN_PIPELINES, surface_size);
            return;
        }
        if !self
            .target
            .borrow()
            .device
            .compile_pipelines_in_background()
        {
            return;
        }
        *self.stage.borrow_mut() = Stage::Cold;
        renderer.begin_first_frame();
        if FIRST_SCREEN_PIPELINES.is_empty() {
            return;
        }
        renderer.build_first_screen_pipelines(FIRST_SCREEN_PIPELINES, surface_size);
        // Chrome sends WebGPU calls to its GPU process at a microtask
        // checkpoint: yielding once lets it compile them while the page
        // composes its first screen.
        let yielded = js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED);
        let _ = wasm_bindgen_futures::JsFuture::from(yielded).await;
    }

    // Installs a new GPU renderer on the target.
    pub(crate) fn install(&self, renderer: &mut WgpuRenderer) {
        let target = self.target.borrow();
        renderer.init_gpu(cranpose_render_wgpu::GpuTarget {
            device: Arc::clone(&target.device),
            queue: Arc::clone(&target.queue),
            surface_format: target.surface_format,
            adapter_backend: target.backend,
            adapter_downlevel: target.downlevel,
        });
    }

    // Hands the landing the device that replaced a lost one. The first
    // screen is behind it then.
    pub(crate) fn recovered(&self, target: GpuTarget) {
        *self.target.borrow_mut() = target;
        *self.stage.borrow_mut() = Stage::Shown;
    }

    // Whether a frame may draw: not while the first frame waits for the
    // pipelines compiled so far, nor while a frame that showed none of
    // itself waits for the pipelines left out of it.
    pub(crate) fn may_draw(&self) -> bool {
        !matches!(*self.stage.borrow(), Stage::Yielding | Stage::Waiting)
    }

    // Whether the first frame, about to draw while pipelines compile, waits.
    // It waits until the page holds the pipelines compiled so far, then
    // draws through `draw_now`.
    pub(crate) fn yields_first_frame(&self, draw_now: &Rc<dyn Fn()>) -> bool {
        let mut stage = self.stage.borrow_mut();
        if *stage != Stage::Cold {
            return false;
        }
        let Some(delivered) = self.target.borrow().device.deliver_background_pipelines() else {
            *stage = Stage::Unshown;
            return false;
        };
        *stage = Stage::Yielding;
        let waiting = Rc::clone(&self.stage);
        let draw_now = Rc::clone(draw_now);
        wasm_bindgen_futures::spawn_local(async move {
            delivered.await;
            *waiting.borrow_mut() = Stage::Unshown;
            draw_now();
        });
        true
    }

    // Prepares the frame about to draw with `renderer`. A frame drawn before
    // one showed whole builds the pipelines a later frame needs after its
    // own. The frames after it build the specialized pipelines that their
    // draws stood in for.
    pub(crate) fn begin_frame(&self, renderer: &mut WgpuRenderer) {
        self.leave_recorded(renderer);
        match *self.stage.borrow() {
            Stage::Cold | Stage::Unshown => renderer.begin_first_frame(),
            Stage::Shown => renderer.compile_pipelines_in_background(),
            Stage::Yielding | Stage::Waiting => {}
        }
        if self.landed.take() {
            renderer.background_pipelines_landed();
        }
    }

    // In a recording run, leaves the first screen's pipelines in `RECORDED`
    // and its WebGL stages in `RECORDED_STAGES` once the first screen ended.
    fn leave_recorded(&self, renderer: &WgpuRenderer) {
        if !self.recording.get() {
            return;
        }
        let Some(records) = renderer.first_screen_pipelines() else {
            return;
        };
        self.recording.set(false);
        let global = js_sys::global();
        // Left before the pipelines, which the recording run waits for.
        #[cfg(not(cranpose_first_screen_glsl))]
        let _ = js_sys::Reflect::set(
            &global,
            &RECORDED_STAGES.into(),
            &cranpose_render_wgpu::first_screen_stages().into(),
        );
        let records = js_sys::Uint8Array::from(records.as_slice());
        if js_sys::Reflect::set(&global, &RECORDED.into(), &records).is_err() {
            log::warn!(
                "[pipeline-records] the first screen's pipelines could not be left in {RECORDED}"
            );
        }
    }

    // Ends the frame just drawn, before its image presents. Returns whether
    // the frame shows none of itself: a frame that left out draws before a
    // frame showed whole. Once the left-out pipelines landed, the page draws
    // again: at once through `draw_now` after a frame that showed none of
    // itself, else through `redraw`. A recording run asks `redraw` for
    // frames until it left its first screen: a screen that holds still would
    // draw no frame after the first screen ended.
    pub(crate) fn end_frame(&self, redraw: &Rc<dyn Fn()>, draw_now: &Rc<dyn Fn()>) -> bool {
        if self.recording.get() {
            // Asked after this frame: the browser drops a frame asked during one.
            let redraw = Rc::clone(redraw);
            wasm_bindgen_futures::spawn_local(async move { redraw() });
        }
        let compiling = self.target.borrow().device.take_background_pipelines();
        let mut stage = self.stage.borrow_mut();
        let unshown = matches!(*stage, Stage::Cold | Stage::Unshown);
        let hides = unshown && compiling.is_some();
        if unshown {
            *stage = if hides { Stage::Waiting } else { Stage::Shown };
        }
        if let Some(compiling) = compiling {
            let landed = Rc::clone(&self.landed);
            let waiting = Rc::clone(&self.stage);
            let again = Rc::clone(if hides { draw_now } else { redraw });
            wasm_bindgen_futures::spawn_local(async move {
                compiling.await;
                landed.set(true);
                if hides {
                    *waiting.borrow_mut() = Stage::Unshown;
                }
                again();
            });
        }
        hides
    }
}
