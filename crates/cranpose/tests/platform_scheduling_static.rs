use std::path::Path;
use std::path::PathBuf;

fn crate_source(path: &str) -> String {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(crate_dir.join(path)).expect("failed to read cranpose source file")
}

fn workspace_source(path: &str) -> String {
    std::fs::read_to_string(workspace_path(path)).expect("failed to read workspace source file")
}

fn workspace_path(path: &str) -> PathBuf {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    workspace_dir.join(path)
}

#[test]
fn ci_architecture_budget_runs_required_gates() {
    let workflow = workspace_source(".github/workflows/rust.yml");
    let release_workflow = workspace_source(".github/workflows/release.yml");
    let pages_workflow = workspace_source(".github/workflows/deploy-pages.yml");

    assert!(
        workflow.contains("architecture-budget:")
            && workflow.contains("linux / stable / architecture budgets"),
        "Rust CI should keep a dedicated architecture budget job"
    );
    assert!(
        workflow.contains("cargo build --workspace --no-default-features"),
        "architecture budget job should prove the workspace builds with default features disabled"
    );
    assert!(
        workflow.contains("cargo check --workspace --all-features"),
        "architecture budget job should prove the all-features graph still type-checks"
    );
    assert!(
        workflow.contains("cargo xtask dependency-budget --explain"),
        "architecture budget job should print duplicate dependency owner details"
    );
    assert!(
        workflow.contains("cargo xtask dependency-budget --strict --explain")
            && workflow
                .contains("cargo xtask dependency-budget --strict --slice desktop-platform --explain")
            && workflow.contains(
                "cargo xtask dependency-budget --strict --slice optional-features --explain"
            ),
        "architecture budget job should enforce full strict zero duplicates and keep focused clean-slice diagnostics"
    );
    assert!(
        workflow.contains("cargo xtask binary-size")
            && workflow.contains("--package isolated-demo")
            && workflow.contains("--bin isolated-demo")
            && workflow.contains("--profile release-small")
            && workflow.contains("--max-bytes 29360128"),
        "architecture budget job should enforce the minimal release-small binary size ceiling"
    );
    assert!(
        workflow.contains("wasm-build:")
            && workflow.contains("Install binaryen 120")
            && workflow.contains("cargo install wasm-pack --version 0.13.1")
            && workflow.contains("run: apps/desktop-demo/build-web.sh --release"),
        "Rust CI should keep the web release build job wired through explicit build-web.sh --release so the wasm-release profile and WASM size budget cannot depend on ambient CI defaults"
    );
    assert!(
        pages_workflow.contains("Deploy to GitHub Pages")
            && pages_workflow.contains("Install binaryen (wasm-opt) for size optimization")
            && pages_workflow.contains("cargo install wasm-pack --version 0.13.1")
            && pages_workflow.contains("./build-web.sh --release")
            && pages_workflow.contains("actions/upload-pages-artifact@v5"),
        "GitHub Pages deployment must publish the same budgeted optimized WASM produced by build-web.sh --release"
    );
    assert!(
        !workflow.contains("android-actions/setup-android")
            && !release_workflow.contains("android-actions/setup-android"),
        "Android CI should install only required SDK packages instead of running the broad setup-android action"
    );
    assert!(
        workflow.contains("bash scripts/ci/install_android_ndk.sh 27.0.12077973")
            && release_workflow.contains("bash scripts/ci/install_android_ndk.sh 27.0.12077973"),
        "Android CI and release workflows should share the narrow NDK installer"
    );
}

#[test]
fn render_common_package_embeds_crate_owned_text_assets() {
    let software_text_source =
        workspace_source("crates/cranpose-render/common/src/software_text_raster.rs");
    let font_layout_source = workspace_source("crates/cranpose-render/common/src/font_layout.rs");
    let wgpu_lib_source = workspace_source("crates/cranpose-render/wgpu/src/lib.rs");
    let wgpu_test_support_source = workspace_source("crates/cranpose-render/wgpu/tests/support.rs");

    for (path, source) in [
        (
            "crates/cranpose-render/common/src/software_text_raster.rs",
            software_text_source.as_str(),
        ),
        (
            "crates/cranpose-render/common/src/font_layout.rs",
            font_layout_source.as_str(),
        ),
        (
            "crates/cranpose-render/wgpu/src/lib.rs",
            wgpu_lib_source.as_str(),
        ),
        (
            "crates/cranpose-render/wgpu/tests/support.rs",
            wgpu_test_support_source.as_str(),
        ),
    ] {
        assert!(
            !source.contains("apps/desktop-demo/assets"),
            "{path} must not embed demo-app assets; library crates must package their own fallback fonts"
        );
    }

    for path in [
        "crates/cranpose-render/common/assets/NotoSansMerged.ttf",
        "crates/cranpose-render/common/assets/NotoSansBold.ttf",
        "crates/cranpose-render/common/assets/TwemojiMozilla.ttf",
    ] {
        let metadata = std::fs::metadata(workspace_path(path)).unwrap_or_else(|error| {
            panic!("{path} should be packaged with render-common: {error}")
        });
        assert!(
            metadata.len() > 1024,
            "{path} should contain the fallback font bytes"
        );
    }
}

#[test]
fn app_shell_frame_schedule_targets_platform_frame_driver() {
    let source = workspace_source("crates/cranpose-app-shell/src/lib.rs");

    assert!(
        source.contains("pub trait PlatformFrameDriver")
            && source.contains("pub struct FrameScheduler"),
        "AppShell scheduling should expose a scheduler and platform driver boundary"
    );
    assert!(
        source.contains("impl FrameSchedule")
            && source.contains("pub fn apply_to<D>(self, driver: &D)")
            && source.contains("pub fn schedule<D>(&self, schedule: FrameSchedule, driver: &D)")
            && source.contains("pub fn schedule_platform_frame<D>(&self, driver: &D)")
            && source.contains("self.frame_scheduler.schedule(schedule, driver)")
            && source.contains("driver.request_frame()")
            && source.contains("driver.request_wake_at(deadline)")
            && source.contains("driver.clear_wake()"),
        "FrameSchedule should be interpreted through the AppShell-owned scheduler and platform driver contract"
    );
}

#[test]
fn desktop_no_vsync_chains_dirty_presented_frames_only() {
    let source = crate_source("src/desktop.rs");

    assert!(
        source.contains(
            "fn should_chain_no_vsync_redraw(frame_interval: Option<Duration>, needs_frame: bool) -> bool"
        ) && source.contains("frame_interval.is_none() && needs_frame"),
        "desktop no-vsync frame chaining must require both an uncapped present mode and pending frame work"
    );
    assert!(
        source.contains(
            "if should_chain_no_vsync_redraw(\n                        frame_interval,\n                        app.frame_schedule().needs_frame,"
        )
            && source.contains("request_redraw_once(window, &mut self.primary_redraw_pending);"),
        "primary desktop frames should request the next no-vsync redraw immediately after presenting a dirty frame"
    );
    assert!(
        source.contains(
            "native.frame_interval(),\n            native.app.frame_schedule().needs_frame"
        ) && source.contains("native.window.request_redraw();"),
        "native desktop frames should use the same no-vsync redraw chaining rule"
    );
}

