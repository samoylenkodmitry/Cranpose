use compose_core::Composition;
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

// Android JNI entry point
#[cfg(target_os = "android")]
#[no_mangle]
pub extern "C" fn android_main(app: ndk::native_activity::NativeActivity) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Debug)
            .with_tag("ComposeRS"),
    );

    log::info!("Starting Compose-RS Android Demo");

    // Set the Android context for ndk-context
    // SAFETY: This is called once at app startup with valid JNI pointers from NativeActivity
    unsafe {
        ndk_context::initialize_android_context(app.vm().cast(), app.activity().cast());
    }

    // For now, just log that we started and keep the app running
    // Full Android integration with wgpu surface will be implemented later
    log::info!("Android app initialized successfully");
    log::info!("Note: Full UI rendering support is work in progress");

    // TODO: Implement proper Android event loop and wgpu surface creation
    // This will require:
    // 1. Creating an Android surface from the NativeActivity window
    // 2. Setting up wgpu with the Android surface
    // 3. Implementing the render loop with Android lifecycle events
    // 4. Handling input events from the native activity

    // For now, just run the composable function once to verify it compiles
    // In a real implementation, this would be called in the render loop
    use compose_core::{location_key, MemoryApplier};

    let mut composition = Composition::new(MemoryApplier::new());
    if let Err(err) = composition.render(location_key(file!(), line!(), column!()), || {
        combined_app();
    }) {
        log::error!("Composition render failed: {err}");
    }

    log::info!("Initial composition completed");

    // Keep the activity alive
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

// Export the android_main for NativeActivity
#[cfg(target_os = "android")]
pub use ndk::native_activity::*;
