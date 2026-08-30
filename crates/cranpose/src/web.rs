//! Web runtime for Compose applications.
//!
//! This module provides the web event loop implementation using wasm-bindgen and WebGPU.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use cranpose_app_shell::{AppShell, PlatformFrameDriver, PointerSource, default_root_key};
use cranpose_platform_web::WebPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{HtmlCanvasElement, PointerEvent, WheelEvent};

use crate::{
    app_launcher::AppSettings,
    wgpu_surface::{
        SurfaceFrame, current_surface_texture, present_initial_placeholder_frame,
        surface_present_required,
    },
};

type RenderLoop = Rc<RefCell<Option<Closure<dyn FnMut()>>>>;

type ReshapeFn = Rc<dyn Fn(Option<(f32, f32)>)>;

fn web_pointer_source(event: &PointerEvent) -> PointerSource {
    match event.pointer_type().as_str() {
        "touch" => PointerSource::Touch,
        "pen" => PointerSource::Stylus,
        "mouse" => PointerSource::Mouse,
        _ => PointerSource::Unknown,
    }
}

fn web_modifiers(event: &web_sys::MouseEvent) -> cranpose_app_shell::Modifiers {
    cranpose_app_shell::Modifiers {
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    }
}

fn wheel_uptime_millis() -> u64 {
    thread_local! {
        static EPOCH: web_time::Instant = web_time::Instant::now();
    }
    EPOCH.with(|epoch| epoch.elapsed().as_millis() as u64)
}

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
    render_loop: &'a RenderLoop,
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