#[test]
fn surface_present_decision_is_shared_across_platform_loops() {
    // The first-present / warmup decision lives once in the shared wgpu_surface
    // module so the desktop, web, and android render loops cannot drift apart
    // (the web loop "white until scroll" bug was a desktop/web divergence).
    let shared = crate_source("src/wgpu_surface.rs");
    assert!(
        shared.contains("pub(crate) fn surface_present_required(")
            && shared.contains("surface_dirty || update_visual_changed || app_needs_redraw"),
        "the shared wgpu_surface module must own the single surface present decision"
    );
}

#[test]
fn desktop_renderer_warmup_reaches_primary_and_native_surfaces() {
    let source = crate_source("src/desktop.rs");

    assert!(
        source.contains(
            "surface_present_required(\n            native.surface_dirty,\n            update_result.visual_changed,\n            native.app.needs_redraw(),"
        ),
        "native windows must still render when renderer-side warmup is the only pending frame work"
    );
    assert!(
        source.contains(
            "surface_present_required(\n                    primary_surface_dirty_before_update || robot_surface_dirty_before_update,\n                    update_result.visual_changed,\n                    app.needs_redraw(),"
        ),
        "primary windows must not skip a redraw requested only by renderer-side warmup"
    );
}

#[test]
fn web_first_frame_is_forced_through_surface_dirty() {
    // Regression guard for the "white until scroll" bug: the web render loop must
    // present the scene built during construction on the first frame even though
    // `update()` reports no visual work, by starting `surface_dirty` true and
    // only clearing it after a successful present.
    let source = crate_source("src/web.rs");

    assert!(
        source.contains("let surface_dirty = Rc::new(Cell::new(true));"),
        "web surface_dirty must start true so the first frame is always presented"
    );
    assert!(
        source.contains(
            "let present_required = surface_present_required(\n            surface_dirty_for_loop.get(),\n            update_result.visual_changed,\n            app.borrow().needs_redraw(),\n        );"
        ),
        "web render loop must gate the present through the shared surface_present_required helper"
    );
    assert!(
        source.contains("surface_dirty_for_loop.set(false);"),
        "web surface_dirty must be cleared only after a successful present"
    );
}

#[test]
fn android_first_frame_is_forced_through_surface_dirty() {
    // The android render loop shares the desktop/web first-present contract: the
    // surface starts dirty and is cleared only after a successful present.
    let source = crate_source("src/android.rs");

    assert!(
        source.contains("surface_dirty: true,"),
        "android GpuResources must start with a dirty surface so the first frame presents"
    );
    assert!(
        source.contains(
            "if surface_present_required(\n                    resources.surface_dirty,\n                    update_result.visual_changed,\n                    shell.needs_redraw(),\n                )"
        ),
        "android render loop must gate the present through the shared surface_present_required helper"
    );
    assert!(
        source.contains("resources.surface_dirty = false;"),
        "android surface_dirty must be cleared only after a successful present"
    );
}

#[test]
fn web_idle_does_not_request_recursive_raf() {
    let source = crate_source("src/web.rs");

    assert!(
        source.contains("struct WebPlatformFrameDriver")
            && source.contains("impl PlatformFrameDriver for WebPlatformFrameDriver"),
        "web runtime should own a concrete platform frame driver"
    );
    assert!(
        !source.contains("request_animation_frame(render_loop.borrow().as_ref().unwrap())"),
        "web runtime must not recursively request RAF every frame"
    );
    assert!(
        source.contains("app.borrow().schedule_platform_frame(&frame_driver)")
            && source.contains("request_web_frame_at_deadline")
            && source.contains("clear_web_frame_wake")
            && source.contains("set_timeout_with_callback_and_timeout_and_arguments_0"),
        "web runtime should translate idle frame deadlines into timeout-driven one-shot RAF requests"
    );
}

#[test]
fn web_frame_request_scheduling_does_not_panic_on_browser_api_failures() {
    let source = crate_source("src/web.rs");
    let start = source
        .find("fn request_animation_frame")
        .expect("web frame scheduling helper should exist");
    let end = source
        .find("fn clear_web_frame_wake")
        .expect("web frame wake clearer should exist");
    let scheduling_source = &source[start..end];

    assert!(
        !scheduling_source.contains(".unwrap()") && !scheduling_source.contains(".expect("),
        "web frame scheduling should log and clear pending state instead of panicking on browser API failures"
    );
}

#[test]
fn web_frame_waker_is_shell_owned_without_thread_local_router() {
    let web_source = crate_source("src/web.rs");
    let app_shell_source = workspace_source("crates/cranpose-app-shell/src/lib.rs");

    assert!(
        !web_source.contains("WEB_FRAME_REQUESTER")
            && !web_source.contains("install_web_frame_requester")
            && !web_source.contains("request_current_web_frame"),
        "web frame wakeups must not route through a process-global/thread-local requester"
    );
    assert!(
        app_shell_source
            .contains("#[cfg(target_arch = \"wasm32\")]\n    pub fn set_frame_waker(&mut self, waker: impl Fn() + 'static)"),
        "wasm AppShell frame wakers should be single-threaded instead of requiring Send"
    );
    assert!(
        web_source.contains("app.borrow_mut().set_frame_waker({")
            && web_source.contains("move || request_frame()"),
        "web runtime should install the per-shell frame requester directly on AppShell"
    );
}

#[test]
fn web_surface_capabilities_are_checked_before_indexing() {
    let source = crate_source("src/web.rs");

    assert!(
        !source.contains("surface_caps.formats[0]")
            && !source.contains("surface_caps.alpha_modes[0]"),
        "web renderer startup should return an error for empty surface capabilities instead of indexing directly"
    );
}

#[test]
fn native_surface_capabilities_are_checked_before_indexing() {
    for path in ["src/android.rs", "src/desktop.rs"] {
        let source = crate_source(path);

        assert!(
            !source.contains("surface_caps.formats[0]")
                && !source.contains("surface_caps.alpha_modes[0]"),
            "{path} should return typed errors for empty surface capabilities instead of indexing directly"
        );
    }
}

