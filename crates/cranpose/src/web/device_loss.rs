//! Recovery of the page's GPU device. WebGPU reports a lost device through
//! its `lost` promise; WebGL loses its context with the canvas's
//! `webglcontextlost` event and may get it back with `webglcontextrestored`.
//! After a loss the page draws nothing until the next frame has opened
//! another device on the canvas's surface, configured the surface with it,
//! handed it to the renderer and given the composition its
//! [`GpuContext`](crate::GpuContext). The composition, its state and the
//! input it holds stay.
//!
//! A page opened with `cranpose_lose_gpu_device` in its query loses its
//! device once, [`DRILL_DELAY`] after it started: WebGPU destroys the
//! device and WebGL loses its context through `WEBGL_lose_context`, so the
//! recovery runs as it would after a real loss.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::{DeviceLostHandler, WgpuRenderer};
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::HtmlCanvasElement;

use super::pipeline_discovery::{GpuTarget, PipelineDiscovery};
use crate::{GpuContext, GpuRequirements, gpu_loss::LossSignal};

/// How long the page draws before the drill loses its device.
const DRILL_DELAY: std::time::Duration = std::time::Duration::from_secs(2);

/// How long the page draws before the drill loses its device, in the
/// milliseconds a timeout takes.
const DRILL_DELAY_MS: i32 = DRILL_DELAY.as_millis() as i32;

/// What the page knows about the losses of its devices.
pub(crate) struct WebDeviceLoss {
    signal: RefCell<LossSignal>,
    /// The device is gone and the next one not open yet: no frame draws.
    paused: Cell<bool>,
    /// A recovery is opening the next device.
    recovering: Cell<bool>,
    /// The GPU context the composition reads.
    gpu: Rc<RefCell<GpuContext>>,
    requirements: GpuRequirements,
    backend: Cell<wgpu::Backend>,
    /// The `WEBGL_lose_context` extension that lost the context for the
    /// drill, which gives it back after the loss event: a lost context
    /// hands out no extension.
    restoring: RefCell<Option<JsValue>>,
}

impl WebDeviceLoss {
    pub(crate) fn new(
        gpu: GpuContext,
        requirements: GpuRequirements,
        backend: wgpu::Backend,
    ) -> Rc<Self> {
        Rc::new(Self {
            signal: RefCell::new(LossSignal::default()),
            paused: Cell::new(false),
            recovering: Cell::new(false),
            gpu: Rc::new(RefCell::new(gpu)),
            requirements,
            backend: Cell::new(backend),
            restoring: RefCell::new(None),
        })
    }

    /// What a renderer of the current device runs when that device is lost.
    pub(crate) fn handler(&self) -> DeviceLostHandler {
        self.signal
            .borrow()
            .handler(super::host_surface::wake_frame)
    }

