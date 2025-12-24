//! Web runtime for Compose applications.
//!
//! This module provides the web event loop implementation using wasm-bindgen and WebGPU.

use crate::launcher::AppSettings;
use compose_app_shell::{default_root_key, AppShell};
use compose_platform_web::WebPlatform;
use compose_render_wgpu::WgpuRenderer;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, MouseEvent, PointerEvent};

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

    // Set CSS size using HtmlElement API
    if let Some(html_element) = canvas.dyn_ref::<web_sys::HtmlElement>() {
        let style = html_element.style();
        style.set_property("width", &format!("{}px", width))?;
        style.set_property("height", &format!("{}px", height))?;
    }

    // Initialize WGPU
    // Use WebGL backend for maximum compatibility with Chrome stable.
    // This avoids the wgpu 0.19 / Chrome WebGPU spec incompatibility
    // (maxInterStageShaderComponents vs maxInterStageShaderVariables).
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::GL,
        ..Default::default()
    });

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

    // For web, use downlevel defaults for maximum compatibility.
    // wgpu 0.19 uses newer WebGPU spec field names, so we use the most
    // conservative limits designed for WebGL2-level capabilities.
    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: Some("Main Device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
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
        .unwrap_or(surface_caps.formats[0]);

    let surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: (width as f64 * scale_factor) as u32,
        height: (height as f64 * scale_factor) as u32,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
        desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &surface_config);

    // Create renderer with fonts from settings
    let mut renderer = if let Some(fonts) = settings.fonts {
        WgpuRenderer::new_with_fonts(fonts)
    } else {
        WgpuRenderer::new()
    };
    renderer.init_gpu(Arc::new(device), Arc::new(queue), surface_format);
    renderer.set_root_scale(scale_factor as f32);

    let app = Rc::new(RefCell::new(AppShell::new(
        renderer,
        default_root_key(),
        content,
    )));
    let platform = Rc::new(RefCell::new(WebPlatform::default()));
    platform.borrow_mut().set_scale_factor(scale_factor);

    // Set buffer_size to physical pixels and viewport to logical dp
    app.borrow_mut()
        .set_buffer_size(surface_config.width, surface_config.height);
    app.borrow_mut().set_viewport(width as f32, height as f32);

    // Set up mouse event handlers
    {
        let app = app.clone();
        let platform = platform.clone();
        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);
            // Use try_borrow_mut to avoid panic if render loop is active
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_cursor(logical.x, logical.y);
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("mousemove", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |_event: MouseEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.pointer_pressed();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("mousedown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |_event: MouseEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.pointer_released();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("mouseup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up pointer event handlers for touch support
    {
        let app = app.clone();
        let platform = platform.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_cursor(logical.x, logical.y);
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointermove", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.pointer_pressed();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.pointer_released();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointerup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up keyboard event handlers
    // Note: We need to listen on document (not canvas) for keyboard events
    // unless the canvas has tabindex set and is focused
    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            use compose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};

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
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("keydown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            use compose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};

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
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("keyup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up paste event handler for clipboard paste
    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            // Get pasted text from clipboardData (synchronous!)
            if let Some(data) = event.clipboard_data() {
                if let Ok(text) = data.get_data("text/plain") {
                    if !text.is_empty() {
                        if let Ok(mut app_mut) = app.try_borrow_mut() {
                            if app_mut.on_paste(&text) {
                                event.prevent_default();
                            }
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
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                if let Some(text) = app_mut.on_copy() {
                    // Put copied text into the clipboard via the event
                    if let Some(data) = event.clipboard_data() {
                        let _ = data.set_data("text/plain", &text);
                        event.prevent_default();
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("copy", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Set up cut event handler for clipboard cut
    {
        let app = app.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                if let Some(text) = app_mut.on_cut() {
                    // Put cut text into the clipboard via the event
                    if let Some(data) = event.clipboard_data() {
                        let _ = data.set_data("text/plain", &text);
                        event.prevent_default();
                    }
                }
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("cut", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    // Render loop
    let render_loop = Rc::new(RefCell::new(None));
    let render_loop_clone = render_loop.clone();

    let surface = Rc::new(surface);
    let surface_config = Rc::new(RefCell::new(surface_config));

    *render_loop.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        app.borrow_mut().update();

        let config = surface_config.borrow();
        match surface.get_current_texture() {
            Ok(output) => {
                let view = output
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                {
                    let mut app_mut = app.borrow_mut();
                    if let Err(err) = app_mut
                        .renderer()
                        .render(&view, config.width, config.height)
                    {
                        log::error!("render failed: {:?}", err);
                    }
                }

                output.present();
            }
            Err(wgpu::SurfaceError::Lost) | Err(wgpu::SurfaceError::Outdated) => {
                // Reconfigure surface
                let mut app_mut = app.borrow_mut();
                let device = app_mut.renderer().device();
                surface.configure(device, &*config);
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                log::error!("Out of memory");
            }
            Err(wgpu::SurfaceError::Timeout) => {
                log::debug!("Surface timeout, skipping frame");
            }
            Err(wgpu::SurfaceError::Other) => {
                log::error!("Surface other error, skipping frame");
            }
        }

        // Request next frame
        request_animation_frame(render_loop_clone.borrow().as_ref().unwrap());
    }) as Box<dyn FnMut()>));

    // Start the render loop
    request_animation_frame(render_loop.borrow().as_ref().unwrap());

    Ok(())
}

fn request_animation_frame(f: &Closure<dyn FnMut()>) {
    web_sys::window()
        .unwrap()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("should register `requestAnimationFrame` OK");
}