#[test]
fn platform_surface_reconfigure_uses_fallible_renderer_device_access() {
    for path in ["src/desktop.rs", "src/web.rs"] {
        let source = crate_source(path);
        assert!(
            !source.contains(".renderer().device()"),
            "{path} should not panic on surface reconfiguration when renderer GPU state is unavailable"
        );
        assert!(
            source.contains(".renderer().try_device()"),
            "{path} should use fallible renderer device access for surface reconfiguration"
        );
    }
}

#[test]
fn desktop_initial_shell_render_enters_native_window_registry() {
    let source = crate_source("src/desktop.rs");

    assert!(
        source.contains("let mut app = native_window::with_native_window_registry(&registry, || {")
            && source.contains("AppShell::new_with_size_and_density("),
        "desktop run_windows uses a hidden primary declaration host, so AppShell construction must enter the native-window registry before the first stable render"
    );
}

#[test]
fn android_idle_does_not_poll_16ms() {
    let source = crate_source("src/android.rs");

    assert!(
        source.contains("app_waker.wake()"),
        "android runtime frame waker should wake the Android looper"
    );
    assert!(
        !source.contains("Duration::from_millis(16)") && !source.contains("from_millis(16)"),
        "android runtime must not poll at 16 ms while idle"
    );
    assert!(
        source.contains("struct AndroidFrameDriver")
            && source.contains("impl PlatformFrameDriver for AndroidFrameDriver")
            && source.contains("shell.schedule_platform_frame(&android_frame_driver)")
            && source.contains("android_frame_driver.deadline_timeout()")
            && source.contains("earliest_android_poll_timeout"),
        "android runtime should route AppShell schedules through the platform frame driver"
    );
}

#[test]
fn android_overlay_events_are_runtime_owned() {
    let overlay_source = crate_source("src/android_overlay_window.rs");
    let jni_source = crate_source("src/android_jni.rs");
    let java_source = workspace_source(
        "crates/cranpose/android/java/dev/cranpose/android/CranposeOverlayWindow.java",
    );
    let runtime_source = crate_source("src/android.rs");

    assert!(
        overlay_source.contains("pub(crate) struct AndroidOverlayEventQueue")
            && overlay_source.contains("pub(crate) struct AndroidOverlayEventQueueHandle")
            && overlay_source.contains("retain_android_overlay_event_queue_handle"),
        "Android overlay callbacks should route through an explicit handle to a runtime-owned event queue"
    );
    assert!(
        !overlay_source.contains("OnceLock<Mutex<VecDeque<AndroidOverlayWindowEvent>>>")
            && !overlay_source.contains("fn overlay_events() -> &'static Mutex<VecDeque")
            && !overlay_source.contains("OnceLock<")
            && !overlay_source.contains("register_android_overlay_event_queue")
            && !overlay_source.contains("lock_overlay_event_queue_slot"),
        "Android overlay events and helper classes must not be retained in process-global Rust storage"
    );
    assert!(
        jni_source.contains("nativeOverlayReleaseQueue")
            && jni_source.contains("push_overlay_event_for_handle"),
        "Android JNI callbacks should release and dispatch explicit overlay queue handles"
    );
    assert!(
        java_source.contains("long eventQueueHandle")
            && java_source.contains("nativeOverlayReleaseQueue")
            && java_source.contains("nativeOverlaySurfaceChanged(eventQueueHandle"),
        "Android overlay Java helper should carry the runtime queue handle through callbacks"
    );
    assert!(
        runtime_source.contains("let overlay_event_queue = Arc::new")
            && !runtime_source.contains("let _overlay_event_queue_registration =")
            && runtime_source.contains("drain_android_overlay_window_events(&overlay_event_queue)"),
        "Android runtime should own and explicitly drain overlay events"
    );
    assert!(
        overlay_source.contains("jni_str!(\"getClassLoader\")")
            && !overlay_source.contains("jni_str!(\"getClass\")"),
        "Android Java bridge loading must use the Activity context classloader; android.app.NativeActivity itself is framework-loaded by the boot classloader"
    );
}

#[test]
fn android_activity_jni_attaches_the_caller_without_recreating_the_vm() {
    let jni_source = crate_source("src/android_jni.rs");

    assert!(
        jni_source.contains("JavaVM::singleton()")
            && jni_source.contains("vm.attach_current_thread")
            && jni_source.contains("env.as_cast_raw::<JObject>")
            && jni_source.contains("env.new_local_ref"),
        "Android activity JNI access must reach the activity through the process JavaVM singleton and attach the calling thread (cheap when android_main is already attached, required when called from a worker thread such as audio playback opening a content:// document), creating a scoped local Activity reference from the global Activity handle"
    );
    assert!(
        !jni_source.contains("JavaVM::from_raw(app.vm_as_ptr"),
        "Android activity JNI access must not recreate the JavaVM from AndroidApp; it must reuse the JavaVM singleton"
    );
}

#[test]
fn android_native_input_is_drained_on_input_available_event() {
    let source = crate_source("src/android.rs");

    assert!(
        source.contains("MainEvent::InputAvailable")
            && source.contains("drain_android_input_events(")
            && source.contains("pending_input_from_android_event(")
            && source.contains("android_activity::InputStatus::Handled"),
        "Android NativeActivity input must be drained from MainEvent::InputAvailable so every input event reaches finish_event before the platform ANR timeout"
    );
    assert!(
        !source.contains("println!(\n                                                    \"[TOUCH]")
            && !source.contains("println!(\"[TOUCH]"),
        "Android input acknowledgement must not perform synchronous stdout logging in the event-finish path"
    );
}

#[test]
fn android_host_window_layout_is_dispatched_on_java_ui_thread() {
    let runtime_source = crate_source("src/android.rs");
    let java_source = workspace_source(
        "crates/cranpose/android/java/dev/cranpose/android/CranposeOverlayWindow.java",
    );

    assert!(
        runtime_source.contains("setActivityWindowLayout")
            && runtime_source.contains("find_android_overlay_class")
            && !runtime_source.contains("jni_str!(\"setLayout\")"),
        "Android host-window layout requests must go through the Java bridge instead of touching Window.setLayout from android_main"
    );
    assert!(
        java_source.contains("setActivityWindowLayout")
            && java_source.contains("activity.runOnUiThread")
            && java_source.contains("activity.getWindow().setLayout"),
        "Android Activity window layout changes must execute on the Java UI thread"
    );
}

#[test]
fn platform_drivers_set_density_through_app_shell() {
    for path in ["src/android.rs", "src/desktop.rs", "src/web.rs"] {
        let source = crate_source(path);
        assert!(
            !source.contains("cranpose_ui::set_density("),
            "{path} must update density through AppShell so the per-shell AppContext owns the value"
        );
    }
}

