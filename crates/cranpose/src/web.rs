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

fn canvas_window(canvas: &HtmlCanvasElement, page: &web_sys::Window) -> web_sys::Window {
    canvas
        .owner_document()
        .and_then(|document| document.default_view())
        .unwrap_or_else(|| page.clone())
}

fn resize_floating_window(host: &web_sys::Window, width: f32, height: f32) -> bool {
    let measure = |value: Result<JsValue, JsValue>| value.ok().and_then(|value| value.as_f64());
    let (Some(outer_width), Some(outer_height), Some(inner_width), Some(inner_height)) = (
        measure(host.outer_width()),
        measure(host.outer_height()),
        measure(host.inner_width()),
        measure(host.inner_height()),
    ) else {
        return false;
    };
    let (window_width, window_height) = crate::web_floating_window::outer_size_for_inner(
        (width, height),
        (outer_width, outer_height),
        (inner_width, inner_height),
    );
    match host.resize_to(window_width, window_height) {
        Ok(()) => true,
        Err(error) => {
            log::debug!("the floating window waits for a gesture to resize: {error:?}");
            false
        }
    }
}

fn is_floating(host: &web_sys::Window, page: &web_sys::Window) -> bool {
    AsRef::<JsValue>::as_ref(host) != AsRef::<JsValue>::as_ref(page)
}

fn apply_requested_canvas_size(
    canvas: &HtmlCanvasElement,
    page: &web_sys::Window,
    (width, height): (f32, f32),
    owed: &Cell<Option<(f32, f32)>>,
) {
    let host = canvas_window(canvas, page);
    if is_floating(&host, page) {
        // A floating window's canvas fills the window. The window takes the
        // size, and the canvas follows it; a window that may not resize yet
        // owes it, and the canvas goes on filling it meanwhile rather than
        // shrinking inside it.
        let resized = resize_floating_window(&host, width, height);
        owed.set((!resized).then_some((width, height)));
        return;
    }
    if let Some(html_element) = canvas.dyn_ref::<web_sys::HtmlElement>() {
        let style = html_element.style();
        let _ = style.set_property("width", &format!("{width}px"));
        let _ = style.set_property("height", &format!("{height}px"));
    }
    owed.set(None);
}

fn resize_owed_floating_window(
    canvas: &HtmlCanvasElement,
    page: &web_sys::Window,
    owed: &Cell<Option<(f32, f32)>>,
) {
    let Some((width, height)) = owed.get() else {
        return;
    };
    let host = canvas_window(canvas, page);
    if !is_floating(&host, page) || resize_floating_window(&host, width, height) {
        owed.set(None);
    }
}

/// Resizes a floating window that owes a size as soon as a gesture lets it.
fn resize_owed_on_release(
    canvas: &HtmlCanvasElement,
    page: &web_sys::Window,
    owed: Rc<Cell<Option<(f32, f32)>>>,
) -> Result<(), JsValue> {
    let on_release = {
        let canvas = canvas.clone();
        let page = page.clone();
        Closure::wrap(Box::new(move |_event: PointerEvent| {
            resize_owed_floating_window(&canvas, &page, &owed);
        }) as Box<dyn FnMut(PointerEvent)>)
    };
    canvas.add_event_listener_with_callback("pointerup", on_release.as_ref().unchecked_ref())?;
    on_release.forget();
    Ok(())
}

/// The canvas's CSS size as laid out, fractions included: at a device pixel
/// ratio of 1.5 a canvas is seldom a whole number of CSS pixels, and the
/// rounded `clientWidth` would size its buffer a pixel short.
fn canvas_css_size(canvas: &HtmlCanvasElement) -> (f64, f64) {
    let rect = canvas.get_bounding_client_rect();
    (rect.width().max(1.0), rect.height().max(1.0))
}

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

fn web_key_modifiers(event: &web_sys::KeyboardEvent) -> cranpose_app_shell::Modifiers {
    cranpose_app_shell::Modifiers {
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    }
}

