use alloc::{format, string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use parking_lot::RwLock;
use wasm_bindgen::{JsCast, JsValue};

use super::TextureFormatDesc;

/// Whether adapters made from now on ask only for the extensions `glow`
/// uses or that name the adapter; see [`ask_only_for_used_extensions`].
static ONLY_USED_EXTENSIONS: AtomicBool = AtomicBool::new(false);

/// Makes the adapters made from now on ask the browser only for the WebGL
/// extensions `glow` uses or that name the adapter, and report capabilities
/// for those alone: no float render targets, anisotropic filtering,
/// per-target blending, sample shading, compressed or 16-bit normalized
/// formats or multiview. WebGL enables an extension when the page asks for
/// it, and each ask can wait for the browser's GPU process: Chrome on a Mali
/// phone offers 15, and asking for all took 16-24 ms of every start. By
/// default an adapter asks for every extension, as upstream's does.
pub fn ask_only_for_used_extensions() {
    ONLY_USED_EXTENSIONS.store(true, Ordering::Relaxed);
}

/// The extensions an adapter made after [`ask_only_for_used_extensions`]
/// enables: the ones `glow` itself uses or that name the adapter.
fn enables_extension(name: &str) -> bool {
    matches!(
        name,
        "WEBGL_debug_renderer_info"
            | "WEBGL_lose_context"
            | "KHR_parallel_shader_compile"
            | "EXT_disjoint_timer_query_webgl2"
    )
}

/// `glow`'s context for `webgl2_context`. After
/// [`ask_only_for_used_extensions`] only the extensions [`enables_extension`]
/// names are enabled and reported: `glow` asks for every extension it knows
/// as it wraps a context, and while it does, the context's own
/// `getSupportedExtensions` and `getExtension` answer from those extensions
/// alone; then they are deleted and the prototype's answer again. A context
/// that refuses the properties is wrapped as `glow` wraps it.
fn glow_context(webgl2_context: &web_sys::WebGl2RenderingContext) -> glow::Context {
    if !ONLY_USED_EXTENSIONS.load(Ordering::Relaxed) {
        return glow::Context::from_webgl2_context(webgl2_context.clone());
    }
    let names = js_sys::Array::new();
    let enabled = js_sys::Map::new();
    for name in webgl2_context.get_supported_extensions().iter().flat_map(|names| names.iter()) {
        if let Some(text) = name.as_string().filter(|text| enables_extension(text)) {
            if let Ok(Some(extension)) = webgl2_context.get_extension(&text) {
                names.push(&name);
                enabled.set(&name, &extension);
            }
        }
    }
    let context: &js_sys::Object = webgl2_context.as_ref();
    let answers = [
        ("getSupportedExtensions", bound_method(&names, "slice")),
        ("getExtension", bound_method(&enabled, "get")),
    ];
    let shadowed = answers.iter().all(|(property, answer)| {
        answer.as_ref().is_some_and(|answer| {
            js_sys::Reflect::set(context, &JsValue::from_str(property), answer).unwrap_or(false)
        })
    });
    let gl = shadowed.then(|| glow::Context::from_webgl2_context(webgl2_context.clone()));
    for (property, _) in &answers {
        let _ = js_sys::Reflect::delete_property(context, &JsValue::from_str(property));
    }
    gl.unwrap_or_else(|| glow::Context::from_webgl2_context(webgl2_context.clone()))
}

/// `target`'s method `name`, bound to `target`.
fn bound_method(target: &JsValue, name: &str) -> Option<js_sys::Function> {
    js_sys::Reflect::get(target, &JsValue::from_str(name))
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()
        .map(|method| method.bind0(target))
}

/// A wrapper around a [`glow::Context`] to provide a fake `lock()` api that makes it compatible
/// with the `AdapterContext` API from the EGL implementation.
#[derive(Debug)]
pub struct AdapterContext {
    pub glow_context: glow::Context,
    pub webgl2_context: web_sys::WebGl2RenderingContext,
}

impl AdapterContext {
    pub fn is_owned(&self) -> bool {
        false
    }

    /// Obtain a lock to the EGL context and get handle to the [`glow::Context`] that can be used to
    /// do rendering.
    #[track_caller]
    pub fn lock(&self) -> &glow::Context {
        &self.glow_context
    }
}

#[derive(Debug)]
pub struct Instance {
    options: wgt::GlBackendOptions,
}

impl Instance {
    pub fn create_surface_from_canvas(
        &self,
        canvas: web_sys::HtmlCanvasElement,
    ) -> Result<Surface, crate::InstanceError> {
        let result =
            canvas.get_context_with_context_options("webgl2", &Self::create_context_options());
        self.create_surface_from_context(Canvas::Canvas(canvas), result)
    }

    /// A surface on `canvas` draws into its drawing buffer upside down, as
    /// [`Surface`] tells; its owner shows it flipped.
    pub fn create_surface_from_offscreen_canvas(
        &self,
        canvas: web_sys::OffscreenCanvas,
    ) -> Result<Surface, crate::InstanceError> {
        let result =
            canvas.get_context_with_context_options("webgl2", &Self::create_context_options());
        self.create_surface_from_context(Canvas::Offscreen(canvas), result)
    }

    /// Common portion of public `create_surface_from_*` functions.
    ///
    /// Note: Analogous code also exists in the WebGPU backend at
    /// `wgpu::backend::web::Context`.
    fn create_surface_from_context(
        &self,
        canvas: Canvas,
        context_result: Result<Option<js_sys::Object>, JsValue>,
    ) -> Result<Surface, crate::InstanceError> {
        let context_object: js_sys::Object = match context_result {
            Ok(Some(context)) => context,
            Ok(None) => {
                // <https://html.spec.whatwg.org/multipage/canvas.html#dom-canvas-getcontext-dev>
                // A getContext() call “returns null if contextId is not supported, or if the
                // canvas has already been initialized with another context type”. Additionally,
                // “not supported” could include “insufficient GPU resources” or “the GPU process
                // previously crashed”. So, we must return it as an `Err` since it could occur
                // for circumstances outside the application author's control.
                return Err(crate::InstanceError::new(String::from(concat!(
                    "canvas.getContext() returned null; ",
                    "webgl2 not available or canvas already in use"
                ))));
            }
            Err(js_error) => {
                // <https://html.spec.whatwg.org/multipage/canvas.html#dom-canvas-getcontext>
                // A thrown exception indicates misuse of the canvas state.
                return Err(crate::InstanceError::new(format!(
                    "canvas.getContext() threw exception {js_error:?}",
                )));
            }
        };

        // Not returning this error because it is a type error that shouldn't happen unless
        // the browser, JS builtin objects, or wasm bindings are misbehaving somehow.
        let webgl2_context: web_sys::WebGl2RenderingContext = context_object
            .dyn_into()
            .expect("canvas context is not a WebGl2RenderingContext");

        Ok(Surface {
            canvas,
            webgl2_context,
            swapchain: RwLock::new(None),
            presentable: true,
        })
    }

    /// The canvas's own buffers, which frames draw into, need no
    /// multisampling, and depth and stencil for the passes that use them.
    fn create_context_options() -> js_sys::Object {
        let context_options = js_sys::Object::new();
        for (option, value) in [("antialias", false), ("depth", true), ("stencil", true)] {
            js_sys::Reflect::set(&context_options, &option.into(), &JsValue::from_bool(value))
                .expect("Cannot create context options");
        }
        context_options
    }
}

#[cfg(send_sync)]
static_assertions::assert_impl_all!(Instance: Send, Sync);

impl crate::Instance for Instance {
    type A = super::Api;

    unsafe fn init(desc: &crate::InstanceDescriptor<'_>) -> Result<Self, crate::InstanceError> {
        profiling::scope!("Init OpenGL (WebGL) Backend");
        Ok(Instance {
            options: desc.backend_options.gl.clone(),
        })
    }

    unsafe fn enumerate_adapters(
        &self,
        surface_hint: Option<&Surface>,
    ) -> Vec<crate::ExposedAdapter<super::Api>> {
        if let Some(surface_hint) = surface_hint {
            let gl = glow_context(&surface_hint.webgl2_context);

            unsafe {
                super::Adapter::expose(
                    AdapterContext {
                        glow_context: gl,
                        webgl2_context: surface_hint.webgl2_context.clone(),
                    },
                    self.options.clone(),
                )
            }
            .into_iter()
            .collect()
        } else {
            Vec::new()
        }
    }

    unsafe fn create_surface(
        &self,
        _display_handle: raw_window_handle::RawDisplayHandle,
        window_handle: raw_window_handle::RawWindowHandle,
    ) -> Result<Surface, crate::InstanceError> {
        let canvas: web_sys::HtmlCanvasElement = match window_handle {
            raw_window_handle::RawWindowHandle::Web(handle) => web_sys::window()
                .and_then(|win| win.document())
                .expect("Cannot get document")
                .query_selector(&format!("canvas[data-raw-handle=\"{}\"]", handle.id))
                .expect("Cannot query for canvas")
                .expect("Canvas is not found")
                .dyn_into()
                .expect("Failed to downcast to canvas type"),
            raw_window_handle::RawWindowHandle::WebCanvas(handle) => {
                let value: &JsValue = unsafe { handle.obj.cast().as_ref() };
                value.clone().unchecked_into()
            }
            raw_window_handle::RawWindowHandle::WebOffscreenCanvas(handle) => {
                let value: &JsValue = unsafe { handle.obj.cast().as_ref() };
                let canvas: web_sys::OffscreenCanvas = value.clone().unchecked_into();

                return self.create_surface_from_offscreen_canvas(canvas);
            }
            _ => {
                return Err(crate::InstanceError::new(format!(
                    "window handle {window_handle:?} is not a web handle"
                )))
            }
        };

        self.create_surface_from_canvas(canvas)
    }
}

impl super::Adapter {
    /// Creates a new external adapter from an existing WebGL2 rendering context.
    ///
    /// # Safety
    ///
    /// - The underlying WebGL2 context must be valid (not lost).
    /// - The underlying WebGL2 context must be valid when interfacing with any objects returned by
    ///   wgpu-hal from this adapter.
    /// - The underlying WebGL2 context must be valid when dropping this adapter and when
    ///   dropping any objects returned from this adapter.
    pub unsafe fn new_external(
        webgl2_context: web_sys::WebGl2RenderingContext,
        options: wgt::GlBackendOptions,
    ) -> Option<crate::ExposedAdapter<super::Api>> {
        let glow_context = glow_context(&webgl2_context);
        unsafe {
            Self::expose(
                AdapterContext {
                    glow_context,
                    webgl2_context,
                },
                options,
            )
        }
    }

    pub fn adapter_context(&self) -> &AdapterContext {
        &self.shared.context
    }
}

impl super::Device {
    /// Returns the underlying WebGL2 `AdapterContext`.
    pub fn context(&self) -> &AdapterContext {
        &self.shared.context
    }
}

/// A surface draws each frame straight into its canvas's drawing buffer,
/// which the browser shows (Cranpose fork). Upstream's draws into a texture
/// of its own that present copies to the canvas, which reads and writes the
/// whole canvas every frame: on a Mate 20 X (Chrome, WebGL) Cranpose's
/// gauntlet ran its GPU 17-24% fewer cycles a second without that copy and
/// wrote about 700 MB a second less to memory.
///
/// The buffer holds each frame upside down, as every target of this backend
/// does, so the surface flips an `HtmlCanvasElement` with the CSS `scale`
/// property, which the browser's compositor applies as it shows the canvas;
/// the page that shows an `OffscreenCanvas` flips it. A pointer position
/// read from the canvas through `offsetX` and `offsetY` flips with it;
/// `clientX` and `clientY` against the canvas's bounding rect do not. The
/// canvas has depth and stencil buffers, which a pass drawing into it uses in
/// place of its own depth attachment, cleared as that attachment asks; the
/// canvas has one of each, so passes that draw into it share them. The
/// canvas can be copied from and to within a frame, but not sampled, and its
/// one format is `Rgba8Unorm`.
#[derive(Debug)]
pub struct Surface {
    canvas: Canvas,
    pub(super) webgl2_context: web_sys::WebGl2RenderingContext,
    pub(super) swapchain: RwLock<Option<Swapchain>>,
    pub(super) presentable: bool,
}

impl Clone for Surface {
    fn clone(&self) -> Self {
        Self {
            canvas: self.canvas.clone(),
            webgl2_context: self.webgl2_context.clone(),
            swapchain: RwLock::new(self.swapchain.read().clone()),
            presentable: self.presentable,
        }
    }
}

#[cfg(send_sync)]
unsafe impl Sync for Surface {}
#[cfg(send_sync)]
unsafe impl Send for Surface {}

#[derive(Clone, Debug)]
enum Canvas {
    Canvas(web_sys::HtmlCanvasElement),
    Offscreen(web_sys::OffscreenCanvas),
}

#[derive(Clone, Debug)]
pub struct Swapchain {
    pub(crate) extent: wgt::Extent3d,
    pub(super) format: wgt::TextureFormat,
    pub(super) format_desc: TextureFormatDesc,
}

impl Surface {
    /// The frame is in the canvas already.
    pub(super) unsafe fn present(
        &self,
        _suf_texture: super::Texture,
        _context: &AdapterContext,
    ) -> Result<(), crate::SurfaceError> {
        match *self.swapchain.read() {
            Some(_) => Ok(()),
            None => Err(crate::SurfaceError::Other(
                "need to configure surface before presenting",
            )),
        }
    }
}

impl crate::Surface for Surface {
    type A = super::Api;

    unsafe fn configure(
        &self,
        device: &super::Device,
        config: &crate::SurfaceConfiguration,
    ) -> Result<(), crate::SurfaceError> {
        match self.canvas {
            Canvas::Canvas(ref canvas) => {
                canvas.set_width(config.extent.width);
                canvas.set_height(config.extent.height);
                if let Ok(style) = js_sys::Reflect::get(canvas, &JsValue::from_str("style")) {
                    let _ = js_sys::Reflect::set(
                        &style,
                        &JsValue::from_str("scale"),
                        &JsValue::from_str("1 -1"),
                    );
                }
            }
            Canvas::Offscreen(ref canvas) => {
                canvas.set_width(config.extent.width);
                canvas.set_height(config.extent.height);
            }
        }
        *self.swapchain.write() = Some(Swapchain {
            extent: config.extent,
            format: config.format,
            format_desc: device.shared.describe_texture_format(config.format),
        });
        Ok(())
    }

    unsafe fn unconfigure(&self, _device: &super::Device) {
        self.swapchain.write().take();
    }

    unsafe fn acquire_texture(
        &self,
        _timeout_ms: Option<core::time::Duration>, //TODO
        _fence: &super::Fence,
    ) -> Result<crate::AcquiredSurfaceTexture<super::Api>, crate::SurfaceError> {
        let swapchain = self.swapchain.read();
        let sc = swapchain.as_ref().unwrap();
        let texture = super::Texture {
            inner: super::TextureInner::DefaultRenderbuffer,
            drop_guard: None,
            array_layer_count: 1,
            mip_level_count: 1,
            format: sc.format,
            format_desc: sc.format_desc.clone(),
            copy_size: crate::CopyExtent {
                width: sc.extent.width,
                height: sc.extent.height,
                depth: 1,
            },
        };
        Ok(crate::AcquiredSurfaceTexture {
            texture,
            suboptimal: false,
        })
    }

    unsafe fn discard_texture(&self, _texture: super::Texture) {}
}