#[test]
fn desktop_frame_cap_deadline_is_option_checked() {
    let source = crate_source("src/desktop.rs");

    assert!(
        !source.contains("native frame cap deadline should exist"),
        "desktop frame pacing should carry frame-cap deadlines through Option instead of panicking"
    );
}

#[test]
fn desktop_x11_client_is_app_owned() {
    let source = crate_source("src/desktop.rs");

    assert!(
        source.contains("native_window_platform_probe: NativeWindowPlatformProbe")
            && source.contains("struct NativeWindowPlatformProbe"),
        "desktop runtime should own native-window platform probing inside App"
    );
    assert!(
        !source.contains("static X11_WINDOW_CLIENT")
            && !source.contains("fn with_x11_window_client<R>"),
        "X11 connection probing must not live in a process/thread-local cache"
    );
}

#[test]
fn ios_backend_is_wired_without_aliasing_desktop() {
    // The cranpose iOS feature is wired to a real winit-based backend: it is no
    // longer reserved, and it does not alias the desktop feature.
    let cranpose_manifest = crate_source("Cargo.toml");
    assert!(
        !cranpose_manifest.contains("ios = []"),
        "cranpose ios feature must be wired to the real backend, not reserved"
    );
    assert!(
        !cranpose_manifest.contains("ios = [\"desktop\"]"),
        "ios must not alias the desktop feature"
    );

    // The facade exposes the backend module instead of an unavailable stub.
    let facade = crate_source("src/lib.rs");
    assert!(
        facade.contains("pub mod ios;"),
        "cranpose must expose the iOS backend module"
    );
    assert!(
        !facade.contains("backend and is unavailable"),
        "the iOS-unavailable compile_error must be gone"
    );

    // The backend drives its own winit UIKit event loop (a real surface, not a
    // reuse of the desktop multi-window runtime).
    let ios = crate_source("src/ios.rs");
    assert!(
        ios.contains("ApplicationHandler") && ios.contains("winit"),
        "ios backend should drive its own winit event loop"
    );

    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");

    // The demo advertises an iOS app feature and ships the iOS entry binary.
    let demo_manifest = std::fs::read_to_string(workspace_dir.join("apps/desktop-demo/Cargo.toml"))
        .expect("failed to read desktop-demo manifest");
    assert!(
        demo_manifest
            .lines()
            .any(|line| line.trim_start().starts_with("ios =")),
        "desktop-demo should advertise an iOS app feature"
    );
    assert!(
        demo_manifest.contains("name = \"cranpose-ios\""),
        "desktop-demo should declare the cranpose-ios binary"
    );

    // The iOS build script builds the ios feature instead of failing.
    let build_script =
        std::fs::read_to_string(workspace_dir.join("apps/ios-demo/ios/build-app.sh"))
            .expect("failed to read ios build script");
    assert!(
        build_script.contains("--features ios"),
        "ios build script should build the ios feature"
    );
}

#[test]
fn wgpu_backend_features_are_target_specific() {
    for manifest in [
        "crates/cranpose/Cargo.toml",
        "crates/cranpose-render/wgpu/Cargo.toml",
    ] {
        let source = workspace_source(manifest);
        assert!(
            !source.contains(
                "[target.'cfg(all(not(target_arch = \"wasm32\"), not(target_os = \"android\")))'.dependencies]"
            ),
            "{manifest} must not use one broad native WGPU backend dependency for every desktop OS"
        );

        let linux = manifest_section(
            &source,
            "[target.'cfg(all(target_os = \"linux\", not(target_arch = \"wasm32\")))'.dependencies]",
        );
        assert!(
            linux.contains("\"gles\"")
                && linux.contains("\"vulkan\"")
                && !linux.contains("\"dx12\"")
                && !linux.contains("\"metal\""),
            "{manifest} Linux WGPU backend set should be GLES/Vulkan only"
        );

        let windows = manifest_section(
            &source,
            "[target.'cfg(target_os = \"windows\")'.dependencies]",
        );
        assert!(
            windows.contains("\"dx12\"")
                && !windows.contains("\"metal\"")
                && !windows.contains("\"gles\"")
                && !windows.contains("\"vulkan\""),
            "{manifest} Windows WGPU backend set should be DX12 only"
        );

        let macos = manifest_section(
            &source,
            "[target.'cfg(target_os = \"macos\")'.dependencies]",
        );
        assert!(
            macos.contains("\"metal\"")
                && !macos.contains("\"dx12\"")
                && !macos.contains("\"gles\"")
                && !macos.contains("\"vulkan\""),
            "{manifest} macOS WGPU backend set should be Metal only"
        );
    }
}

#[test]
fn render_state_has_no_process_global_fallback() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let source =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-ui/src/render_state.rs"))
            .expect("failed to read render_state.rs");

    assert!(
        !source.contains("OnceLock<RenderState>"),
        "render_state fallback must not be a process-global RenderState"
    );
    assert!(
        source.contains("fn require_current_app_context(operation: &str) -> Rc<AppContext>")
            && source.contains("panic!(\"{operation} requires an active AppContext\")")
            && !source.contains("static UNIT_TEST_APP_CONTEXT")
            && !source.contains("Box::leak(Box::new(AppContext::new()))")
            && !source.contains("cfg(any(test, feature = \"test-helpers\"))]\nfn require_current_app_context_without_scope")
            && !source.contains("cfg(not(any(test, feature = \"test-helpers\")))]\nfn require_current_app_context_without_scope")
            && !source.contains("with_fallback_render_state")
            && !source.contains("FALLBACK_"),
        "render_state must route production runtime access through the active AppContext without hidden fallback state"
    );
}

fn manifest_section<'a>(source: &'a str, header: &str) -> &'a str {
    let start = source
        .find(header)
        .unwrap_or_else(|| panic!("manifest section `{header}` is missing"));
    let tail = &source[start + header.len()..];
    let end = tail.find("\n[").unwrap_or(tail.len());
    &tail[..end]
}

#[test]
fn fps_monitor_runtime_state_is_shell_owned() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let source =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-app-shell/src/fps_monitor.rs"))
            .expect("failed to read fps_monitor.rs");

    assert!(
        source.contains("pub(crate) struct FpsMonitor"),
        "fps monitoring state should be owned by an AppShell field"
    );
    assert!(
        !source.contains("static FPS_TRACKER") && !source.contains("static RECOMPOSITION_COUNT"),
        "fps monitor counters must not be authoritative process state"
    );
    assert!(
        !source.contains("PUBLISHED_STATS")
            && !source.contains("pub fn fps_stats()")
            && !source.contains("pub fn current_fps()"),
        "public FPS snapshots must come from the owning AppShell, not from process-global publication"
    );
}

