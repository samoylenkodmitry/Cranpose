//! Web runtime for Compose applications.
//!
//! This module provides the web event loop implementation using wasm-bindgen and WebGPU.

use crate::{
    launcher::AppSettings,
    wgpu_surface::{current_surface_texture, surface_present_required, SurfaceFrame},
};
use cranpose_app_shell::{default_root_key, AppShell, PlatformFrameDriver, PointerSource};
use cranpose_platform_web::WebPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, PointerEvent, WheelEvent};

/// Maps a DOM `PointerEvent.pointerType` string onto the framework
/// [`PointerSource`] so text fields show finger handles only for touch.
fn web_pointer_source(event: &PointerEvent) -> PointerSource {
    match event.pointer_type().as_str() {
        "touch" => PointerSource::Touch,
        "pen" => PointerSource::Stylus,
        "mouse" => PointerSource::Mouse,
        _ => PointerSource::Unknown,
    }
}

const WEB_WHEEL_LINE_DELTA_PIXELS: f32 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WebBackendPreference {
    Auto,
    WebGpu,
    Gl,
}

#[derive(Debug)]
struct BrowserDisplayHandle;

impl wgpu::rwh::HasDisplayHandle for BrowserDisplayHandle {
    fn display_handle(&self) -> Result<wgpu::rwh::DisplayHandle<'_>, wgpu::rwh::HandleError> {
        Ok(wgpu::rwh::DisplayHandle::web())
    }
}

#[derive(Default)]
struct WebFrameTimer {
    generation: Cell<u64>,
    pending: Cell<bool>,
}

struct WebPlatformFrameDriver<'a> {
    frame_timer: &'a Rc<WebFrameTimer>,
    frame_pending: &'a Rc<Cell<bool>>,
    render_loop: &'a Rc<RefCell<Option<Closure<dyn FnMut()>>>>,
}

impl PlatformFrameDriver for WebPlatformFrameDriver<'_> {
    fn request_frame(&self) {
        request_web_frame(self.frame_pending, self.render_loop, Some(self.frame_timer));
    }

    fn request_wake_at(&self, deadline: web_time::Instant) {
        request_web_frame_at_deadline(
            self.frame_timer,
            deadline,
            self.frame_pending,
            self.render_loop,
        );
    }

    fn clear_wake(&self) {
        clear_web_frame_wake(self.frame_timer);
    }
}