const WEB_KEY_CODES: &[(&str, cranpose_app_shell::KeyCode)] = {
    use cranpose_app_shell::KeyCode;
    &[
        ("KeyA", KeyCode::A),
        ("KeyB", KeyCode::B),
        ("KeyC", KeyCode::C),
        ("KeyD", KeyCode::D),
        ("KeyE", KeyCode::E),
        ("KeyF", KeyCode::F),
        ("KeyG", KeyCode::G),
        ("KeyH", KeyCode::H),
        ("KeyI", KeyCode::I),
        ("KeyJ", KeyCode::J),
        ("KeyK", KeyCode::K),
        ("KeyL", KeyCode::L),
        ("KeyM", KeyCode::M),
        ("KeyN", KeyCode::N),
        ("KeyO", KeyCode::O),
        ("KeyP", KeyCode::P),
        ("KeyQ", KeyCode::Q),
        ("KeyR", KeyCode::R),
        ("KeyS", KeyCode::S),
        ("KeyT", KeyCode::T),
        ("KeyU", KeyCode::U),
        ("KeyV", KeyCode::V),
        ("KeyW", KeyCode::W),
        ("KeyX", KeyCode::X),
        ("KeyY", KeyCode::Y),
        ("KeyZ", KeyCode::Z),
        ("Digit0", KeyCode::Digit0),
        ("Digit1", KeyCode::Digit1),
        ("Digit2", KeyCode::Digit2),
        ("Digit3", KeyCode::Digit3),
        ("Digit4", KeyCode::Digit4),
        ("Digit5", KeyCode::Digit5),
        ("Digit6", KeyCode::Digit6),
        ("Digit7", KeyCode::Digit7),
        ("Digit8", KeyCode::Digit8),
        ("Digit9", KeyCode::Digit9),
        ("ArrowUp", KeyCode::ArrowUp),
        ("ArrowDown", KeyCode::ArrowDown),
        ("ArrowLeft", KeyCode::ArrowLeft),
        ("ArrowRight", KeyCode::ArrowRight),
        ("Home", KeyCode::Home),
        ("End", KeyCode::End),
        ("PageUp", KeyCode::PageUp),
        ("PageDown", KeyCode::PageDown),
        ("Backspace", KeyCode::Backspace),
        ("Delete", KeyCode::Delete),
        ("Enter", KeyCode::Enter),
        ("NumpadEnter", KeyCode::Enter),
        ("Tab", KeyCode::Tab),
        ("Space", KeyCode::Space),
        ("Escape", KeyCode::Escape),
        ("Minus", KeyCode::Minus),
        ("Equal", KeyCode::Equal),
        ("BracketLeft", KeyCode::BracketLeft),
        ("BracketRight", KeyCode::BracketRight),
        ("Backslash", KeyCode::Backslash),
        ("Semicolon", KeyCode::Semicolon),
        ("Quote", KeyCode::Quote),
        ("Comma", KeyCode::Comma),
        ("Period", KeyCode::Period),
        ("Slash", KeyCode::Slash),
        ("Backquote", KeyCode::Backquote),
    ]
};

fn web_key_code(code: &str) -> cranpose_app_shell::KeyCode {
    WEB_KEY_CODES
        .iter()
        .find(|(name, _)| *name == code)
        .map_or(cranpose_app_shell::KeyCode::Unknown, |(_, key)| *key)
}