#[test]
fn fps_monitor_counts_presented_frames_not_shell_updates() {
    let shell_frame = workspace_source("crates/cranpose-app-shell/src/shell_frame.rs");
    let app_shell = workspace_source("crates/cranpose-app-shell/src/lib.rs");
    let desktop = workspace_source("crates/cranpose/src/desktop.rs");

    assert!(
        !shell_frame.contains("record_frame_work"),
        "AppShell update processing must not mutate presented-frame FPS stats"
    );
    assert!(
        app_shell.contains("pub fn record_presented_frame"),
        "AppShell should expose an explicit presented-frame sampling boundary"
    );
    assert!(
        desktop.contains("record_presented_frame"),
        "desktop presentation paths should record FPS after real redraws"
    );
}

#[test]
fn render_hit_diagnostics_are_scene_owned() {
    let source = workspace_source("crates/cranpose-render/common/src/graph_scene.rs");

    assert!(
        source.contains("pub struct RenderDiagnostics")
            && source.contains("live_modifier_slice_lookup_miss_count"),
        "render hit diagnostics should be represented as retained scene diagnostics"
    );
    assert!(
        !source.contains("LIVE_MODIFIER_SLICE_LOOKUP_MISS_COUNT")
            && !source.contains("AtomicUsize"),
        "render hit diagnostics must not use process-global counters"
    );
}

#[test]
fn pointer_input_task_registry_is_app_context_owned() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let pointer_input_source = std::fs::read_to_string(
        workspace_dir.join("crates/cranpose-ui/src/modifier/pointer_input.rs"),
    )
    .expect("failed to read pointer_input.rs");
    let render_state_source =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-ui/src/render_state.rs"))
            .expect("failed to read render_state.rs");

    assert!(
        !pointer_input_source.contains("static POINTER_INPUT_TASKS"),
        "pointer input task wakeups must not use a module-local task table"
    );
    assert!(
        render_state_source.contains("pointer_input_tasks:")
            && render_state_source.contains("register_pointer_input_task")
            && render_state_source.contains("request_pointer_input_task_poll")
            && render_state_source.contains("context.enter(||")
            && render_state_source
                .contains("context.pointer_input_tasks.request_poll(task_id, owner)"),
        "pointer input task wakeups should run inside the owning AppContext"
    );
}

#[test]
fn fling_velocity_diagnostics_are_app_context_owned() {
    let scroll_source = workspace_source("crates/cranpose-ui/src/modifier/scroll.rs");
    let render_state_source = workspace_source("crates/cranpose-ui/src/render_state.rs");
    let desktop_source = crate_source("src/desktop.rs");

    assert!(
        !scroll_source.contains("LAST_FLING_VELOCITY")
            && !scroll_source.contains("This global state means parallel tests could interfere"),
        "fling velocity diagnostics must not use process-global test state"
    );
    assert!(
        render_state_source.contains("last_fling_velocity_bits")
            && render_state_source.contains("record_last_fling_velocity")
            && render_state_source.contains("debug_last_fling_velocity")
            && render_state_source.contains("debug_reset_last_fling_velocity"),
        "fling velocity diagnostics should be stored on the owning AppContext"
    );
    assert!(
        desktop_source.contains("GetLastFlingVelocity")
            && desktop_source.contains("ResetLastFlingVelocity")
            && desktop_source
                .contains("app.debug_enter_app_context(cranpose_ui::debug_last_fling_velocity)")
            && desktop_source.contains(
                "app.debug_enter_app_context(cranpose_ui::debug_reset_last_fling_velocity)"
            ),
        "desktop robots should query fling diagnostics through the app-thread robot channel"
    );
}

#[test]
fn text_measurer_installation_requires_app_context() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let render_state_source =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-ui/src/render_state.rs"))
            .expect("failed to read render_state.rs");
    let text_measure_source =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-ui/src/text/measure.rs"))
            .expect("failed to read text measure source");

    assert!(
        render_state_source.contains("text: crate::text::measure::TextService::new()"),
        "AppContext should create its own text service instead of cloning fallback text setup"
    );
    assert!(
        render_state_source.contains("panic!(\"set_text_measurer requires an active AppContext\")"),
        "public text measurer installation should require an active AppContext"
    );
    assert!(
        !text_measure_source.contains("fallback_text_measurer_snapshot")
            && !text_measure_source.contains("set_fallback_text_measurer"),
        "fallback text service must not be a mutable setup path for future AppContexts"
    );
}

#[test]
fn render_text_hyphenation_dictionaries_are_measurer_owned() {
    let source = workspace_source("crates/cranpose-render/common/src/text_hyphenation.rs");

    assert!(
        source.contains("pub struct HyphenationDictionaryStore"),
        "hyphenation dictionaries should live in an explicit store owned by the text measurer"
    );
    assert!(
        !source.contains("static DICTIONARIES")
            && !source.contains("OnceLock<RwLock<HashMap<Language, Standard>>>")
            && !source.contains("fn dictionaries() -> &'static"),
        "hyphenation dictionaries must not be retained in process-global mutable state"
    );
}

#[test]
fn wasm_framework_sources_use_browser_safe_time() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let source_roots = [
        "crates/cranpose-core/src",
        "crates/cranpose-runtime-std/src",
        "crates/cranpose-app-shell/src",
        "crates/cranpose-ui/src",
        "crates/cranpose-foundation/src",
        "crates/cranpose-render/common/src",
        "crates/cranpose-render/wgpu/src",
        "crates/cranpose-platform/web/src",
    ];
    let source_files = ["crates/cranpose/src/web.rs"];
    let mut offenders = Vec::new();

    for root in source_roots {
        for path in rust_sources(&workspace_dir.join(root)) {
            collect_forbidden_time_source_offenders(workspace_dir, &path, &mut offenders);
        }
    }
    for file in source_files {
        collect_forbidden_time_source_offenders(
            workspace_dir,
            &workspace_dir.join(file),
            &mut offenders,
        );
    }

    assert!(
        offenders.is_empty(),
        "wasm-delivered framework code must use web_time for clocks; found unsupported std time in:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn wasm_time_source_detection_catches_std_time_import_shapes() {
    let cases = [
        ("direct", "use std::time::Instant;\n"),
        ("alias", "use std::time::Instant as StdInstant;\n"),
        (
            "grouped_multiline",
            "use std::time::{\n    Duration,\n    Instant,\n};\n",
        ),
        (
            "nested_group",
            "use std::{collections::HashMap, time::{Duration, SystemTime}};\n",
        ),
        (
            "qualified_now",
            "fn tick() { let _now = std::time::Instant::now(); }\n",
        ),
        (
            "qualified_type",
            "fn tick(now: std::time::SystemTime) { let _ = now; }\n",
        ),
    ];

    for (name, source) in cases {
        let mut offenders = Vec::new();
        collect_forbidden_time_source_offenders_from_source(
            Path::new(name),
            source,
            &mut offenders,
        );
        assert_eq!(
            offenders.len(),
            1,
            "{name} should report exactly one std::time offender, got {offenders:?}"
        );
    }
}