/// Runs a web Compose application with wgpu rendering.
///
/// Called by `AppLauncher::run_web()`. This is the framework-level
/// entrypoint that manages the web canvas and rendering.
///
/// **Note:** Applications should use `AppLauncher` instead of calling this directly.
pub async fn run(
    canvas_id: &str,
    settings: AppSettings,
    content: impl FnMut() + 'static,
) -> Result<(), JsValue> {
    // Set up console logging
    console_error_panic_hook::set_once();

    // Get the window and document
    let window = web_sys::window().ok_or("no global window exists")?;
    let document = window
        .document()
        .ok_or("should have a document on window")?;

    // Get the canvas element
    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| format!("canvas with id '{}' not found", canvas_id))?
        .dyn_into::<HtmlCanvasElement>()?;

    // Get device pixel ratio for proper scaling
    let scale_factor = window.device_pixel_ratio();

    // Set canvas size
    let width = settings.initial_width;
    let height = settings.initial_height;

    canvas.set_width((width as f64 * scale_factor) as u32);
    canvas.set_height((height as f64 * scale_factor) as u32);

    // Set CSS size and disable browser touch gesture handling on the canvas
    if let Some(html_element) = canvas.dyn_ref::<web_sys::HtmlElement>() {
        let style = html_element.style();
        style.set_property("width", &format!("{}px", width))?;
        style.set_property("height", &format!("{}px", height))?;
        style.set_property("touch-action", "none")?;
    }

    let backend_preference = requested_web_backend(&window);
    let mut instance_desc =
        wgpu::InstanceDescriptor::new_with_display_handle(Box::new(BrowserDisplayHandle));
    instance_desc.backends = instance_backends(backend_preference);
    let instance = match backend_preference {
        WebBackendPreference::Auto => {
            wgpu::util::new_instance_with_webgpu_detection(instance_desc).await
        }
        WebBackendPreference::WebGpu | WebBackendPreference::Gl => {
            wgpu::Instance::new(instance_desc)
        }
    };

    let surface = instance
        .create_surface(wgpu::SurfaceTarget::Canvas(canvas.clone()))
        .map_err(|e| format!("failed to create surface: {:?}", e))?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .map_err(|e| format!("failed to find suitable adapter: {:?}", e))?;

    let adapter_info = adapter.get_info();
    let adapter_limits = adapter.limits();
    let required_limits =
        required_limits_for_web_backend(adapter_info.backend, adapter_limits.clone());
    log::info!(
        "Web backend preference={:?}, selected backend={:?}, max_texture_dimension_2d={}",
        backend_preference,
        adapter_info.backend,
        adapter_limits.max_texture_dimension_2d
    );

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("Main Device"),
            required_features: wgpu::Features::empty(),
            required_limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| format!("failed to create device: {:?}", e))?;

    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps
        .formats
        .iter()
        .copied()
        .find(|f| f.is_srgb())
        .or_else(|| surface_caps.formats.first().copied())
        .ok_or_else(|| JsValue::from_str("web surface reports no supported formats"))?;
    let alpha_mode = surface_caps
        .alpha_modes
        .first()
        .copied()
        .ok_or_else(|| JsValue::from_str("web surface reports no supported alpha modes"))?;

    let present_mode = crate::present_mode::select_present_mode(&surface_caps);
    let surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: (width as f64 * scale_factor) as u32,
        height: (height as f64 * scale_factor) as u32,
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &surface_config);

    let mut surface_config = surface_config;
    let (actual_width, actual_height, effective_scale) =
        if adapter_info.backend == wgpu::Backend::BrowserWebGpu {
            (surface_config.width, surface_config.height, scale_factor)
        } else {
            // WebGL backends can cap the swapchain below the requested size.
            // Probe the actual surface once so layout and root scale match the
            // real render target instead of the requested CSS size.
            let probe = match current_surface_texture(&surface, "web probe") {
                SurfaceFrame::Ready(probe) => probe,
                SurfaceFrame::Reconfigure => {
                    return Err(
                        "failed to probe surface texture: surface needs reconfiguration".into(),
                    );
                }
                SurfaceFrame::Skip => {
                    return Err("failed to probe surface texture: surface unavailable".into());
                }
            };
            let actual_width = probe.texture.width();
            let actual_height = probe.texture.height();
            probe.present();
            let effective_scale =
                if actual_width < surface_config.width || actual_height < surface_config.height {
                    let fit_x = actual_width as f64 / width as f64;
                    let fit_y = actual_height as f64 / height as f64;
                    let s = fit_x.min(fit_y);
                    surface_config.width = actual_width;
                    surface_config.height = actual_height;
                    s
                } else {
                    scale_factor
                };
            (actual_width, actual_height, effective_scale)
        };

    // Create renderer with fonts from settings
    let fonts: &[&[u8]] = settings.fonts.unwrap_or(&[]);
    log::info!("Web renderer startup: {} font(s)", fonts.len());
    let mut renderer = WgpuRenderer::new(fonts);
    renderer.init_gpu(
        Arc::new(device),
        Arc::new(queue),
        surface_format,
        adapter_info.backend,
    );
    renderer.set_root_scale(effective_scale as f32);

    let app = Rc::new(RefCell::new(AppShell::new_with_size_and_density(
        renderer,
        default_root_key(),
        content,
        (actual_width, actual_height),
        (width as f32, height as f32),
        effective_scale as f32,
    )));
    let platform = Rc::new(RefCell::new(WebPlatform::default()));
    platform.borrow_mut().set_scale_factor(effective_scale);

    let surface = Rc::new(surface);
    let surface_config = Rc::new(RefCell::new(surface_config));
    let render_loop: Rc<RefCell<Option<Closure<dyn FnMut()>>>> = Rc::new(RefCell::new(None));
    let frame_pending = Rc::new(Cell::new(false));
    let frame_timer = Rc::new(WebFrameTimer::default());
    // Starts true so the scene built during `AppShell` construction is presented
    // on the first frame even though `update()` reports no visual work; cleared
    // only after a successful present (mirrors the desktop `surface_dirty` flag).
    let surface_dirty = Rc::new(Cell::new(true));
    let request_frame: Rc<dyn Fn()> = {
        let frame_pending = frame_pending.clone();
        let render_loop = render_loop.clone();
        let frame_timer = frame_timer.clone();
        Rc::new(move || request_web_frame(&frame_pending, &render_loop, Some(&frame_timer)))
    };
    app.borrow_mut().set_frame_waker({
        let request_frame = request_frame.clone();
        move || request_frame()
    });

    // Hidden editable element backing IME composition. Browsers only start
    // IME composition sessions (and mobile browsers only show their virtual
    // keyboard) when an editable element is focused, so text-field focus
    // moves DOM focus onto this textarea; its composition events feed the
    // shared preedit/commit pipeline below.
    let ime_textarea = create_ime_textarea(&document)?;
    app.borrow_mut()
        .set_platform_text_input(Rc::new(WebTextInput {
            textarea: ime_textarea.clone(),
        }));

    // Pointer events (below) cover mouse, touch and pen and carry the device
    // type via `pointerType`; the separate `mouse*` listeners are intentionally
    // not registered so a mouse click is not double-dispatched and the pointer
    // source stays unambiguous.
    {
        let app = app.clone();
        let platform = platform.clone();
        let wheel_canvas = canvas.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: WheelEvent| {
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);

            let mut delta_x = event.delta_x() as f32;
            let mut delta_y = event.delta_y() as f32;
            match event.delta_mode() {
                WheelEvent::DOM_DELTA_PIXEL => {}
                WheelEvent::DOM_DELTA_LINE => {
                    delta_x *= WEB_WHEEL_LINE_DELTA_PIXELS;
                    delta_y *= WEB_WHEEL_LINE_DELTA_PIXELS;
                }
                WheelEvent::DOM_DELTA_PAGE => {
                    let page_width = wheel_canvas.client_width().max(1) as f32;
                    let page_height = wheel_canvas.client_height().max(1) as f32;
                    delta_x *= page_width;
                    delta_y *= page_height;
                }
                _ => {}
            }

            // Ctrl+wheel is the browser zoom gesture; trackpad pinches also
            // arrive as ctrl+wheel events. Translate the vertical delta into
            // a multiplicative zoom step about the cursor.
            if event.ctrl_key() {
                // Browsers report wheel-down/pinch-in as positive delta_y.
                let zoom_factor = 1.2f32.powf(-delta_y / 40.0);
                if let Ok(mut app_mut) = app.try_borrow_mut() {
                    app_mut.set_cursor(logical.x, logical.y);
                    if app_mut.pointer_zoomed(zoom_factor) {
                        event.prevent_default();
                    }
                    request_frame();
                }
                return;
            }

            if event.alt_key() {
                if delta_x.abs() <= f32::EPSILON {
                    delta_x = delta_y;
                }
                delta_y = 0.0;
            }

            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_cursor(logical.x, logical.y);
                if app_mut.pointer_scrolled(delta_x, delta_y) {
                    event.prevent_default();
                }
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("wheel", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up pointer event handlers for touch support
    {
        let app = app.clone();
        let platform = platform.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_pointer_source(web_pointer_source(&event));
                app_mut.set_cursor(logical.x, logical.y);
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointermove", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let platform = platform.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_pointer_source(web_pointer_source(&event));
                app_mut.set_cursor(logical.x, logical.y);
                app_mut.pointer_pressed();
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let platform = platform.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_pointer_source(web_pointer_source(&event));
                // Touch lift-off positions must not become velocity samples
                // (see AppShell::pointer_released_at_position).
                app_mut.pointer_released_at_position(logical.x, logical.y);
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointerup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.cancel_gesture();
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        canvas
            .add_event_listener_with_callback("pointercancel", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up keyboard event handlers
    // Note: We need to listen on document (not canvas) for keyboard events
    // unless the canvas has tabindex set and is focused
    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};

            // During IME composition the browser owns text input: key events
            // arrive with `isComposing` (or the synthetic 229 keyCode) and the
            // composition listeners deliver the text. Dispatching them (or
            // calling prevent_default) would double-commit characters and can
            // break the composition session.
            if event.is_composing() || event.key_code() == 229 {
                return;
            }

            // Convert web key code to our KeyCode
            let key_code = match event.code().as_str() {
                // Letters
                "KeyA" => KeyCode::A,
                "KeyB" => KeyCode::B,
                "KeyC" => KeyCode::C,
                "KeyD" => KeyCode::D,
                "KeyE" => KeyCode::E,
                "KeyF" => KeyCode::F,
                "KeyG" => KeyCode::G,
                "KeyH" => KeyCode::H,
                "KeyI" => KeyCode::I,
                "KeyJ" => KeyCode::J,
                "KeyK" => KeyCode::K,
                "KeyL" => KeyCode::L,
                "KeyM" => KeyCode::M,
                "KeyN" => KeyCode::N,
                "KeyO" => KeyCode::O,
                "KeyP" => KeyCode::P,
                "KeyQ" => KeyCode::Q,
                "KeyR" => KeyCode::R,
                "KeyS" => KeyCode::S,
                "KeyT" => KeyCode::T,
                "KeyU" => KeyCode::U,
                "KeyV" => KeyCode::V,
                "KeyW" => KeyCode::W,
                "KeyX" => KeyCode::X,
                "KeyY" => KeyCode::Y,
                "KeyZ" => KeyCode::Z,
                // Numbers
                "Digit0" => KeyCode::Digit0,
                "Digit1" => KeyCode::Digit1,
                "Digit2" => KeyCode::Digit2,
                "Digit3" => KeyCode::Digit3,
                "Digit4" => KeyCode::Digit4,
                "Digit5" => KeyCode::Digit5,
                "Digit6" => KeyCode::Digit6,
                "Digit7" => KeyCode::Digit7,
                "Digit8" => KeyCode::Digit8,
                "Digit9" => KeyCode::Digit9,
                // Navigation
                "ArrowUp" => KeyCode::ArrowUp,
                "ArrowDown" => KeyCode::ArrowDown,
                "ArrowLeft" => KeyCode::ArrowLeft,
                "ArrowRight" => KeyCode::ArrowRight,
                "Home" => KeyCode::Home,
                "End" => KeyCode::End,
                "PageUp" => KeyCode::PageUp,
                "PageDown" => KeyCode::PageDown,
                // Editing
                "Backspace" => KeyCode::Backspace,
                "Delete" => KeyCode::Delete,
                "Enter" | "NumpadEnter" => KeyCode::Enter,
                "Tab" => KeyCode::Tab,
                "Space" => KeyCode::Space,
                "Escape" => KeyCode::Escape,
                // Punctuation
                "Minus" => KeyCode::Minus,
                "Equal" => KeyCode::Equal,
                "BracketLeft" => KeyCode::BracketLeft,
                "BracketRight" => KeyCode::BracketRight,
                "Backslash" => KeyCode::Backslash,
                "Semicolon" => KeyCode::Semicolon,
                "Quote" => KeyCode::Quote,
                "Comma" => KeyCode::Comma,
                "Period" => KeyCode::Period,
                "Slash" => KeyCode::Slash,
                "Backquote" => KeyCode::Backquote,
                _ => KeyCode::Unknown,
            };

            let modifiers = Modifiers {
                shift: event.shift_key(),
                ctrl: event.ctrl_key(),
                alt: event.alt_key(),
                meta: event.meta_key(),
            };

            // Get the text produced by this key (from event.key)
            // Filter out long key names like "Shift", "Control", etc.
            let text = {
                let key = event.key();
                if key.len() == 1 {
                    key
                } else {
                    String::new()
                }
            };

            let key_event = KeyEvent {
                key_code,
                text,
                modifiers,
                event_type: KeyEventType::KeyDown,
            };

            if let Ok(mut app_mut) = app.try_borrow_mut() {
                if app_mut.on_key_event(&key_event) {
                    event.prevent_default();
                }
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};

            // Skip IME-composition key events (see the keydown handler).
            if event.is_composing() || event.key_code() == 229 {
                return;
            }

            // Similar conversion for keyup
            let key_code = match event.code().as_str() {
                "KeyA" => KeyCode::A,
                "KeyB" => KeyCode::B,
                "KeyC" => KeyCode::C,
                "KeyD" => KeyCode::D,
                "KeyE" => KeyCode::E,
                "KeyF" => KeyCode::F,
                "KeyG" => KeyCode::G,
                "KeyH" => KeyCode::H,
                "KeyI" => KeyCode::I,
                "KeyJ" => KeyCode::J,
                "KeyK" => KeyCode::K,
                "KeyL" => KeyCode::L,
                "KeyM" => KeyCode::M,
                "KeyN" => KeyCode::N,
                "KeyO" => KeyCode::O,
                "KeyP" => KeyCode::P,
                "KeyQ" => KeyCode::Q,
                "KeyR" => KeyCode::R,
                "KeyS" => KeyCode::S,
                "KeyT" => KeyCode::T,
                "KeyU" => KeyCode::U,
                "KeyV" => KeyCode::V,
                "KeyW" => KeyCode::W,
                "KeyX" => KeyCode::X,
                "KeyY" => KeyCode::Y,
                "KeyZ" => KeyCode::Z,
                "Digit0" => KeyCode::Digit0,
                "Digit1" => KeyCode::Digit1,
                "Digit2" => KeyCode::Digit2,
                "Digit3" => KeyCode::Digit3,
                "Digit4" => KeyCode::Digit4,
                "Digit5" => KeyCode::Digit5,
                "Digit6" => KeyCode::Digit6,
                "Digit7" => KeyCode::Digit7,
                "Digit8" => KeyCode::Digit8,
                "Digit9" => KeyCode::Digit9,
                "ArrowUp" => KeyCode::ArrowUp,
                "ArrowDown" => KeyCode::ArrowDown,
                "ArrowLeft" => KeyCode::ArrowLeft,
                "ArrowRight" => KeyCode::ArrowRight,
                "Backspace" => KeyCode::Backspace,
                "Delete" => KeyCode::Delete,
                "Enter" | "NumpadEnter" => KeyCode::Enter,
                "Tab" => KeyCode::Tab,
                "Space" => KeyCode::Space,
                _ => KeyCode::Unknown,
            };

            let modifiers = Modifiers {
                shift: event.shift_key(),
                ctrl: event.ctrl_key(),
                alt: event.alt_key(),
                meta: event.meta_key(),
            };

            let key_event = KeyEvent {
                key_code,
                text: String::new(), // KeyUp doesn't produce text
                modifiers,
                event_type: KeyEventType::KeyUp,
            };

            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.on_key_event(&key_event);
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("keyup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up paste event handler for clipboard paste
    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            // Get pasted text from clipboardData (synchronous!)
            if let Some(data) = event.clipboard_data() {
                if let Ok(text) = data.get_data("text/plain") {
                    if !text.is_empty() {
                        if let Ok(mut app_mut) = app.try_borrow_mut() {
                            if app_mut.on_paste(&text) {
                                event.prevent_default();
                            }
                            request_frame();
                        }
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("paste", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up copy event handler for clipboard copy
    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                if let Some(text) = app_mut.on_copy() {
                    // Put copied text into the clipboard via the event
                    if let Some(data) = event.clipboard_data() {
                        let _ = data.set_data("text/plain", &text);
                        event.prevent_default();
                    }
                    request_frame();
                }
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("copy", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up cut event handler for clipboard cut
    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                if let Some(text) = app_mut.on_cut() {
                    // Put cut text into the clipboard via the event
                    if let Some(data) = event.clipboard_data() {
                        let _ = data.set_data("text/plain", &text);
                        event.prevent_default();
                    }
                    request_frame();
                }
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("cut", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // IME composition: the browser resends the whole preedit on every update,
    // so `compositionupdate` maps 1:1 onto the framework preedit hook.
    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::CompositionEvent| {
            let text = event.data().unwrap_or_default();
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.on_ime_preedit(&text, None);
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        ime_textarea.add_event_listener_with_callback(
            "compositionupdate",
            closure.as_ref().unchecked_ref(),
        )?;
        closure.forget();
    }

    // Composition finished: clear the preedit (which deletes the composing
    // text) and commit the final string, mirroring the desktop
    // `Ime::Commit` flow.
    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let textarea = ime_textarea.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::CompositionEvent| {
            let text = event.data().unwrap_or_default();
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                let _ = app_mut.on_ime_preedit("", None);
                if !text.is_empty() {
                    app_mut.on_paste(&text);
                }
                request_frame();
            }
            // Drop the composed text from the hidden textarea so the next
            // composition session starts from an empty mirror.
            textarea.set_value("");
        }) as Box<dyn FnMut(_)>);
        ime_textarea
            .add_event_listener_with_callback("compositionend", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    let frame_pending_for_loop = frame_pending.clone();
    let frame_timer_for_loop = frame_timer.clone();
    let render_loop_for_deadline = render_loop.clone();
    let surface_dirty_for_loop = surface_dirty.clone();
    let request_frame_for_loop = request_frame.clone();

    *render_loop.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        frame_pending_for_loop.set(false);
        let update_result = app.borrow_mut().update();

        // Present when the surface still owes its first (or post-reconfigure)
        // frame, when `update()` produced visual work, or when the app otherwise
        // wants a redraw. Gating solely on `visual_changed` would skip the scene
        // built during construction and leave the canvas blank until an event
        // dirtied the tree (the "white until scroll" bug).
        let present_required = surface_present_required(
            surface_dirty_for_loop.get(),
            update_result.visual_changed,
            app.borrow().needs_redraw(),
        );
        if present_required {
            let config = surface_config.borrow();
            match current_surface_texture(&surface, "web") {
                SurfaceFrame::Ready(output) => {
                    let view = output
                        .texture
                        .create_view(&wgpu::TextureViewDescriptor::default());
                    let render_width = output.texture.width();
                    let render_height = output.texture.height();

                    {
                        let mut app_mut = app.borrow_mut();
                        if let Err(err) =
                            app_mut
                                .renderer()
                                .render(&view, render_width, render_height)
                        {
                            log::error!("render failed: {:?}", err);
                        }
                    }

                    output.present();
                    surface_dirty_for_loop.set(false);
                }
                SurfaceFrame::Reconfigure => {
                    {
                        let mut app_mut = app.borrow_mut();
                        if let Some(device) = app_mut.renderer().try_device() {
                            surface.configure(device, &*config);
                        } else {
                            log::error!(
                                "web surface reconfigure skipped: GPU renderer is not initialized"
                            );
                        }
                    }
                    // The reconfigured surface has not presented yet: keep it
                    // dirty and request another frame so it flushes.
                    surface_dirty_for_loop.set(true);
                    request_frame_for_loop();
                }
                // Surface unavailable this tick; keep it dirty so the next
                // scheduled frame retries the present.
                SurfaceFrame::Skip => surface_dirty_for_loop.set(true),
            }
        }

        let frame_driver = WebPlatformFrameDriver {
            frame_timer: &frame_timer_for_loop,
            frame_pending: &frame_pending_for_loop,
            render_loop: &render_loop_for_deadline,
        };
        app.borrow().schedule_platform_frame(&frame_driver);
    }) as Box<dyn FnMut()>));

    // Start the render loop
    request_frame();

    Ok(())
}

/// Web text-input focus hook: moves DOM focus onto the hidden IME textarea
/// while a cranpose text field is focused, and releases it when text-field
/// focus is lost.
///
/// The browser only starts IME composition sessions for editable elements
/// (and mobile browsers only show their virtual keyboard for them), so the
/// canvas alone can never receive composition events.
struct WebTextInput {
    textarea: web_sys::HtmlTextAreaElement,
}

impl cranpose_app_shell::PlatformTextInputHandler for WebTextInput {
    fn show_keyboard(&self) {
        let _ = self.textarea.focus();
    }

    fn hide_keyboard(&self) {
        self.textarea.set_value("");
        let _ = self.textarea.blur();
    }
}

/// Creates the visually hidden, focusable textarea that backs IME
/// composition and appends it to the document body.
fn create_ime_textarea(
    document: &web_sys::Document,
) -> Result<web_sys::HtmlTextAreaElement, JsValue> {
    let textarea: web_sys::HtmlTextAreaElement = document.create_element("textarea")?.dyn_into()?;

    // The composed text is delivered through composition events; browser
    // assistance on the mirror itself would only interfere.
    textarea.set_attribute("autocomplete", "off")?;
    textarea.set_attribute("autocorrect", "off")?;
    textarea.set_attribute("autocapitalize", "off")?;
    textarea.set_attribute("spellcheck", "false")?;
    // Programmatic focus only - the textarea must never be tab-reachable.
    textarea.set_attribute("tabindex", "-1")?;
    textarea.set_attribute("aria-hidden", "true")?;

    // Visually hidden but still focusable: `display:none`/`visibility:hidden`
    // elements refuse focus, so shrink to a transparent 1x1 point instead.
    let style = textarea.style();
    style.set_property("position", "fixed")?;
    style.set_property("top", "0")?;
    style.set_property("left", "0")?;
    style.set_property("width", "1px")?;
    style.set_property("height", "1px")?;
    style.set_property("opacity", "0")?;
    style.set_property("border", "0")?;
    style.set_property("padding", "0")?;
    style.set_property("margin", "0")?;
    style.set_property("outline", "none")?;
    style.set_property("resize", "none")?;
    style.set_property("overflow", "hidden")?;
    style.set_property("background", "transparent")?;
    style.set_property("pointer-events", "none")?;

    let body = document
        .body()
        .ok_or_else(|| JsValue::from_str("document has no body for the IME textarea"))?;
    body.append_child(&textarea)?;
    Ok(textarea)
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) -> bool {
    let Some(window) = web_sys::window() else {
        log::error!("requestAnimationFrame unavailable: browser window is not available");
        return false;
    };

    match window.request_animation_frame(f.as_ref().unchecked_ref()) {
        Ok(_) => true,
        Err(error) => {
            log::error!("requestAnimationFrame registration failed: {error:?}");
            false
        }
    }
}

fn request_web_frame(
    frame_pending: &Cell<bool>,
    render_loop: &Rc<RefCell<Option<Closure<dyn FnMut()>>>>,
    timer: Option<&WebFrameTimer>,
) {
    if let Some(timer) = timer {
        timer.pending.set(false);
        timer
            .generation
            .set(timer.generation.get().saturating_add(1));
    }
    if frame_pending.replace(true) {
        return;
    }
    let render_loop = render_loop.borrow();
    let Some(render_loop) = render_loop.as_ref() else {
        frame_pending.set(false);
        return;
    };
    if !request_animation_frame(render_loop) {
        frame_pending.set(false);
    }
}

fn request_web_frame_at_deadline(
    timer: &Rc<WebFrameTimer>,
    deadline: web_time::Instant,
    frame_pending: &Rc<Cell<bool>>,
    render_loop: &Rc<RefCell<Option<Closure<dyn FnMut()>>>>,
) {
    if frame_pending.get() || timer.pending.get() {
        return;
    }

    timer.pending.set(true);
    let generation = timer.generation.get();
    let delay = deadline
        .checked_duration_since(web_time::Instant::now())
        .unwrap_or_default();
    let delay_ms = delay.as_millis().min(i32::MAX as u128) as i32;
    let timer_for_timeout = timer.clone();
    let frame_pending_for_timeout = frame_pending.clone();
    let render_loop_for_timeout = render_loop.clone();
    let callback = Closure::once_into_js(move || {
        if timer_for_timeout.generation.get() != generation {
            return;
        }
        timer_for_timeout.pending.set(false);
        request_web_frame(
            &frame_pending_for_timeout,
            &render_loop_for_timeout,
            Some(&timer_for_timeout),
        );
    });

    let Some(window) = web_sys::window() else {
        log::error!("setTimeout unavailable: browser window is not available");
        timer.pending.set(false);
        request_web_frame(frame_pending, render_loop, Some(timer));
        return;
    };

    if let Err(error) = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(callback.unchecked_ref(), delay_ms)
    {
        log::error!("setTimeout registration failed for frame deadline: {error:?}");
        timer.pending.set(false);
        request_web_frame(frame_pending, render_loop, Some(timer));
    }
}

fn clear_web_frame_wake(timer: &WebFrameTimer) {
    timer.pending.set(false);
    timer
        .generation
        .set(timer.generation.get().saturating_add(1));
}

fn requested_web_backend(window: &web_sys::Window) -> WebBackendPreference {
    let query = window.location().search().unwrap_or_default();
    for pair in query.trim_start_matches('?').split('&') {
        let Some((key, value)) = pair.split_once('=') else {
            continue;
        };
        if key != "backend" {
            continue;
        }
        return match value {
            "webgpu" => WebBackendPreference::WebGpu,
            "gl" => WebBackendPreference::Gl,
            _ => WebBackendPreference::Auto,
        };
    }
    WebBackendPreference::Gl
}

fn instance_backends(preference: WebBackendPreference) -> wgpu::Backends {
    match preference {
        WebBackendPreference::Auto => wgpu::Backends::BROWSER_WEBGPU | wgpu::Backends::GL,
        WebBackendPreference::WebGpu => wgpu::Backends::BROWSER_WEBGPU,
        WebBackendPreference::Gl => wgpu::Backends::GL,
    }
}

fn required_limits_for_web_backend(
    backend: wgpu::Backend,
    adapter_limits: wgpu::Limits,
) -> wgpu::Limits {
    match backend {
        wgpu::Backend::BrowserWebGpu => wgpu::Limits::default().using_resolution(adapter_limits),
        _ => wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter_limits),
    }
}
