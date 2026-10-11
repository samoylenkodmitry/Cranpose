//! The first frame on WebGPU, drawn twice: once to learn the render
//! pipelines it needs while the browser compiles them all at once, then for
//! real.
//!
//! Created one after another, the twelve pipelines of the gauntlet's first
//! frame held Chrome's GPU process for 0.85 to 0.96 s on an Apple M5 with
//! no Metal shader cached, and nothing showed meanwhile. Through
//! `createRenderPipelineAsync` the same twelve compiled side by side in
//! 0.21 to 0.25 s, and creating them again afterwards took 3 ms in all.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use cranpose_render_wgpu::WgpuRenderer;

/// What the renderer draws with.
pub(crate) struct GpuTarget {
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
    pub(crate) surface_format: wgpu::TextureFormat,
    pub(crate) backend: wgpu::Backend,
    pub(crate) downlevel: wgpu::DownlevelFlags,
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
}

impl PipelineDiscovery {
    pub(crate) fn new(target: GpuTarget) -> Self {
        Self {
            target,
            stage: Rc::new(RefCell::new(Stage::Cold)),
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
        let mut stage = self.stage.borrow_mut();
        *stage = match std::mem::replace(&mut *stage, Stage::Done) {
            Stage::Cold if self.target.device.begin_pipeline_discovery() => Stage::Discovering,
            Stage::Landed(compiled) => {
                self.install(renderer);
                Stage::Redrawing {
                    _compiled: compiled,
                }
            }
            Stage::Cold | Stage::Redrawing { .. } | Stage::Done => Stage::Done,
            current @ (Stage::Discovering | Stage::Compiling) => current,
        };
        if matches!(*stage, Stage::Discovering | Stage::Redrawing { .. }) {
            renderer.begin_discovery_frame();
        }
    }

    /// Ends the frame `renderer` drew: after discovery, waits for the browser
    /// to compile what it learned and calls `redraw` once it has.
    pub(crate) fn end_frame(&self, renderer: &WgpuRenderer, redraw: &Rc<dyn Fn()>) {
        let mut stage = self.stage.borrow_mut();
        match &*stage {
            Stage::Discovering => {
                renderer.end_discovery_frame();
                *stage = Stage::Compiling;
                let compiling = self.target.device.end_pipeline_discovery();
                let landed = Rc::clone(&self.stage);
                let redraw = Rc::clone(redraw);
                wasm_bindgen_futures::spawn_local(async move {
                    let compiled = compiling.await;
                    *landed.borrow_mut() = Stage::Landed(compiled);
                    redraw();
                });
            }
            Stage::Redrawing { .. } => {
                renderer.end_discovery_frame();
                *stage = Stage::Done;
            }
            Stage::Cold | Stage::Compiling | Stage::Landed(_) | Stage::Done => {}
        }
    }
}