#[test]
fn wasm_time_source_detection_allows_duration_and_web_time() {
    let mut offenders = Vec::new();

    collect_forbidden_time_source_offenders_from_source(
        Path::new("allowed"),
        "\
use std::time::Duration;
use web_time::Instant;

fn tick() {
    let _delay = Duration::from_millis(16);
    let _now = Instant::now();
}
",
        &mut offenders,
    );

    assert!(
        offenders.is_empty(),
        "Duration and web_time::Instant should remain valid in wasm framework code: {offenders:?}"
    );
}

#[test]
fn unsafe_code_stays_in_android_boundary_modules() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_dir = crate_dir.join("src");
    let allowed = [
        "android_jni.rs",
        "android_surface.rs",
        "android_file_picker.rs",
        "ios_file_picker.rs",
    ];
    let mut offenders = Vec::new();

    for path in rust_sources(&source_dir) {
        let relative = path
            .strip_prefix(&source_dir)
            .expect("source path should be under src");
        let file_name = relative
            .file_name()
            .and_then(|name| name.to_str())
            .expect("source file should have a UTF-8 name");
        if allowed.contains(&file_name) {
            continue;
        }

        let source = std::fs::read_to_string(&path).expect("failed to read cranpose source file");
        if source_has_unsafe_boundary_escape(&source) {
            offenders.push(relative.display().to_string());
        }
    }

    assert!(
        offenders.is_empty(),
        "unsafe code must stay in Android boundary modules; found in {offenders:?}"
    );
}

#[test]
fn android_surface_boundary_returns_typed_errors() {
    let source = crate_source("src/android_surface.rs");

    assert!(
        source.contains("enum AndroidSurfaceError")
            && source.contains("Result<wgpu::Surface<'static>, AndroidSurfaceError>"),
        "Android WGPU surface creation should expose a typed error from the unsafe boundary"
    );
    assert!(
        !source.contains(".expect("),
        "Android WGPU surface creation must not panic inside the unsafe boundary"
    );
}

#[test]
fn android_gpu_initialization_returns_typed_errors() {
    let runtime_source = crate_source("src/android.rs");
    let surface_source = crate_source("src/android_surface.rs");

    assert!(
        !runtime_source.contains(".expect(\"Failed to find suitable adapter\")")
            && !runtime_source.contains(".expect(\"Failed to create device\")"),
        "Android GPU initialization should return typed adapter/device errors instead of panicking"
    );
    assert!(
        surface_source.contains("RequestAdapter(#[from] wgpu::RequestAdapterError)")
            && surface_source.contains("RequestDevice(#[from] wgpu::RequestDeviceError)"),
        "Android GPU initialization errors should be represented in AndroidSurfaceError"
    );
}

#[test]
fn desktop_native_window_gpu_context_absence_returns_launch_error() {
    let desktop_source = crate_source("src/desktop.rs");
    let launcher_source = crate_source("src/launcher.rs");

    assert!(
        !desktop_source.contains("native windows require an initialized desktop GPU context"),
        "native peer-window creation should return LaunchError when the desktop GPU context is unavailable"
    );
    assert!(
        launcher_source.contains("GpuContextUnavailable"),
        "LaunchError should represent missing desktop GPU context explicitly"
    );
}

#[test]
fn desktop_launch_content_unavailable_returns_launch_error() {
    let desktop_source = crate_source("src/desktop.rs");
    let launcher_source = crate_source("src/launcher.rs");

    assert!(
        !desktop_source.contains("content already taken"),
        "desktop startup should return LaunchError when the content closure is unavailable"
    );
    assert!(
        desktop_source.contains("LaunchError::ContentUnavailable")
            && launcher_source.contains("ContentUnavailable"),
        "LaunchError should represent an unavailable desktop content closure explicitly"
    );
}

#[test]
fn desktop_run_wrappers_do_not_repanic_typed_launch_errors() {
    let desktop_source = crate_source("src/desktop.rs");
    let launcher_source = crate_source("src/launcher.rs");

    assert!(
        launcher_source.contains("fn exit_after_launch_error")
            && launcher_source.contains("std::process::exit(1)"),
        "desktop run wrappers should share an explicit process-exit boundary for launch failures"
    );
    assert!(
        !launcher_source.contains("panic!(\"desktop launch failed")
            && !desktop_source.contains("panic!(\"failed to launch desktop app"),
        "desktop run wrappers should not turn typed LaunchError values back into panics"
    );
    assert!(
        launcher_source.contains("exit_after_launch_error(\"desktop launch failed\", error)")
            && desktop_source.contains(
                "crate::launcher::exit_after_launch_error(\"desktop launch failed\", error)"
            ),
        "AppLauncher::run, AppLauncher::run_windows, and desktop::run should use the same launch-error exit path"
    );
}

#[test]
fn wasm_runtime_scheduler_is_single_threaded() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let platform =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-core/src/platform.rs"))
            .expect("failed to read platform.rs");
    let runtime =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-core/src/runtime.rs"))
            .expect("failed to read runtime.rs");
    let std_runtime =
        std::fs::read_to_string(workspace_dir.join("crates/cranpose-runtime-std/src/lib.rs"))
            .expect("failed to read cranpose-runtime-std");

    assert!(
        platform.contains(
            "#[cfg(not(target_arch = \"wasm32\"))]\npub trait RuntimeScheduler: Send + Sync"
        ) && platform.contains("#[cfg(target_arch = \"wasm32\")]\npub trait RuntimeScheduler"),
        "RuntimeScheduler must keep Send+Sync on native and avoid fake Sync on wasm"
    );
    assert!(
        runtime.contains("runtime_id: RuntimeId")
            && runtime.contains("REGISTERED_RUNTIMES.with")
            && runtime.contains("#[cfg(target_arch = \"wasm32\")]\n    fn wake_by_ref"),
        "wasm task wakers should route by runtime id instead of storing a Send+Sync scheduler"
    );
    assert!(
        std_runtime.contains("RefCell<Option<Box<dyn Fn() + 'static>>>")
            && std_runtime.contains("pub fn set_frame_waker(&self, waker: impl Fn() + 'static)"),
        "wasm frame wakers should not require Send or Sync"
    );
}

