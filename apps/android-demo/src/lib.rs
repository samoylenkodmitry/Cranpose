// Composition is no longer needed since we use AppShell
// use compose_core::Composition;
use compose_ui::{
    composable, Brush, Button, Color, Column, ColumnSpec, CornerRadii, LinearArrangement,
    Modifier, Row, RowSpec, Size, Spacer, Text, VerticalAlignment,
};

#[composable]
fn combined_app() {
    let selected_tab = compose_core::useState(|| 0);

    Column(
        Modifier::empty()
            .padding(16.0)
            .then(Modifier::empty().background(Color(0.05, 0.07, 0.15, 1.0)))
            .then(Modifier::empty().padding(12.0)),
        ColumnSpec::default(),
        {
            let selected_tab = selected_tab.clone();
            move || {
                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                match selected_tab.get() {
                    0 => counter_example(),
                    1 => grid_example(),
                    _ => counter_example(),
                }
            }
        },
    );
}

#[composable]
fn counter_example() {
    let count_state = compose_core::useState(|| 0i32);

    Column(
        Modifier::empty()
            .padding(32.0)
            .then(Modifier::empty().background(Color(0.08, 0.10, 0.18, 1.0)))
            .then(Modifier::empty().rounded_corners(24.0))
            .then(Modifier::empty().padding(20.0)),
        ColumnSpec::default(),
        {
            let count_state = count_state.clone();
            move || {
                Text(
                    "Compose Counter (Android)",
                    Modifier::empty()
                        .padding(12.0)
                        .then(Modifier::empty().background(Color(1.0, 1.0, 1.0, 0.08)))
                        .then(Modifier::empty().rounded_corners(16.0)),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 24.0,
                });

                let count = count_state.get();
                Text(
                    format!("Count: {}", count),
                    Modifier::empty()
                        .padding(12.0)
                        .then(Modifier::empty().background(Color(0.12, 0.16, 0.28, 0.8)))
                        .then(Modifier::empty().rounded_corners(12.0)),
                );

                Spacer(Size {
                    width: 0.0,
                    height: 16.0,
                });

                Row(
                    Modifier::empty().fill_max_width().then(Modifier::empty().padding(8.0)),
                    RowSpec::new()
                        .horizontal_arrangement(LinearArrangement::SpacedBy(12.0))
                        .vertical_alignment(VerticalAlignment::CenterVertically),
                    {
                        let count_state = count_state.clone();
                        move || {
                            Button(
                                Modifier::empty()
                                    .rounded_corners(16.0)
                                    .then(Modifier::empty().draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.35, 0.45, 0.85, 1.0)),
                                            CornerRadii::uniform(16.0),
                                        );
                                    }))
                                    .then(Modifier::empty().padding(10.0)),
                                {
                                    let count_state = count_state.clone();
                                    move || {
                                        count_state.set(count_state.get() + 1);
                                    }
                                },
                                || {
                                    Text("Increment", Modifier::empty().padding(6.0));
                                },
                            );

                            Button(
                                Modifier::empty()
                                    .rounded_corners(16.0)
                                    .then(Modifier::empty().draw_behind(|scope| {
                                        scope.draw_round_rect(
                                            Brush::solid(Color(0.65, 0.35, 0.35, 1.0)),
                                            CornerRadii::uniform(16.0),
                                        );
                                    }))
                                    .then(Modifier::empty().padding(10.0)),
                                {
                                    let count_state = count_state.clone();
                                    move || {
                                        count_state.set(count_state.get() - 1);
                                    }
                                },
                                || {
                                    Text("Decrement", Modifier::empty().padding(6.0));
                                },
                            );
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn grid_example() {
    Text(
        "Grid example - Coming soon!",
        Modifier::empty()
            .padding(12.0)
            .then(Modifier::empty().background(Color(0.12, 0.16, 0.28, 0.8)))
            .then(Modifier::empty().rounded_corners(12.0)),
    );
}

// Android entry point using android-activity
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: android_activity::AndroidApp) {
    use android_activity::{InputStatus, MainEvent, PollEvent};
    use compose_app_shell::{default_root_key, AppShell};
    use compose_render_wgpu::WgpuRenderer;
    use std::sync::Arc;

    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)  // Reduce log spam
            .with_tag("ComposeRS")
            .with_filter(
                android_logger::FilterBuilder::new()
                    .filter_level(log::LevelFilter::Info)
                    .filter_module("wgpu_core", log::LevelFilter::Warn)
                    .filter_module("wgpu_hal", log::LevelFilter::Warn)
                    .filter_module("naga", log::LevelFilter::Warn)
                    .build()
            ),
    );

    log::info!("Starting Compose-RS Android Demo");

    // Initialize wgpu instance
    // ANDROID EMULATOR FIX ATTEMPT: Try GLES backend instead of Vulkan
    // Emulators often have issues with Vulkan passthrough, but GLES works better
    #[cfg(target_os = "android")]
    let backends = {
        log::info!("Android detected - trying GL backend first for emulator compatibility");
        wgpu::Backends::GL | wgpu::Backends::VULKAN
    };
    #[cfg(not(target_os = "android"))]
    let backends = wgpu::Backends::all();

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..Default::default()
    });

    let mut surface_state: Option<(
        wgpu::Surface<'static>,
        Arc<wgpu::Device>,
        Arc<wgpu::Queue>,
        wgpu::SurfaceConfiguration,
        AppShell<WgpuRenderer>,
    )> = None;

    let mut window_size = (0u32, 0u32);
    let mut needs_redraw = false;

    // Main event loop - process events quickly, render outside callback
    loop {
        // Poll events with very short timeout to avoid blocking input
        app.poll_events(Some(std::time::Duration::from_millis(1)), |event| {
            match event {
                PollEvent::Main(main_event) => match main_event {
                    MainEvent::InitWindow { .. } => {
                        log::info!("Window initialized, setting up rendering");

                        // Get the native window
                        if let Some(native_window) = app.native_window() {
                            use raw_window_handle::{
                                AndroidDisplayHandle, AndroidNdkWindowHandle,
                                RawDisplayHandle, RawWindowHandle,
                            };

                            // Create surface from the Android window using raw window handle
                            let surface = unsafe {
                                // Manually create raw handles for Android
                                let window_handle = AndroidNdkWindowHandle::new(
                                    std::ptr::NonNull::new(native_window.ptr().as_ptr() as *mut _)
                                        .expect("Null window pointer"),
                                );
                                let display_handle = AndroidDisplayHandle::new();

                                let raw_window_handle = RawWindowHandle::AndroidNdk(window_handle);
                                let raw_display_handle = RawDisplayHandle::Android(display_handle);

                                let target = wgpu::SurfaceTargetUnsafe::RawHandle {
                                    raw_display_handle,
                                    raw_window_handle,
                                };

                                instance
                                    .create_surface_unsafe(target)
                                    .expect("Failed to create surface")
                            };

                            // Request adapter
                            let adapter = pollster::block_on(instance.request_adapter(
                                &wgpu::RequestAdapterOptions {
                                    power_preference: wgpu::PowerPreference::HighPerformance,
                                    compatible_surface: Some(&surface),
                                    force_fallback_adapter: false,
                                },
                            ))
                            .expect("Failed to find suitable adapter");

                            let adapter_info = adapter.get_info();
                            log::info!("Found adapter: {:?}", adapter_info);
                            log::info!("  Backend: {:?}", adapter_info.backend);
                            log::info!("  Device: {}", adapter_info.device);
                            log::info!("  Vendor: {}", adapter_info.vendor);
                            log::info!("  Driver: {} ({})", adapter_info.driver, adapter_info.driver_info);

                            // Request device and queue
                            let (device, queue) = pollster::block_on(adapter.request_device(
                                &wgpu::DeviceDescriptor {
                                    label: Some("Android Device"),
                                    required_features: wgpu::Features::empty(),
                                    required_limits: wgpu::Limits::default(),
                                },
                                None,
                            ))
                            .expect("Failed to create device");

                            let device = Arc::new(device);
                            let queue = Arc::new(queue);

                            // Get surface capabilities and format
                            let surface_caps = surface.get_capabilities(&adapter);
                            let surface_format = surface_caps
                                .formats
                                .iter()
                                .copied()
                                .find(|f| f.is_srgb())
                                .unwrap_or(surface_caps.formats[0]);

                            log::info!("Surface format: {:?}", surface_format);

                            // Configure surface
                            let (width, height) = window_size;
                            let width = width.max(1);
                            let height = height.max(1);

                            let surface_config = wgpu::SurfaceConfiguration {
                                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                                format: surface_format,
                                width,
                                height,
                                present_mode: wgpu::PresentMode::Fifo,
                                alpha_mode: surface_caps.alpha_modes[0],
                                view_formats: vec![],
                                desired_maximum_frame_latency: 2,
                            };

                            surface.configure(&device, &surface_config);

                            // Create renderer
                            let mut renderer = WgpuRenderer::new();
                            renderer.init_gpu(device.clone(), queue.clone(), surface_format);

                            // Create app shell with our combined_app
                            let mut app_shell = AppShell::new(renderer, default_root_key(), || {
                                combined_app();
                            });

                            app_shell.set_buffer_size(width, height);
                            app_shell.set_viewport(width as f32, height as f32);

                            surface_state = Some((surface, device, queue, surface_config, app_shell));

                            log::info!("Rendering initialized successfully");
                        }
                    }
                    MainEvent::TerminateWindow { .. } => {
                        log::info!("Window terminated");
                        surface_state = None;
                    }
                    MainEvent::WindowResized { .. } => {
                        if let Some(native_window) = app.native_window() {
                            let width = native_window.width() as u32;
                            let height = native_window.height() as u32;
                            window_size = (width, height);

                            log::info!("Window resized to {}x{}", width, height);

                            if let Some((surface, device, _, surface_config, app_shell)) =
                                &mut surface_state
                            {
                                if width > 0 && height > 0 {
                                    surface_config.width = width;
                                    surface_config.height = height;
                                    surface.configure(device, surface_config);
                                    app_shell.set_buffer_size(width, height);
                                    app_shell.set_viewport(width as f32, height as f32);
                                }
                            }
                        }
                    }
                    MainEvent::RedrawNeeded { .. } => {
                        needs_redraw = true;
                    }
                    _ => {}
                },
                // CRITICAL: Handle input events to prevent ANR
                // Android requires us to consume input events or they queue up and timeout
                _ => {
                    // Process and consume all pending input events
                    if let Ok(mut iter) = app.input_events_iter() {
                        loop {
                            // next() returns true if there was an event, false if queue is empty
                            if !iter.next(|_event| {
                                // For now, just consume the event to prevent ANR
                                // TODO: Actually handle touch events and trigger redraws
                                needs_redraw = true;
                                InputStatus::Handled
                            }) {
                                break; // No more events
                            }
                        }
                    }
                }
            }
        });

        // Do actual rendering outside the event callback to avoid blocking input
        if needs_redraw && surface_state.is_some() {
            if let Some((surface, _, _, _, app_shell)) = &mut surface_state {
                // Always update and render to ensure continuous display
                app_shell.update();

                match surface.get_current_texture() {
                    Ok(frame) => {
                        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
                        let (width, height) = app_shell.buffer_size();

                        if let Err(e) = app_shell.renderer().render(&view, width, height) {
                            log::error!("Render error: {:?}", e);
                        }

                        frame.present();
                    }
                    Err(wgpu::SurfaceError::Lost) => {
                        log::warn!("Surface lost, will be reconfigured");
                    }
                    Err(wgpu::SurfaceError::OutOfMemory) => {
                        log::error!("Out of memory!");
                        break;
                    }
                    Err(e) => {
                        log::debug!("Surface error: {:?}", e);
                    }
                }
            }
            needs_redraw = false;
        }

        // Small sleep to avoid busy loop when not rendering
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
}
