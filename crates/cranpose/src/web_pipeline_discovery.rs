//! The first frame on WebGPU, drawn twice: once to learn the render
//! pipelines it needs while the browser compiles them all at once, then for
//! real.
//!
//! Created one after another, the twelve pipelines of the gauntlet's first
//! frame held Chrome's GPU process for 0.85 to 0.96 s on an Apple M5 with
//! no Metal shader cached, and nothing showed meanwhile. Through
//! `createRenderPipelineAsync` the same twelve compiled side by side in
//! 0.21 to 0.25 s, and creating them again afterwards took 3 ms in all.
//!
//! Every frame after it creates the pipelines it lacks in the background
//! (`wgpu::Device::compile_pipelines_in_background`): drawn one after
//! another, the 19 pipelines the demo's Liquid UI tab first needed held
//! Chrome's GPU process, and the page, 1.4 s on an Apple M5 with no Metal
//! shader cached. The frame leaves out the draws that wait for one, and the
//! page draws again when they landed.
//!
//! The first screen's pipelines an earlier run of the app recorded (see
//! [`FIRST_SCREEN_PIPELINES`]) build as the renderer starts: of the sets
//! recorded on this backend, the one drawn at the canvas size nearest this
//! one's. The first frame waits for every pipeline that starts, so a set
//! another first screen drew holds it back: the gauntlet's set recorded at
//! its default load, opened at a higher one, put its first frame in Chrome
//! on an Apple M3 Pro at 1.34 s against 0.81 s with no set. On WebGL
//! Chrome's GPU process links each program one at a time, about 40 ms on a
//! Mate 20 X, and would otherwise sit idle while the page composes, then
//! link all thirteen of the gauntlet's while its first frame waits: that
//! frame ended at 1.11-1.21 s with them built at start, 1.45-1.57 s
//! without. On WebGPU they begin the first frame's discovery, so Chrome
//! compiles them while the page composes: on an Apple M5 with no Metal
//! shader cached the gauntlet's first content came at 0.57-0.66 s with a
//! set that matched its first frame, 0.71-0.85 s without one.
//!
//! The frame a discovery learned its pipelines from draws as soon as they
//! compiled, without waiting for the next animation frame and the update
//! that runs first: with Metal's shader cache warm the gauntlet's first
//! content came at 0.30-0.34 s, 0.33-0.39 s when it waited.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_render_wgpu::WgpuRenderer;

/// The pipelines the app's first screen drew with in a recording run of an
/// earlier build of the same app (`__cranposeRecordFirstScreen`), which the
/// build embeds from the file `CRANPOSE_FIRST_SCREEN_PIPELINES` names;
/// empty for a build that names none.
const FIRST_SCREEN_PIPELINES: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/first_screen_pipelines.bin"));

/// The global a recording run sets before the page loads, and the global
/// the page then leaves its first screen's pipelines in.
const RECORD_REQUEST: &str = "__cranposeRecordFirstScreen";
const RECORDED: &str = "__cranposeFirstScreenPipelines";

/// What the renderer draws with.
pub(crate) struct GpuTarget {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) backend: wgpu::Backend,
    pub(crate) downlevel: wgpu::DownlevelFlags,
    /// The size the canvas's drawing buffer was given, in pixels.
    pub(crate) surface_size: (u32, u32),
}

enum Stage {
    /// No frame drew yet.
    Cold,
    /// The first frame is drawing to learn its pipelines.
    Discovering,
    /// The browser compiles the pipelines the first frame created.
    Compiling,
    /// They compiled: the next frame installs a new renderer and draws.
    Landed(Option<wgpu::webgpu::DiscoveredPipelines>),
    /// The frame that creates them again is drawing; held, they stay cached
    /// until it is done.
    Redrawing {
        _compiled: Option<wgpu::webgpu::DiscoveredPipelines>,
    },
    Done,
}

/// Runs the first frame's pipeline discovery and installs the renderer.
pub(crate) struct PipelineDiscovery {
    target: GpuTarget,
    stage: Rc<RefCell<Stage>>,
    /// Whether this is a recording run whose first screen is still to be
    /// left in [`RECORDED`].
    recording: std::cell::Cell<bool>,
    /// Whether pipelines compiled in the background landed since the last
    /// frame, which drew without them.
    landed: Rc<std::cell::Cell<bool>>,
}

impl PipelineDiscovery {
    pub(crate) fn new(target: GpuTarget) -> Self {
        let recording = js_sys::Reflect::get(&js_sys::global(), &RECORD_REQUEST.into())
            .is_ok_and(|requested| requested.is_truthy());
        Self {
            target,
            stage: Rc::new(RefCell::new(Stage::Cold)),
            recording: std::cell::Cell::new(recording),
            landed: Rc::default(),
        }
    }