    /// The GPU context for the composition, which a recovery replaces.
    pub(crate) fn share(&self) -> impl Fn() -> GpuContext + 'static {
        let gpu = Rc::clone(&self.gpu);
        move || gpu.borrow().clone()
    }

    /// Whether the page draws no frame: the device is lost and the next one
    /// not open yet.
    pub(crate) fn paused(&self) -> bool {
        self.paused.get()
    }

    /// Listens for the canvas's WebGL context: its loss pauses the page, and
    /// a browser that gives it back starts the recovery.
    pub(crate) fn watch_canvas(
        self: &Rc<Self>,
        canvas: &HtmlCanvasElement,
        recovery: &Rc<Recovery>,
    ) -> Result<(), JsValue> {
        if self.backend.get() != wgpu::Backend::Gl {
            return Ok(());
        }
        let (loss, app) = (Rc::clone(self), Rc::downgrade(&recovery.app));
        let lost = Closure::wrap(Box::new(move |event: web_sys::Event| {
            // Without this the browser never gives the context back.
            event.prevent_default();
            log::error!("[gpu-device] the WebGL context was lost");
            loss.paused.set(true);
            // Freed now, the renderer's objects go with the lost context;
            // freed after the restore, WebGL would warn that they are not
            // the new context's.
            if let Some(mut app) = app
                .upgrade()
                .as_ref()
                .and_then(|app| app.try_borrow_mut().ok())
            {
                app.renderer().release_gpu();
            }
            if let Some(extension) = loss.restoring.borrow_mut().take() {
                restore_after_event(extension);
            }
        }) as Box<dyn FnMut(_)>);
        canvas
            .add_event_listener_with_callback("webglcontextlost", lost.as_ref().unchecked_ref())?;
        lost.forget();
        let loss = Rc::clone(self);
        let restored = Closure::wrap(Box::new(move |_: web_sys::Event| {
            log::warn!("[gpu-device] the WebGL context is back");
            loss.signal.borrow().report_loss();
            super::host_surface::wake_frame();
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback(
            "webglcontextrestored",
            restored.as_ref().unchecked_ref(),
        )?;
        restored.forget();
        Ok(())
    }

    /// A frame reached the canvas, so the device works.
    pub(crate) fn presented(&self) {
        self.signal.borrow_mut().presented();
    }

    /// Loses the device [`DRILL_DELAY`] from now when the page's query asks
    /// for `cranpose_lose_gpu_device`.
    pub(crate) fn schedule_drill(
        self: &Rc<Self>,
        page: &web_sys::Window,
        canvas: &HtmlCanvasElement,
    ) {
        let asked = page
            .location()
            .search()
            .is_ok_and(|query| query.contains("cranpose_lose_gpu_device"));
        if !asked {
            return;
        }
        let (loss, canvas) = (Rc::clone(self), canvas.clone());
        let drill = Closure::once_into_js(move || loss.lose_for_drill(&canvas));
        if page
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                drill.unchecked_ref(),
                DRILL_DELAY_MS,
            )
            .is_err()
        {
            log::warn!("[gpu-device] the device loss drill could not be scheduled");
        }
    }

    fn lose_for_drill(&self, canvas: &HtmlCanvasElement) {
        log::warn!("[gpu-device] losing the device, as cranpose_lose_gpu_device asks");
        if self.backend.get() == wgpu::Backend::Gl {
            // The context's loss event asks for it back.
            match lose_context_extension(canvas) {
                Some(extension) => {
                    call(&extension, "loseContext");
                    *self.restoring.borrow_mut() = Some(extension);
                }
                None => log::warn!("[gpu-device] the canvas has no WEBGL_lose_context"),
            }
        } else {
            self.paused.set(true);
            self.gpu.borrow().device.destroy();
        }
    }

    /// Starts replacing a lost device, from the frame loop. The page draws
    /// again once the next device is open. After
    /// [`crate::gpu_loss::MAX_RECOVERIES`] recoveries in a row without a
    /// frame, it stops trying.
    pub(crate) fn recover(self: &Rc<Self>, recovery: &Rc<Recovery>) {
        if self.recovering.get() || !self.signal.borrow().take_loss() {
            return;
        }
        self.paused.set(true);
        if !self.signal.borrow_mut().begin_recovery() {
            log::error!("[gpu-device] the device keeps getting lost without a frame; giving up");
            return;
        }
        self.recovering.set(true);
        let (loss, recovery) = (Rc::clone(self), Rc::clone(recovery));
        wasm_bindgen_futures::spawn_local(async move {
            let instance = loss.gpu.borrow().instance.clone();
            match super::open_device(&instance, &recovery.surface, loss.requirements).await {
                Ok((adapter, device, queue)) => loss.hand_over(&recovery, adapter, device, queue),
                Err(error) => {
                    log::error!("[gpu-device] no new device after a loss: {error}");
                    loss.signal.borrow().report_loss();
                }
            }
            loss.recovering.set(false);
            (recovery.request_frame)();
        });
    }

    fn hand_over(
        &self,
        recovery: &Recovery,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) {
        let backend = adapter.get_info().backend;
        let downlevel = adapter.get_downlevel_capabilities().flags;
        let (device, queue) = (Arc::new(device), Arc::new(queue));
        let config = recovery.config.borrow();
        let view_format = crate::surface_format::display_surface_view_format(config.format);
        recovery.surface.configure(&device, &config);
        recovery.discovery.recovered(GpuTarget {
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
            surface_format: view_format,
            backend,
            downlevel,
            surface_size: (config.width, config.height),
        });
        drop(config);
        {
            let mut app = recovery.app.borrow_mut();
            let renderer = app.renderer();
            renderer.set_device_lost_handler(self.handler());
            renderer.init_gpu(cranpose_render_wgpu::GpuTarget {
                device: Arc::clone(&device),
                queue: Arc::clone(&queue),
                surface_format: view_format,
                adapter_backend: backend,
                adapter_downlevel: downlevel,
            });
            app.request_root_render();
        }
        recovery.surface_dirty.set(true);
        let mut gpu = self.gpu.borrow_mut();
        gpu.adapter = adapter;
        gpu.device = device;
        gpu.queue = queue;
        self.backend.set(backend);
        self.paused.set(false);
        log::warn!("[gpu-device] opened a new device after the last one was lost");
    }
}

/// What a recovery hands the next device to.
pub(crate) struct Recovery {
    pub(crate) app: Rc<RefCell<AppShell<WgpuRenderer>>>,
    pub(crate) surface: Rc<wgpu::Surface<'static>>,
    pub(crate) config: Rc<RefCell<wgpu::SurfaceConfiguration>>,
    pub(crate) surface_dirty: Rc<Cell<bool>>,
    pub(crate) request_frame: Rc<dyn Fn()>,
    pub(crate) discovery: Rc<PipelineDiscovery>,
}

/// Gives the canvas its WebGL context back through `extension` once the loss
/// event finished, as the browser would after a driver reset.
fn restore_after_event(extension: JsValue) {
    let restore = Closure::once_into_js(move || call(&extension, "restoreContext"));
    let scheduled = web_sys::window().and_then(|window| {
        window
            .set_timeout_with_callback_and_timeout_and_arguments_0(restore.unchecked_ref(), 0)
            .ok()
    });
    if scheduled.is_none() {
        log::warn!("[gpu-device] the WebGL context's restore could not be scheduled");
    }
}

/// The canvas's `WEBGL_lose_context` extension, while its context lives.
fn lose_context_extension(canvas: &HtmlCanvasElement) -> Option<JsValue> {
    let context = canvas.get_context("webgl2").ok().flatten()?;
    let get = js_sys::Reflect::get(&context, &"getExtension".into()).ok()?;
    get.dyn_ref::<js_sys::Function>()?
        .call1(&context, &"WEBGL_lose_context".into())
        .ok()
        .filter(|extension| !extension.is_null() && !extension.is_undefined())
}

/// Calls `method` of a `WEBGL_lose_context` extension.
fn call(extension: &JsValue, method: &str) {
    let called = js_sys::Reflect::get(extension, &method.into())
        .ok()
        .and_then(|method| method.dyn_ref::<js_sys::Function>()?.call0(extension).ok());
    if called.is_none() {
        log::warn!("[gpu-device] WEBGL_lose_context could not {method}");
    }
}