fn web_key_event_prefix(
    event: &web_sys::KeyboardEvent,
) -> Option<(cranpose_app_shell::KeyCode, cranpose_app_shell::Modifiers)> {
    if event.is_composing() || event.key_code() == 229 {
        return None;
    }
    Some((web_key_code(&event.code()), web_key_modifiers(event)))
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
        .ok_or_else(|| format!("canvas with id '{canvas_id}' not found"))?
        .dyn_into::<HtmlCanvasElement>()?;

    let scale_factor = window.device_pixel_ratio();
    crate::web_frame_host::follow(&canvas);

    if let Some(html_element) = canvas.dyn_ref::<web_sys::HtmlElement>() {
        let style = html_element.style();
        for (property, value) in
            crate::web_canvas_layout::canvas_inline_styles(settings.web_fill_viewport)
        {
            style.set_property(property, value)?;
        }
    }
    let (css_width, css_height) = canvas_css_size(&canvas);
    let width = css_width.round() as u32;
    let height = css_height.round() as u32;
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
        .map_err(|e| format!("failed to create surface: {e:?}"))?;

    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        })
        .await
        .map_err(|e| format!("failed to find suitable adapter: {e:?}"))?;

    let adapter_info = adapter.get_info();
    let render_scale = crate::web_surface_scale::web_canvas_buffer_scale(scale_factor);
    let (buffer_width, buffer_height) =
        crate::web_surface_scale::web_canvas_device_size(css_width, css_height, scale_factor, None);
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
        .map_err(|e| format!("failed to create device: {e:?}"))?;

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
        color_space: wgpu::SurfaceColorSpace::Auto,
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
            queue.present(probe);
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
        "Web canvas css={width}x{height}, buffer={actual_width}x{actual_height}, effective_scale={effective_scale:.2}, device_scale={scale_factor:.2}"
    );

    let fonts = settings.resolve_font_set();
    let mut renderer = WgpuRenderer::with_font_set(fonts);
    renderer.warm_shaders(cranpose_liquid::shader_warm_ups());
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
        (css_width as f32, css_height as f32),
        effective_scale as f32,
    )));
    app.borrow_mut().set_semantics_enabled(true);
    crate::accessibility::install_inspector(&mut app.borrow_mut(), settings.developer_inspector);
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
    crate::web_host_surface::wake_with(request_frame.clone());
    app.borrow_mut().set_frame_waker({
        let request_frame = request_frame.clone();
        move || request_frame()
    });

    crate::web_clipboard::install(&app, request_frame.clone());

    crate::web_drop::install(&canvas, request_frame.clone())?;

    crate::web_power::start_battery_probe(request_frame.clone());

    app.borrow_mut()
        .set_platform_text_input(accessibility.borrow().text_input_handler());

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
    crate::web_accessibility_options::watch(&window, app.clone(), request_frame.clone());
    crate::web_lifecycle::watch(&window, request_frame.clone());

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
            use cranpose_app_shell::{KeyEvent, KeyEventType};

            let Some((key_code, modifiers)) = web_key_event_prefix(&event) else {
                return;
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
            use cranpose_app_shell::{KeyEvent, KeyEventType};

            let Some((key_code, modifiers)) = web_key_event_prefix(&event) else {
                return;
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

    let floating_size_owed: Rc<Cell<Option<(f32, f32)>>> = Rc::new(Cell::new(None));
    let device_size: Rc<Cell<Option<(u32, u32)>>> = Rc::new(Cell::new(None));

    let reshape: ReshapeFn = {
        let canvas = canvas.clone();
        let window = window.clone();
        let app = app.clone();
        let surface = surface.clone();
        let surface_config = surface_config.clone();
        let surface_dirty = surface_dirty.clone();
        let request_frame = request_frame.clone();
        let floating_size_owed = floating_size_owed.clone();
        let device_size = device_size.clone();
        Rc::new(move |requested: Option<(f32, f32)>| {
            if let Some(size) = requested {
                apply_requested_canvas_size(&canvas, &window, size, &floating_size_owed);
            }
            let host = canvas_window(&canvas, &window);

            let scale_factor = host.device_pixel_ratio();
            let (width, height) = canvas_css_size(&canvas);
            let (buffer_width, buffer_height) = crate::web_surface_scale::web_canvas_device_size(
                width,
                height,
                scale_factor,
                device_size.get(),
            );
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

    crate::web_host_surface::publish(css_width as f32, css_height as f32, effective_scale as f32);

    resize_owed_on_release(&canvas, &window, floating_size_owed)?;
    let canvas_watch = Rc::new(crate::web_canvas_watch::CanvasWatch::new(
        canvas.clone(),
        {
            let reshape = reshape.clone();
            Rc::new(move || reshape(None))
        },
        device_size,
    ));
    canvas_watch.follow();

    let render_loop_for_deadline = render_loop.clone();
    let request_frame_for_loop = request_frame.clone();
    let document_for_loop = document.clone();
    let cursors_for_loop = RefCell::new(crate::web_cursor::WebCursors::new(
        &canvas,
        settings.custom_cursor_size,
    ));
    let canvas_for_cursor = canvas.clone();
    let reshape_for_loop = reshape.clone();

    *render_loop.borrow_mut() = Some(Closure::wrap(Box::new(move || {
        frame_pending.set(false);
        canvas_watch.follow();
        let update_result = app.borrow_mut().update();
        // A size the app asked for in this update is applied in the same
        // frame, while the gesture that led to it still counts as one: a
        // floating window only resizes itself in answer to one.
        if let Some(size) = crate::web_host_surface::take_requested_size() {
            reshape_for_loop(Some(size));
        }
        crate::web_cursor::sync_pointer_icon(
            &cursors_for_loop,
            &document_for_loop,
            &canvas_for_cursor,
            app.borrow().take_pointer_icon_change(),
        );
        if let Ok(mut app_mut) = app.try_borrow_mut()
            && let Err(error) = accessibility
                .borrow_mut()
                .sync(&document_for_loop, &mut app_mut)
        {
            log::error!("web accessibility sync failed: {error:?}");
        }

        let present_required = surface_present_required(
            surface_dirty.get(),
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
                            &output.texture,
                            &view,
                            render_width,
                            render_height,
                        ) {
                            log::error!("render failed: {err:?}");
                        }
                        app_mut.renderer().present(output);
                    }

                    surface_dirty.set(false);
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
                    surface_dirty.set(true);
                    request_frame_for_loop();
                }
                SurfaceFrame::Skip => surface_dirty.set(true),
            }
        }

        let frame_driver = WebPlatformFrameDriver {
            frame_timer: &frame_timer,
            frame_pending: &frame_pending,
            render_loop: &render_loop_for_deadline,
        };
        app.borrow().schedule_platform_frame(&frame_driver);
    }) as Box<dyn FnMut()>));

    request_frame();

    Ok(())
}

fn request_animation_frame(window: &web_sys::Window, f: &Closure<dyn FnMut()>) -> bool {
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
    let Some(host) = crate::web_frame_host::window() else {
        log::error!("requestAnimationFrame unavailable: browser window is not available");
        return;
    };
    if frame_pending.replace(true) && crate::web_frame_host::asked_of(&host) {
        return;
    }
    let render_loop = render_loop.borrow();
    let Some(render_loop) = render_loop.as_ref() else {
        frame_pending.set(false);
        return;
    };
    if request_animation_frame(&host, render_loop) {
        crate::web_frame_host::note_asked(&host);
    } else {
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

    let Some(window) = crate::web_frame_host::window() else {
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