fn set_height_to_dynamic_viewport_height_with_static_fallback(
    style: &web_sys::CssStyleDeclaration,
) -> Result<(), JsValue> {
    style.set_property("height", "100vh")?;
    style.set_property("height", "100dvh")
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
    console_error_panic_hook::set_once();

    let platform_env = crate::platform_env::PlatformEnvironment::new();
    let mut content = content;
    let content = {
        let env = Rc::clone(&platform_env);
        move || env.compose_root(&mut content)
    };

    crate::web_services::register();
    crate::web_host_surface::install();

    let window = web_sys::window().ok_or("no global window exists")?;
    let document = window
        .document()
        .ok_or("should have a document on window")?;

    let canvas = document
        .get_element_by_id(canvas_id)
        .ok_or_else(|| format!("canvas with id '{}' not found", canvas_id))?
        .dyn_into::<HtmlCanvasElement>()?;

    let scale_factor = window.device_pixel_ratio();

    let requested_width = settings.initial_width;
    let requested_height = settings.initial_height;
    if let Some(html_element) = canvas.dyn_ref::<web_sys::HtmlElement>() {
        let style = html_element.style();
        if settings.web_fill_viewport {
            style.set_property("width", "100vw")?;
            set_height_to_dynamic_viewport_height_with_static_fallback(&style)?;
        } else {
            style.set_property(
                "width",
                &format!("min({requested_width}px, calc(100vw - 36px))"),
            )?;
            style.set_property(
                "height",
                &format!("min({requested_height}px, calc(100vh - 36px))"),
            )?;
        }
        style.set_property("touch-action", "none")?;
    }
    let width = canvas.client_width().max(1) as u32;
    let height = canvas.client_height().max(1) as u32;
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
    let render_scale = crate::web_surface_scale::web_canvas_buffer_scale(scale_factor);
    let (buffer_width, buffer_height) =
        crate::web_surface_scale::web_canvas_buffer_dimensions(width, height, scale_factor);
    canvas.set_width(buffer_width);
    canvas.set_height(buffer_height);
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
            required_features: cranpose_render_wgpu::optional_device_features(&adapter),
            required_limits,
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::default(),
            trace: wgpu::Trace::Off,
        })
        .await
        .map_err(|e| format!("failed to create device: {:?}", e))?;

    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format =
        crate::surface_format::select_display_surface_format(&surface_caps.formats)
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
        width: buffer_width,
        height: buffer_height,
        present_mode,
        alpha_mode,
        view_formats: crate::surface_format::display_surface_view_formats(surface_format),
        desired_maximum_frame_latency: 2,
    };

    surface.configure(&device, &surface_config);

    let mut surface_config = surface_config;
    let (actual_width, actual_height, effective_scale) =
        if adapter_info.backend == wgpu::Backend::BrowserWebGpu {
            present_initial_placeholder_frame(
                &surface,
                &device,
                &queue,
                surface_format,
                "web initial present",
            );
            (surface_config.width, surface_config.height, render_scale)
        } else {
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
            let probe_view = probe.texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(surface_format.remove_srgb_suffix()),
                ..Default::default()
            });
            cranpose_render_wgpu::clear_to_default_background(&device, &queue, &probe_view);
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
                    render_scale
                };
            (actual_width, actual_height, effective_scale)
        };
    log::info!(
        "Web canvas css={}x{}, buffer={}x{}, effective_scale={:.2}, device_scale={:.2}",
        width,
        height,
        actual_width,
        actual_height,
        effective_scale,
        scale_factor
    );

    let fonts = settings.resolve_font_set();
    let mut renderer = WgpuRenderer::with_font_set(fonts);
    #[allow(clippy::arc_with_non_send_sync)]
    renderer.init_gpu(
        Arc::new(device),
        Arc::new(queue),
        crate::surface_format::display_surface_view_format(surface_format),
        adapter_info.backend,
        adapter.get_downlevel_capabilities().flags,
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
    app.borrow_mut().set_semantics_enabled(true);
    let accessibility = Rc::new(RefCell::new(
        crate::web_accessibility::WebAccessibilityBridge::install(
            &document,
            canvas.clone(),
            app.clone(),
        )?,
    ));
    let platform = Rc::new(RefCell::new(WebPlatform::default()));
    platform.borrow_mut().set_scale_factor(scale_factor);

    let surface = Rc::new(surface);
    let surface_config = Rc::new(RefCell::new(surface_config));
    let render_loop: RenderLoop = Rc::new(RefCell::new(None));
    let frame_pending = Rc::new(Cell::new(false));
    let frame_timer = Rc::new(WebFrameTimer::default());
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

    crate::web_clipboard::install(&app, request_frame.clone());

    crate::web_drop::install(&canvas, request_frame.clone())?;

    crate::web_power::start_battery_probe(request_frame.clone());

    let ime_textarea = create_ime_textarea(&document)?;
    app.borrow_mut()
        .set_platform_text_input(Rc::new(WebTextInput {
            textarea: ime_textarea.clone(),
        }));

    if let Ok(Some(query)) = window.match_media("(prefers-color-scheme: dark)") {
        let initial = if query.matches() {
            cranpose_services::SystemTheme::Dark
        } else {
            cranpose_services::SystemTheme::Light
        };
        platform_env.set_system_theme(initial);
        let env = Rc::clone(&platform_env);
        let app_for_theme = app.clone();
        let request_frame_for_theme = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::MediaQueryListEvent| {
            let theme = if event.matches() {
                cranpose_services::SystemTheme::Dark
            } else {
                cranpose_services::SystemTheme::Light
            };
            if env.set_system_theme(theme) {
                app_for_theme.borrow_mut().request_root_render();
                request_frame_for_theme();
            }
        }) as Box<dyn FnMut(_)>);
        query.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let platform = platform.clone();
        let wheel_canvas = canvas.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: WheelEvent| {
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);

            let wheel = crate::web_wheel::wheel_scroll_from_dom(
                event.delta_x() as f32,
                event.delta_y() as f32,
                event.delta_mode(),
                crate::web_wheel::WebWheelPage {
                    width: wheel_canvas.client_width() as f32,
                    height: wheel_canvas.client_height() as f32,
                },
                web_modifiers(&event),
                wheel_uptime_millis(),
            );

            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_cursor(logical.x, logical.y);
                if app_mut.wheel_scrolled(wheel) {
                    event.prevent_default();
                }
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("wheel", closure.as_ref().unchecked_ref())?;
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
                app_mut.set_modifiers(web_modifiers(&event));
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
        let pointer_canvas = canvas.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            let _ = pointer_canvas.set_pointer_capture(event.pointer_id());
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_pointer_source(web_pointer_source(&event));
                app_mut.set_modifiers(web_modifiers(&event));
                let event_time = app_mut.realtime_pointer_event_time(None);
                app_mut.set_cursor_at_event_time(logical.x, logical.y, event_time);
                app_mut.pointer_pressed_at_event_time(event_time);
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointerdown", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let platform = platform.clone();
        let pointer_canvas = canvas.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            let x = event.offset_x() as f64;
            let y = event.offset_y() as f64;
            let logical = platform.borrow().pointer_position(x, y);
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.set_pointer_source(web_pointer_source(&event));
                app_mut.set_modifiers(web_modifiers(&event));
                let event_time = app_mut.realtime_pointer_event_time(None);
                app_mut.pointer_released_at_position_event_time(logical.x, logical.y, event_time);
                request_frame();
            }
            let _ = pointer_canvas.release_pointer_capture(event.pointer_id());
        }) as Box<dyn FnMut(_)>);
        canvas.add_event_listener_with_callback("pointerup", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let pointer_canvas = canvas.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            if let Ok(mut app_mut) = app.try_borrow_mut() {
                app_mut.cancel_gesture();
                request_frame();
            }
            let _ = pointer_canvas.release_pointer_capture(event.pointer_id());
        }) as Box<dyn FnMut(_)>);
        canvas
            .add_event_listener_with_callback("pointercancel", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::KeyboardEvent| {
            use cranpose_app_shell::{KeyCode, KeyEvent, KeyEventType, Modifiers};

            if event.is_composing() || event.key_code() == 229 {
                return;
            }

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
                "Home" => KeyCode::Home,
                "End" => KeyCode::End,
                "PageUp" => KeyCode::PageUp,
                "PageDown" => KeyCode::PageDown,
                "Backspace" => KeyCode::Backspace,
                "Delete" => KeyCode::Delete,
                "Enter" | "NumpadEnter" => KeyCode::Enter,
                "Tab" => KeyCode::Tab,
                "Space" => KeyCode::Space,
                "Escape" => KeyCode::Escape,
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

            let text = {
                let key = event.key();
                if key.len() == 1 { key } else { String::new() }
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

            if event.is_composing() || event.key_code() == 229 {
                return;
            }

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
                text: String::new(),
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

    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            if let Some(data) = event.clipboard_data()
                && let Ok(text) = data.get_data("text/plain")
                && !text.is_empty()
                && let Ok(mut app_mut) = app.try_borrow_mut()
            {
                if app_mut.on_paste(&text) {
                    event.prevent_default();
                }
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("paste", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut()
                && let Some(text) = app_mut.on_copy()
            {
                if let Some(data) = event.clipboard_data() {
                    let _ = data.set_data("text/plain", &text);
                    event.prevent_default();
                }
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("copy", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let app = app.clone();
        let request_frame = request_frame.clone();
        let closure = Closure::wrap(Box::new(move |event: web_sys::ClipboardEvent| {
            if let Ok(mut app_mut) = app.try_borrow_mut()
                && let Some(text) = app_mut.on_cut()
            {
                if let Some(data) = event.clipboard_data() {
                    let _ = data.set_data("text/plain", &text);
                    event.prevent_default();
                }
                request_frame();
            }
        }) as Box<dyn FnMut(_)>);
        document.add_event_listener_with_callback("cut", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

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
            textarea.set_value("");
        }) as Box<dyn FnMut(_)>);
        ime_textarea
            .add_event_listener_with_callback("compositionend", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    let reshape: ReshapeFn = {
        let canvas = canvas.clone();
        let window = window.clone();
        let app = app.clone();
        let platform = platform.clone();
        let surface = surface.clone();
        let surface_config = surface_config.clone();
        let surface_dirty = surface_dirty.clone();
        let request_frame = request_frame.clone();
        Rc::new(move |requested: Option<(f32, f32)>| {
            if let Some((requested_width, requested_height)) = requested
                && let Some(html_element) = canvas.dyn_ref::<web_sys::HtmlElement>()
            {
                let style = html_element.style();
                let _ = style.set_property("width", &format!("{requested_width}px"));
                let _ = style.set_property("height", &format!("{requested_height}px"));
            }

            let scale_factor = window.device_pixel_ratio();
            let width = canvas.client_width().max(1) as u32;
            let height = canvas.client_height().max(1) as u32;
            let (buffer_width, buffer_height) =
                crate::web_surface_scale::web_canvas_buffer_dimensions(width, height, scale_factor);
            let render_scale = crate::web_surface_scale::web_canvas_buffer_scale(scale_factor);

            let unchanged = {
                let config = surface_config.borrow();
                config.width == buffer_width && config.height == buffer_height
            };
            if unchanged && requested.is_none() {
                return;
            }

            canvas.set_width(buffer_width);
            canvas.set_height(buffer_height);
            {
                let mut config = surface_config.borrow_mut();
                config.width = buffer_width;
                config.height = buffer_height;
                let mut app_mut = app.borrow_mut();
                if let Some(device) = app_mut.renderer().try_device() {
                    surface.configure(device, &config);
                }
                app_mut.renderer().set_root_scale(render_scale as f32);
                app_mut.set_buffer_size(buffer_width, buffer_height);
                app_mut.set_viewport(width as f32, height as f32);
                app_mut.set_density(render_scale as f32);
                app_mut.request_root_render();
            }
            platform.borrow_mut().set_scale_factor(scale_factor);
            crate::web_host_surface::publish(width as f32, height as f32, render_scale as f32);
            surface_dirty.set(true);
            request_frame();
        })
    };

    crate::web_host_surface::publish(width as f32, height as f32, effective_scale as f32);

    {
        let reshape = reshape.clone();
        let closure = Closure::wrap(Box::new(move || reshape(None)) as Box<dyn FnMut()>);
        window.add_event_listener_with_callback("resize", closure.as_ref().unchecked_ref())?;
        closure.forget();
    }

    {
        let reshape = reshape.clone();
        let request_frame_for_requests = request_frame.clone();
        let closure = Closure::wrap(Box::new(move || {
            if let Some(size) = crate::web_host_surface::take_requested_size() {
                reshape(Some(size));
            }
            request_frame_for_requests();
        }) as Box<dyn FnMut()>);
        window.set_interval_with_callback_and_timeout_and_arguments_0(
            closure.as_ref().unchecked_ref(),
            60,
        )?;
        closure.forget();
    }

    let frame_pending_for_loop = frame_pending.clone();
    let frame_timer_for_loop = frame_timer.clone();
    let render_loop_for_deadline = render_loop.clone();
    let surface_dirty_for_loop = surface_dirty.clone();
    let request_frame_for_loop = request_frame.clone();
    let document_for_loop = document.clone();
    let accessibility_for_loop = accessibility.clone();

    *render_loop.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        frame_pending_for_loop.set(false);
        let update_result = app.borrow_mut().update();
        if let Ok(mut app_mut) = app.try_borrow_mut()
            && let Err(error) = accessibility_for_loop
                .borrow_mut()
                .sync(&document_for_loop, &mut app_mut)
        {
            log::error!("web accessibility sync failed: {error:?}");
        }

        let present_required = surface_present_required(
            surface_dirty_for_loop.get(),
            update_result.visual_changed,
            app.borrow().needs_redraw(),
        );
        if present_required {
            let config = surface_config.borrow();
            match current_surface_texture(&surface, "web") {
                SurfaceFrame::Ready(output) => {
                    let view = output.texture.create_view(&wgpu::TextureViewDescriptor {
                        format: Some(crate::surface_format::display_surface_view_format(
                            config.format,
                        )),
                        ..Default::default()
                    });
                    let render_width = output.texture.width();
                    let render_height = output.texture.height();

                    {
                        let mut app_mut = app.borrow_mut();
                        if let Err(err) = app_mut.renderer().render_surface_texture(
                            &view,
                            render_width,
                            render_height,
                        ) {
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
                            surface.configure(device, &config);
                        } else {
                            log::error!(
                                "web surface reconfigure skipped: GPU renderer is not initialized"
                            );
                        }
                    }
                    surface_dirty_for_loop.set(true);
                    request_frame_for_loop();
                }
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

    request_frame();

    Ok(())
}

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

fn create_ime_textarea(
    document: &web_sys::Document,
) -> Result<web_sys::HtmlTextAreaElement, JsValue> {
    let textarea: web_sys::HtmlTextAreaElement = document.create_element("textarea")?.dyn_into()?;

    textarea.set_attribute("autocomplete", "off")?;
    textarea.set_attribute("autocorrect", "off")?;
    textarea.set_attribute("autocapitalize", "off")?;
    textarea.set_attribute("spellcheck", "false")?;
    textarea.set_attribute("tabindex", "-1")?;
    textarea.set_attribute("aria-hidden", "true")?;

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
    render_loop: &RenderLoop,
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
    render_loop: &RenderLoop,
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