    /// Installs the first renderer and builds the recorded first screen's
    /// pipelines, which the browser compiles beside the page's start.
    pub(crate) async fn start(&self, renderer: &mut WgpuRenderer) {
        self.install(renderer);
        if self.target.backend == wgpu::Backend::Gl {
            renderer.build_first_screen_pipelines(FIRST_SCREEN_PIPELINES, self.target.surface_size);
        } else if !FIRST_SCREEN_PIPELINES.is_empty()
            && self.target.device.begin_pipeline_discovery()
        {
            *self.stage.borrow_mut() = Stage::Discovering;
            renderer.begin_discovery_frame();
            renderer.build_first_screen_pipelines(FIRST_SCREEN_PIPELINES, self.target.surface_size);
            // Chrome sends WebGPU calls to its GPU process at a microtask
            // checkpoint: yielding once lets it compile them while the page
            // composes its first screen.
            let yielded = js_sys::Promise::resolve(&wasm_bindgen::JsValue::UNDEFINED);
            let _ = wasm_bindgen_futures::JsFuture::from(yielded).await;
        }
    }

    /// Installs a new GPU renderer on the target: at start, and after
    /// discovery, whose frame left the renderer's caches holding what it did
    /// not draw.
    pub(crate) fn install(&self, renderer: &mut WgpuRenderer) {
        renderer.init_gpu(
            Arc::clone(&self.target.device),
            Arc::clone(&self.target.queue),
            self.target.surface_format,
            self.target.backend,
            self.target.downlevel,
        );
    }

    /// Whether a frame may draw: not while the discovered pipelines compile.
    pub(crate) fn may_draw(&self) -> bool {
        !matches!(*self.stage.borrow(), Stage::Compiling)
    }

    /// Prepares the frame about to draw with `renderer`: the first frame on
    /// WebGPU discovers its pipelines, and the frame after they compiled
    /// draws with a new renderer. Both also build what a later frame needs.
    pub(crate) fn begin_frame(&self, renderer: &mut WgpuRenderer) {
        self.leave_recorded(renderer);
        let mut stage = self.stage.borrow_mut();
        *stage = match std::mem::replace(&mut *stage, Stage::Done) {
            Stage::Cold if self.target.device.begin_pipeline_discovery() => Stage::Discovering,
            Stage::Landed(compiled) => {
                self.install(renderer);
                Stage::Redrawing {
                    _compiled: compiled,
                }
            }
            Stage::Cold => {
                self.target.device.compile_pipelines_in_background();
                Stage::Done
            }
            Stage::Redrawing { .. } | Stage::Done => Stage::Done,
            current @ (Stage::Discovering | Stage::Compiling) => current,
        };
        if matches!(*stage, Stage::Discovering | Stage::Redrawing { .. }) {
            renderer.begin_discovery_frame();
        } else if matches!(*stage, Stage::Done) {
            renderer.compile_pipelines_in_background();
        }
        if self.landed.take() {
            renderer.background_pipelines_landed();
        }
    }

    /// In a recording run, leaves the first screen's pipelines in
    /// [`RECORDED`] once the first screen ended.
    fn leave_recorded(&self, renderer: &WgpuRenderer) {
        if !self.recording.get() {
            return;
        }
        let Some(records) = renderer.first_screen_pipelines() else {
            return;
        };
        self.recording.set(false);
        let records = js_sys::Uint8Array::from(records.as_slice());
        if js_sys::Reflect::set(&js_sys::global(), &RECORDED.into(), &records).is_err() {
            log::warn!(
                "[pipeline-records] the first screen's pipelines could not be left in {RECORDED}"
            );
        }
    }

    /// Ends the frame that drew: after discovery, waits for the browser to
    /// compile what it learned and calls `draw_now` once it has. A
    /// recording run asks `redraw` for frames until its first screen is
    /// left: a screen that holds still would draw no frame after the first
    /// screen ended.
    pub(crate) fn end_frame(&self, redraw: &Rc<dyn Fn()>, draw_now: &Rc<dyn Fn()>) {
        if self.recording.get() {
            // Asked after this frame: a frame asked during one is dropped.
            let redraw = Rc::clone(redraw);
            wasm_bindgen_futures::spawn_local(async move { redraw() });
        }
        let mut stage = self.stage.borrow_mut();
        match &*stage {
            Stage::Discovering => {
                *stage = Stage::Compiling;
                let compiling = self.target.device.end_pipeline_discovery();
                let landed = Rc::clone(&self.stage);
                let draw_now = Rc::clone(draw_now);
                wasm_bindgen_futures::spawn_local(async move {
                    let compiled = compiling.await;
                    *landed.borrow_mut() = Stage::Landed(compiled);
                    draw_now();
                });
            }
            Stage::Redrawing { .. } => {
                *stage = Stage::Done;
                self.target.device.compile_pipelines_in_background();
            }
            Stage::Done => {
                if let Some(compiling) = self.target.device.take_background_pipelines() {
                    let landed = Rc::clone(&self.landed);
                    let redraw = Rc::clone(redraw);
                    wasm_bindgen_futures::spawn_local(async move {
                        compiling.await;
                        landed.set(true);
                        redraw();
                    });
                }
            }
            Stage::Cold | Stage::Compiling | Stage::Landed(_) => {}
        }
    }
}