#[test]
fn workspace_ffi_boundaries_are_explicit() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let source_roots = ["crates", "apps", "xtask"];
    let allowed = [
        "crates/cranpose/src/android_jni.rs",
        "crates/cranpose/src/android_surface.rs",
        "crates/cranpose/src/android_file_picker.rs",
        "crates/cranpose/src/ios_file_picker.rs",
        "apps/desktop-demo-platform/src/android_entry.rs",
        "apps/isolated-demo/src/native_entry.rs",
    ];
    let guard_source = Path::new("crates/cranpose/tests/platform_scheduling_static.rs");
    let mut offenders = Vec::new();

    for root in source_roots {
        for path in rust_sources(&workspace_dir.join(root)) {
            let relative = path
                .strip_prefix(workspace_dir)
                .expect("source path should be under workspace");
            if relative == guard_source {
                continue;
            }
            let relative_display = relative.display().to_string();
            if allowed.contains(&relative_display.as_str()) {
                continue;
            }

            let source = std::fs::read_to_string(&path).expect("failed to read source file");
            if source_has_unsafe_boundary_escape(&source) {
                offenders.push(relative_display);
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "workspace unsafe/FFI boundary code must stay in reviewed boundary modules; found in {offenders:?}"
    );
}

#[test]
fn unsafe_blocks_have_nearby_safety_invariants() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let boundary_modules = [
        "crates/cranpose/src/android_jni.rs",
        "crates/cranpose/src/android_surface.rs",
        "apps/desktop-demo-platform/src/android_entry.rs",
        "apps/isolated-demo/src/native_entry.rs",
    ];
    let mut offenders = Vec::new();

    for module in boundary_modules {
        let source = std::fs::read_to_string(workspace_dir.join(module))
            .unwrap_or_else(|err| panic!("failed to read {module}: {err}"));
        offenders.extend(
            unsafe_lines_without_safety_invariant(&source)
                .into_iter()
                .map(|line| format!("{module}:{line}")),
        );
    }

    assert!(
        offenders.is_empty(),
        "unsafe blocks must include a nearby SAFETY invariant:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn workspace_sources_do_not_cfg_on_robot_app_feature() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let source_roots = ["crates", "apps"];
    let cfg_feature = ["cfg(feature = \"", "robot-app", "\")"].concat();
    let cfg_feature_tight = ["cfg(feature=\"", "robot-app", "\")"].concat();
    let cfg_attr_feature = ["cfg_attr(feature = \"", "robot-app", "\""].concat();
    let cfg_attr_feature_tight = ["cfg_attr(feature=\"", "robot-app", "\""].concat();
    let blocked_patterns = [
        cfg_feature,
        cfg_feature_tight,
        cfg_attr_feature,
        cfg_attr_feature_tight,
    ];
    let guard_source = Path::new("crates/cranpose/tests/platform_scheduling_static.rs");
    let mut offenders = Vec::new();

    for root in source_roots {
        for path in rust_sources(&workspace_dir.join(root)) {
            let relative = path
                .strip_prefix(workspace_dir)
                .expect("source path should be under workspace");
            if relative == guard_source {
                continue;
            }
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative.display()));
            for (line_number, line) in source.lines().enumerate() {
                if blocked_patterns
                    .iter()
                    .any(|pattern| line.contains(pattern))
                {
                    offenders.push(format!("{}:{}", relative.display(), line_number + 1));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "runtime/source behavior must not be gated on the desktop robot-app feature:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn crate_roots_deny_unsafe_code() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let roots = workspace_crate_roots(workspace_dir);

    let missing = roots
        .iter()
        .filter_map(|path| {
            let relative = path
                .strip_prefix(workspace_dir)
                .expect("crate root should live under workspace");
            let source = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative.display()));
            (!source.contains("#![deny(unsafe_code)]")).then(|| relative.display().to_string())
        })
        .collect::<Vec<_>>();

    assert!(
        !roots.is_empty(),
        "workspace crate root discovery found no roots"
    );
    assert!(
        missing.is_empty(),
        "crate roots must deny unsafe code; missing in {missing:?}"
    );
}

#[test]
fn workspace_sources_avoid_half_state_language() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let source_roots = ["crates", "apps", "docs"];
    let single_files = ["README.md"];
    let blocked_terms = [
        ("TO", "DO:"),
        ("TO", "DO!("),
        ("FIX", "ME"),
        ("leg", "acy"),
        ("old ", "way"),
        ("when ", "implemented"),
        ("migra", "tion"),
        ("work", "around"),
        ("backward ", "compat"),
        ("backwards ", "compat"),
    ]
    .map(|(left, right)| format!("{left}{right}").to_lowercase());
    let guard_source = Path::new("crates/cranpose/tests/platform_scheduling_static.rs");
    let mut offenders = Vec::new();

    for root in source_roots {
        for path in text_sources(&workspace_dir.join(root)) {
            let relative = path
                .strip_prefix(workspace_dir)
                .expect("source path should be under workspace");
            if relative == guard_source {
                continue;
            }
            collect_blocked_language_offenders(
                workspace_dir,
                relative,
                &blocked_terms,
                &mut offenders,
            );
        }
    }
    for file in single_files {
        collect_blocked_language_offenders(
            workspace_dir,
            Path::new(file),
            &blocked_terms,
            &mut offenders,
        );
    }

    assert!(
        offenders.is_empty(),
        "workspace text should describe the current architecture directly; found prohibited half-state wording:\n{}",
        offenders.join("\n")
    );
}

fn source_has_unsafe_boundary_escape(source: &str) -> bool {
    source.lines().any(|line| {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            return false;
        }
        (trimmed.contains("unsafe") || trimmed.contains("#[no_mangle]"))
            && trimmed != "#![deny(unsafe_code)]"
    })
}

fn unsafe_lines_without_safety_invariant(source: &str) -> Vec<usize> {
    let lines = source.lines().collect::<Vec<_>>();
    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            if !line_requires_safety_invariant(line) {
                return None;
            }
            let start = index.saturating_sub(3);
            let has_safety = lines[start..index]
                .iter()
                .any(|previous| previous.trim_start().starts_with("// SAFETY:"));
            (!has_safety).then_some(index + 1)
        })
        .collect()
}

fn line_requires_safety_invariant(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("#![") || trimmed.starts_with("#[") {
        return false;
    }

    trimmed.contains("unsafe {")
        || trimmed.contains("unsafe{")
        || trimmed.starts_with("unsafe fn ")
        || trimmed.contains(" unsafe fn ")
        || trimmed.starts_with("unsafe impl ")
        || trimmed.contains(" unsafe impl ")
}

fn collect_blocked_language_offenders(
    workspace_dir: &Path,
    relative: &Path,
    blocked_terms: &[String],
    offenders: &mut Vec<String>,
) {
    let path = workspace_dir.join(relative);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative.display()));
    for (line_number, line) in source.lines().enumerate() {
        let lower = line.to_lowercase();
        if let Some(term) = blocked_terms
            .iter()
            .find(|term| lower.contains(term.as_str()))
        {
            offenders.push(format!(
                "{}:{}: contains `{}`",
                relative.display(),
                line_number + 1,
                term
            ));
        }
    }
}

fn collect_forbidden_time_source_offenders(
    workspace_dir: &Path,
    path: &Path,
    offenders: &mut Vec<String>,
) {
    let relative = path
        .strip_prefix(workspace_dir)
        .expect("source path should be under workspace");
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("failed to read {}: {err}", relative.display()));

    collect_forbidden_time_source_offenders_from_source(relative, &source, offenders);
}

fn collect_forbidden_time_source_offenders_from_source(
    relative: &Path,
    source: &str,
    offenders: &mut Vec<String>,
) {
    let mut pending_use = String::new();
    let mut pending_use_start_line = 0;

    for (index, line) in source.lines().enumerate() {
        let line_number = index + 1;
        let Some(code) = rust_code_before_line_comment(line) else {
            continue;
        };
        let trimmed = code.trim_start();
        if trimmed.is_empty() || trimmed.starts_with("#![") || trimmed.starts_with("#[") {
            continue;
        }

        if !pending_use.is_empty() {
            pending_use.push(' ');
            pending_use.push_str(trimmed);
            if trimmed.contains(';') {
                if let Some(reason) = forbidden_std_time_import_reason(&pending_use) {
                    offenders.push(format!(
                        "{}:{}: {reason}",
                        relative.display(),
                        pending_use_start_line
                    ));
                }
                pending_use.clear();
                pending_use_start_line = 0;
            }
            continue;
        }

        if starts_use_statement(trimmed) {
            pending_use_start_line = line_number;
            pending_use.push_str(trimmed);
            if trimmed.contains(';') {
                if let Some(reason) = forbidden_std_time_import_reason(&pending_use) {
                    offenders.push(format!(
                        "{}:{}: {reason}",
                        relative.display(),
                        pending_use_start_line
                    ));
                }
                pending_use.clear();
                pending_use_start_line = 0;
            }
            continue;
        }

        let normalized = rust_path_source(trimmed);
        if let Some(fragment) = forbidden_std_time_path_fragment(&normalized) {
            offenders.push(format!(
                "{}:{}: uses `{fragment}`",
                relative.display(),
                line_number
            ));
        }
    }

    if !pending_use.is_empty() {
        if let Some(reason) = forbidden_std_time_import_reason(&pending_use) {
            offenders.push(format!(
                "{}:{}: {reason}",
                relative.display(),
                pending_use_start_line
            ));
        }
    }
}

fn rust_code_before_line_comment(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("///") || trimmed.starts_with("//!") {
        return None;
    }

    line.split_once("//")
        .map(|(before_comment, _)| before_comment)
        .or(Some(line))
}

fn starts_use_statement(trimmed: &str) -> bool {
    trimmed.starts_with("use ")
        || trimmed.starts_with("pub use ")
        || (trimmed.starts_with("pub(") && trimmed.contains(" use "))
}

fn forbidden_std_time_import_reason(statement: &str) -> Option<&'static str> {
    let normalized = rust_path_source(statement);

    if forbidden_std_time_path_fragment(&normalized).is_some() {
        return Some("imports unsupported std::time::Instant/SystemTime");
    }
    if std_time_group_contains_forbidden_member(&normalized) {
        return Some("imports unsupported std::time::Instant/SystemTime");
    }
    if std_nested_group_contains_forbidden_time_member(&normalized) {
        return Some("imports unsupported std::time::Instant/SystemTime");
    }

    None
}

fn forbidden_std_time_path_fragment(normalized: &str) -> Option<&'static str> {
    if normalized.contains("std::time::Instant") {
        return Some("std::time::Instant");
    }
    if normalized.contains("std::time::SystemTime") {
        return Some("std::time::SystemTime");
    }

    None
}

fn std_time_group_contains_forbidden_member(normalized: &str) -> bool {
    group_contents_after(normalized, "std::time::{").is_some_and(contains_forbidden_time_member)
}

fn std_nested_group_contains_forbidden_time_member(normalized: &str) -> bool {
    group_contents_after(normalized, "std::{").is_some_and(|std_group| {
        std_group.contains("time::Instant")
            || std_group.contains("time::SystemTime")
            || group_contents_after(std_group, "time::{")
                .is_some_and(contains_forbidden_time_member)
    })
}

fn contains_forbidden_time_member(group: &str) -> bool {
    rust_path_segment_exists(group, "Instant") || rust_path_segment_exists(group, "SystemTime")
}

fn rust_path_segment_exists(source: &str, segment: &str) -> bool {
    let mut remaining = source;
    while let Some(offset) = remaining.find(segment) {
        let before = remaining[..offset].chars().next_back();
        let after = remaining[offset + segment.len()..].chars().next();
        if before.is_none_or(|ch| !rust_identifier_char(ch))
            && after.is_none_or(|ch| !rust_identifier_char(ch))
        {
            return true;
        }
        remaining = &remaining[offset + segment.len()..];
    }
    false
}

fn rust_identifier_char(ch: char) -> bool {
    ch == '_' || ch.is_ascii_alphanumeric()
}

fn group_contents_after<'a>(source: &'a str, prefix: &str) -> Option<&'a str> {
    let start = source.find(prefix)? + prefix.len();
    let mut depth = 1usize;

    for (offset, ch) in source[start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[start..start + offset]);
                }
            }
            _ => {}
        }
    }

    Some(&source[start..])
}

fn rust_path_source(source: &str) -> String {
    source.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn text_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_text_sources(root, &mut out);
    out
}

fn collect_text_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("failed to read source directory") {
        let path = entry.expect("failed to read source directory entry").path();
        if path.is_dir() {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if matches!(name, "target" | ".git" | ".gradle" | "build") {
                continue;
            }
            collect_text_sources(&path, out);
            continue;
        }
        let extension = path.extension().and_then(|extension| extension.to_str());
        if matches!(extension, Some("rs" | "md" | "toml" | "sh")) {
            out.push(path);
        }
    }
}

fn rust_sources(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    collect_rust_sources(root, &mut out);
    out
}

fn workspace_crate_roots(workspace_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for source_root in ["crates", "apps", "xtask"] {
        for path in rust_sources(&workspace_dir.join(source_root)) {
            if is_crate_root_source(&path) {
                roots.push(path);
            }
        }
    }
    roots
}

fn is_crate_root_source(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if !matches!(file_name, "lib.rs" | "main.rs") {
        return false;
    }
    path.parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some("src")
}

fn collect_rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("failed to read cranpose source directory") {
        let path = entry.expect("failed to read source directory entry").path();
        if path.is_dir() {
            collect_rust_sources(&path, out);
        } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}
