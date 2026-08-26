use std::path::{Path, PathBuf};

fn crate_source(path: &str) -> String {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(crate_dir.join(path)).expect("failed to read cranpose source file")
}

/// Manifest comments explain what the framework contributes, and name the very
/// declarations an application must not make. Only the markup is asserted on.
fn strip_xml_comments(source: &str) -> String {
    let mut remaining = source;
    let mut out = String::with_capacity(source.len());
    while let Some(open) = remaining.find("<!--") {
        out.push_str(&remaining[..open]);
        remaining = match remaining[open..].find("-->") {
            Some(close) => &remaining[open + close + "-->".len()..],
            None => "",
        };
    }
    out.push_str(remaining);
    out
}

/// The one place the Cranpose native build is configured.
const CRANPOSE_GRADLE_PLUGIN: &str = "crates/cranpose/android/cranpose-gradle-plugin/src/main/kotlin/dev/cranpose/gradle/CranposeAndroidPlugin.kt";

/// Where the plugin reads a service's manifest fragment from, by service name.
fn cranpose_manifest(service: &str) -> String {
    format!("crates/cranpose/android/manifests/{service}.xml")
}

/// Every Android application built from this repository.
const ANDROID_APPLICATION_BUILD_FILES: [&str; 2] = [
    "apps/android-demo/android/app/build.gradle.kts",
    "apps/isolated-demo/android/app/build.gradle.kts",
];

/// Their manifests, which state only what is specific to each application.
const ANDROID_APPLICATION_MANIFESTS: [&str; 2] = [
    "apps/android-demo/android/app/src/main/AndroidManifest.xml",
    "apps/isolated-demo/android/app/src/main/AndroidManifest.xml",
];

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
    let heavy_workflow = workspace_source(".github/workflows/heavy-selfhosted.yml");
    let release_workflow = workspace_source(".github/workflows/release.yml");
    let pages_workflow = workspace_source(".github/workflows/deploy-pages.yml");
    let justfile = workspace_source("justfile");

    assert!(
        workflow.contains("architecture-budget:")
            && workflow.contains("name: architecture budgets (linux)"),
        "Rust CI should keep a dedicated architecture budget job"
    );

    // The gates themselves live in the justfile, and CI invokes the recipe. That
    // is the point of the justfile: a gate spelled in two places drifts, and this
    // repository had nine such divergences before it existed. So assert the
    // command in the justfile and the invocation in the workflow, never the
    // command in the workflow.
    for recipe in [
        "run: just fmt-check",
        "run: just typos",
        "run: just versions",
        "run: just test",
        "run: just clippy",
        "run: just doc",
        "run: just budgets",
        "run: just clippy-wasm",
        "run: just web",
    ] {
        assert!(
            workflow.contains(recipe),
            "Rust CI should invoke `{recipe}` rather than spelling the gate inline"
        );
    }
    assert!(
        workflow.contains("command -v just >/dev/null || cargo install just --locked"),
        "every CI job that runs a recipe must provision `just` first"
    );

    assert!(
        justfile.contains("cargo build --workspace --no-default-features"),
        "the budgets recipe should prove the workspace builds with default features disabled"
    );
    assert!(
        justfile.contains("cargo check --workspace --all-features"),
        "the budgets recipe should prove the all-features graph still type-checks"
    );
    assert!(
        justfile.contains("cargo xtask dependency-budget --explain"),
        "the budgets recipe should print duplicate dependency owner details"
    );
    assert!(
        justfile.contains("cargo xtask dependency-budget --strict --explain")
            && justfile.contains(
                "cargo xtask dependency-budget --strict --slice desktop-platform --explain"
            )
            && justfile.contains(
                "cargo xtask dependency-budget --strict --slice optional-features --explain"
            ),
        "the budgets recipe should enforce full strict zero duplicates and keep focused clean-slice diagnostics"
    );
    assert!(
        justfile.contains("cargo xtask binary-size")
            && justfile.contains("--package isolated-demo")
            && justfile.contains("--bin isolated-demo")
            && justfile.contains("--profile release-small")
            && justfile.contains("--max-bytes 15728640"),
        "the budgets recipe should enforce the accessibility-enabled release-small binary size ceiling"
    );
    assert!(
        justfile.contains("apps/desktop-demo/build-web.sh --release"),
        "the web recipe must pass --release explicitly so the wasm-release profile and \
         WASM size budget cannot depend on ambient CI defaults"
    );
    assert!(
        workflow.contains("wasm-build:")
            && workflow.contains("wasm-opt --version")
            && workflow.contains("cargo install wasm-pack --version 0.13.1 --locked"),
        "Rust CI should keep the web release build job provisioned with a pinned wasm-pack"
    );

    assert!(
        pages_workflow.contains("Deploy to GitHub Pages")
            && pages_workflow.contains("Install binaryen (wasm-opt) for size optimization")
            && pages_workflow.contains("cargo install wasm-pack --version 0.13.1")
            && pages_workflow.contains("./build-web.sh --release")
            && pages_workflow.contains("actions/upload-pages-artifact@"),
        "GitHub Pages deployment must publish the same budgeted optimized WASM produced by build-web.sh --release"
    );
    assert!(
        !workflow.contains("android-actions/setup-android")
            && !release_workflow.contains("android-actions/setup-android"),
        "Android CI should install only required SDK packages instead of running the broad setup-android action"
    );
    assert!(
        heavy_workflow.contains("ANDROID_NDK_HOME=$sdk_root/ndk/27.0.12077973")
            && heavy_workflow.contains("sdkmanager \"ndk;27.0.12077973\"")
            && heavy_workflow.contains("test -f \"$ANDROID_NDK_HOME/source.properties\"")
            && release_workflow.contains("bash scripts/ci/install_android_ndk.sh 27.0.12077973"),
        "self-hosted Android CI and hosted release builds should provision and validate the pinned NDK"
    );
}

/// Every GitHub Action is pinned to a commit, not to a movable tag.
///
/// A tag like `@v6` is a pointer its owner can repoint at any commit. These
/// workflows run on persistent self-hosted machines that keep their workspaces
/// and caches between runs, so a repointed tag executes with far more reach
/// than it would on a throwaway cloud runner. The version stays in a trailing
/// comment so the pin is still readable.
#[test]
fn workflow_actions_are_pinned_to_commit_shas() {
    let mut unpinned = Vec::new();
    let mut seen = 0usize;
    for name in [
        "rust.yml",
        "heavy-selfhosted.yml",
        "publish.yml",
        "release.yml",
        "deploy-pages.yml",
        "build-one.yml",
    ] {
        let workflow = workspace_source(&format!(".github/workflows/{name}"));
        for line in workflow.lines() {
            let trimmed = line.trim();
            let Some(reference) = trimmed
                .strip_prefix("- uses:")
                .or_else(|| trimmed.strip_prefix("uses:"))
            else {
                continue;
            };
            let reference = reference.trim();
            seen += 1;
            // Local composite actions are referenced by path, not by ref.
            if reference.starts_with('.') {
                continue;
            }
            let Some((_, git_ref)) = reference.split_once('@') else {
                unpinned.push(format!("{name}: {reference} (no ref at all)"));
                continue;
            };
            let git_ref = git_ref.split_whitespace().next().unwrap_or(git_ref);
            let pinned = git_ref.len() == 40 && git_ref.chars().all(|c| c.is_ascii_hexdigit());
            if !pinned {
                unpinned.push(format!("{name}: {reference}"));
            }
        }
    }

    // A sweep that matches nothing passes for the wrong reason. This test was
    // once deleted wholesale and its absence looked exactly like success, so
    // pin the fact that it is still looking at something.
    assert!(
        seen >= 10,
        "expected to inspect many action references, saw only {seen}: the parser has drifted"
    );
    assert!(
        unpinned.is_empty(),
        "every workflow action must be pinned to a 40-character commit SHA; found movable refs: {unpinned:?}"
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
            "if !robot_driven\n                        && should_chain_no_vsync_redraw(\n                            frame_interval,\n                            app.frame_schedule().needs_frame,"
        )
            && source.contains("request_redraw_once(window, &mut self.primary_redraw_pending);"),
        "primary desktop frames should chain dirty no-vsync redraws while allowing robot commands to advance between presented frames"
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
    // The first-present / warmup decision lives once in wgpu_surface
    // (compiled for every wgpu shell: desktop, web, iOS, Android) so the
    // render loops cannot drift apart (the web loop "white until scroll"
    // bug was a desktop/web divergence).
    let shared = crate_source("src/wgpu_surface.rs");
    assert!(
        shared.contains("pub(crate) fn surface_present_required(")
            && shared.contains("surface_dirty || update_visual_changed || app_needs_redraw"),
        "the shared desktop_input module must own the single surface present decision"
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
fn android_resume_robot_contract_retains_gpu_and_marks_shell_dirty() {
    let source = crate_source("src/android.rs");

    assert!(
        !source.contains(
            "drop_present_surface(&mut gpu_resources, &mut app_shell);\n                            } else {\n                                gpu_resources = None;"
        ),
        "the resume robot must not discard the device and renderer on TerminateWindow"
    );
    assert!(
        source.contains("resources.surface = None;"),
        "the resume robot must detach only the native surface"
    );
    assert!(
        source.contains("setup.resources.surface_dirty = true;\n            shell.mark_dirty();"),
        "the resume robot must force a composition before the first resumed present"
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
    let offscreen_period = "const OFFSCREEN_UPDATE_PERIOD: Duration = Duration::from_millis(16);";
    assert!(
        source.contains(offscreen_period),
        "the off-screen work pace is the one 16 ms period this file may hold"
    );
    assert_eq!(
        source.matches("from_millis(16)").count(),
        1,
        "android runtime must not poll at 16 ms while idle; the only 16 ms period is OFFSCREEN_UPDATE_PERIOD, which paces work for an app that asked to keep running off screen"
    );
    assert!(
        source.contains("let offscreen = no_surface && cranpose_services::background_active();"),
        "the off-screen pass must run only when an app asked to keep working with no surface"
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
        jni_source.contains("jni_str!(\"getClassLoader\")")
            && !jni_source.contains("jni_str!(\"getClass\")")
            && overlay_source.contains("load_cranpose_java_class")
            && !overlay_source.contains("jni_str!(\"getClass\")"),
        "Android Java bridge loading must use the Activity context classloader (via the shared \
         android_jni helper); android.app.NativeActivity itself is framework-loaded by the boot \
         classloader"
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
fn android_launch_arguments_reach_the_service_registry() {
    let services_source = crate_source("src/android_services.rs");
    let decoder_source = crate_source("src/android_launch_args.rs");
    let environment_source = crate_source("src/platform_env.rs");
    let java_source =
        workspace_source("crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java");

    assert!(
        java_source.contains("public String cranposeEncodeLaunchArguments()")
            && java_source.contains("ApplicationInfo.FLAG_DEBUGGABLE")
            && java_source
                .contains("private static native void nativeOnLaunchArguments(String payload);")
            && java_source.contains("nativeOnLaunchArguments(cranposeEncodeLaunchArguments());"),
        "CranposeActivity should encode the launching intent's extras with the debuggable flag and re-push them from onNewIntent; a NativeActivity has no other way to see them"
    );
    assert!(
        java_source.contains("private void loadCranposeNativeLibrary()")
            && java_source.contains("System.loadLibrary(libraryName)"),
        "the Java-declared launch-argument callback only resolves because CranposeActivity loads the library itself; libnativeloader does not register it with ART's JNI resolver"
    );
    assert!(
        services_source.contains("jni_str!(\"cranposeEncodeLaunchArguments\")")
            && services_source
                .contains("set_platform_launch_args(Rc::new(read_launch_arguments(&app)))"),
        "the Android backend should pull the launching intent's extras at startup, where getIntent() is already populated, instead of racing a push from onCreate"
    );
    assert!(
        services_source
            .contains("Java_dev_cranpose_android_CranposeActivity_nativeOnLaunchArguments")
            && services_source.contains("PENDING_LAUNCH_ARGS")
            && services_source.contains("shell.request_root_render()"),
        "onNewIntent extras should be parked for the native loop, which owns the snapshot, and force a root render once applied"
    );
    assert!(
        decoder_source.contains("pub(crate) fn decode_launch_arguments"),
        "the intent-extra wire format should be decoded in safe Rust, outside the JNI boundary"
    );
    assert!(
        environment_source.contains("local_launch_args().provides(launch_args)"),
        "the platform environment should publish the launch arguments so composition observes a replacement intent"
    );
}

#[test]
fn android_play_billing_reaches_the_purchase_registry() {
    let services_source = crate_source("src/android_services.rs");
    let backend_source = crate_source("src/android_purchases.rs");
    let wire_source = crate_source("src/android_purchase_wire.rs");
    let java_source = workspace_source(
        "crates/cranpose/android/java-billing/dev/cranpose/android/CranposeBilling.java",
    );

    assert!(
        services_source.contains("crate::android_purchases::register(app.clone())"),
        "the Android backend should install the Play Billing purchase backend alongside the other platform services"
    );
    assert!(
        backend_source.contains("set_platform_purchases(Arc::new(AndroidPurchases {")
            && backend_source.contains("load_cranpose_java_class(env, &activity, BILLING_CLASS)"),
        "the Play Billing backend should reach its Java bridge through the activity class loader and register itself into cranpose_services::purchases"
    );
    assert!(
        backend_source.contains("jni_str!(\"cranposeBillingConfigure\")")
            && backend_source.contains("jni_str!(\"cranposeBillingPurchase\")")
            && backend_source.contains("jni_str!(\"cranposeBillingRestore\")"),
        "querying products, buying and restoring should each be one non-blocking JNI call into the Java bridge"
    );
    assert!(
        backend_source.contains("Java_dev_cranpose_android_CranposeBilling_nativeBillingSnapshot")
            && backend_source
                .contains("Java_dev_cranpose_android_CranposeBilling_nativeBillingEvent")
            && backend_source.contains("wake_native_loop()"),
        "store answers arrive on Play Billing worker threads and must be parked for the native loop, which is woken so the frame that reads them happens"
    );
    assert!(
        wire_source.contains("pub(crate) fn decode_store_snapshot")
            && wire_source.contains("pub(crate) fn decode_purchase_event"),
        "the Play Billing wire format should be decoded in safe Rust, outside the JNI boundary"
    );
    assert!(
        java_source.contains("private static native void nativeBillingSnapshot(String payload);")
            && java_source.contains("activity.runOnUiThread")
            && java_source.contains("client.launchBillingFlow(activity, flow)"),
        "the Java bridge should flatten the whole store snapshot into one JNI call and launch the payment sheet on the Java UI thread"
    );
    assert!(
        java_source.contains("acknowledgePurchase"),
        "Play refunds an unacknowledged purchase, so the bridge must acknowledge every entitlement it sees"
    );
}

/// The accessibility payload is a positional record split on tabs on the Java
/// side, so the two ends have to agree on how many fields there are. Rust
/// builds the record and Java rejects any record of the wrong width, which
/// means a field added on one side alone does not fail loudly — it makes the
/// app silently unreachable to a screen reader. Hence this check.
#[test]
fn android_accessibility_record_width_agrees_across_the_jni_boundary() {
    let wire_source = crate_source("src/android_accessibility_wire.rs");
    let java_source =
        workspace_source("crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java");

    let rust_fields = wire_source
        .lines()
        .find(|line| line.trim_start().starts_with("\"{}\\t"))
        .map(|line| line.matches("{}").count())
        .expect("the accessibility record format string should be one line");
    let java_fields = java_source
        .lines()
        .find(|line| line.contains("ACCESSIBILITY_FIELDS ="))
        .and_then(|line| {
            line.rsplit('=')
                .next()
                .map(|value| value.trim().trim_end_matches(';').to_string())
        })
        .and_then(|value| value.parse::<usize>().ok())
        .expect("CranposeActivity should declare the accessibility record width");

    assert_eq!(
        rust_fields, java_fields,
        "the encoder writes {rust_fields} fields but CranposeActivity parses {java_fields}"
    );
    assert!(
        java_source.contains("if (fields.length != ACCESSIBILITY_FIELDS) continue;"),
        "a record of the wrong width should be skipped, not indexed past its end"
    );
}

/// A custom action is the only screen-reader command with no position to
/// synthesise a tap at, so it is the one that needs a dispatch path of its
/// own — declared in Java, exported from the JNI boundary, and resolved back
/// to a handler on the frame loop.
#[test]
fn android_accessibility_custom_actions_reach_the_frame_loop() {
    let java_source =
        workspace_source("crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java");
    let boundary_source = crate_source("src/android_accessibility.rs");
    let loop_source = crate_source("src/android.rs");
    let projection_source = crate_source("src/accessibility.rs");

    assert!(
        java_source.contains(
            "private static native void nativeOnAccessibilityCustomAction(int virtualViewId, int actionIndex);"
        ) && java_source.contains("nativeOnAccessibilityCustomAction(element.id, customIndex);"),
        "the provider should route a custom action back by identity rather than by synthesising a tap it has no position for"
    );
    assert!(
        boundary_source.contains(
            "Java_dev_cranpose_android_CranposeActivity_nativeOnAccessibilityCustomAction"
        ) && boundary_source.contains("pub(crate) fn drain_custom_actions()"),
        "the JNI boundary should park custom actions for the frame loop instead of running app code on the Java thread"
    );
    assert!(
        loop_source.contains("crate::android_accessibility::drain_custom_actions()")
            && loop_source.contains("crate::accessibility::resolve_element_id(")
            && loop_source.contains("crate::accessibility::perform_custom_action("),
        "the frame loop should resolve the virtual view id and run the action against the live semantics tree"
    );
    assert!(
        projection_source.contains("pub(crate) fn perform_custom_action(")
            && projection_source.contains("pub(crate) fn element_ids("),
        "resolving an accessibility id and running its action are platform-neutral and belong outside the JNI boundary"
    );
}

/// The writer's half of the owned row's order id.
///
/// Nothing in CI compiles or runs `CranposeBilling.java` -- no app in this
/// workspace enables the `playbilling` feature -- so the decoder's tests are
/// the only executable coverage the wire has, and they would keep passing with
/// the Java side of the field deleted: `order_id()` would simply return `None`
/// on every Android device, forever, which is also what a legitimately absent
/// order id looks like.
#[test]
fn the_play_billing_bridge_sends_the_order_id_that_granted_each_entitlement() {
    let java_source = workspace_source(
        "crates/cranpose/android/java-billing/dev/cranpose/android/CranposeBilling.java",
    );

    assert!(
        java_source.contains("purchase.getOrderId()"),
        "the bridge must read Play's order id; the product id is what the app already knew"
    );
    assert!(
        java_source.contains("escape(orderId)"),
        "the order id must be escaped onto the owned row like every other field: the row is \
         tab separated and nothing in Play's format forbids a tab"
    );

    // The two maps are one fact split in half. Refilling `owned` without
    // refilling `orders` in the same breath leaves an order id outliving the
    // purchase that produced it -- a paper trail pointing at the wrong sale.
    let apply = java_source
        .split("private int apply(")
        .nth(1)
        .expect("the snapshot bridge should apply purchase lists in one place");
    let apply = apply
        .split("\n    private")
        .next()
        .expect("a method body is delimited by the next member");
    assert!(
        apply.contains("owned.clear()") && apply.contains("orders.clear()"),
        "ownership and its order ids must be replaced together, or an order id survives the \
         purchase it belongs to"
    );
}

#[test]
fn android_native_input_is_drained_on_input_available_event() {
    let source = crate_source("src/android.rs");

    assert!(
        source.contains("MainEvent::InputAvailable")
            && source.contains("drain_android_input_events(")
            && source.contains("push_pending_inputs_from_android_event(")
            && source.contains("android_activity::InputStatus::Handled"),
        "Android NativeActivity input must be drained from MainEvent::InputAvailable so every input event reaches finish_event before the platform ANR timeout"
    );
    assert!(
        !source
            .contains("println!(\n                                                    \"[TOUCH]")
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
fn web_primary_pointer_stream_is_captured_until_release_or_cancel() {
    let source = crate_source("src/web.rs");

    assert!(
        source.contains("set_pointer_capture(event.pointer_id())"),
        "web pointer-down must capture the pointer so selection handles keep ownership outside the canvas"
    );
    assert!(
        source
            .matches("release_pointer_capture(event.pointer_id())")
            .count()
            >= 2,
        "web pointer-up and pointer-cancel must both release canvas pointer capture"
    );
}

#[test]
fn android_cancel_terminates_the_primary_pointer_stream() {
    let source = crate_source("src/android.rs");

    assert!(
        source.contains("MotionAction::Cancel")
            && source.contains("PendingInput::PointerCancel")
            && source.contains("shell.cancel_gesture()"),
        "Android ACTION_CANCEL must reach AppShell::cancel_gesture instead of leaving a selection handle captured"
    );
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
            linux.contains("\"vulkan\"")
                && !linux.contains("\"gles\"")
                && !linux.contains("\"dx12\"")
                && !linux.contains("\"metal\""),
            "{manifest} Linux WGPU backend set should hardcode Vulkan only; GLES is opt-in via backend-gles"
        );

        let android = manifest_section(
            &source,
            "[target.'cfg(target_os = \"android\")'.dependencies]",
        );
        assert!(
            android.contains("\"vulkan\"")
                && !android.contains("\"gles\"")
                && !android.contains("\"dx12\"")
                && !android.contains("\"metal\""),
            "{manifest} Android WGPU backend set should hardcode Vulkan only; GLES comes from the android feature enabling backend-gles"
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

    let render_wgpu = workspace_source("crates/cranpose-render/wgpu/Cargo.toml");
    assert!(
        render_wgpu.contains("backend-gles = [\"wgpu/gles\", \"naga/glsl-out\"]"),
        "cranpose-render-wgpu must expose the GLES fallback as backend-gles (wgpu/gles + naga/glsl-out)"
    );

    let facade = workspace_source("crates/cranpose/Cargo.toml");
    assert!(
        facade.contains("renderer-wgpu-gles = ["),
        "cranpose must expose renderer-wgpu-gles for the desktop GLES fallback"
    );
    let android_feature_start = facade
        .find("android = [")
        .expect("cranpose android feature is missing");
    let android_feature = &facade[android_feature_start..];
    let android_feature = &android_feature[..android_feature
        .find(']')
        .expect("cranpose android feature array is unterminated")];
    assert!(
        android_feature.contains("cranpose-render-wgpu?/backend-gles"),
        "the cranpose android feature must keep the GLES fallback enabled on Android"
    );
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
fn unsafe_code_stays_in_reviewed_platform_boundary_modules() {
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source_dir = crate_dir.join("src");
    let allowed = [
        // The display-shape read behind the renderer's visible-region cull:
        // one `dlsym`-resolved `AConfiguration_getScreenRound` call (the
        // symbol is API 30+, so it must not be linked) and the call through
        // it.
        "android_display.rs",
        // The entry-point macro: the `#[unsafe(no_mangle)]` in the expansion it writes
        // is the symbol `NativeActivity` resolves after loading the library.
        "android_entry.rs",
        // The display refresh-rate vote: `dlsym`/`dlopen` resolution of the
        // `ANativeWindow_setFrameRate*` NDK symbols and the calls through
        // them, mirroring how HWUI votes the panel's frame rate.
        "android_frame_rate.rs",
        // The ADPF hint session: dlsym-resolved APerformanceHint_* calls and
        // the sessions they manage; absent symbols degrade to a no-op.
        "android_perf_hint.rs",
        "android_frame_telemetry.rs",
        "android_jni.rs",
        "android_accessibility.rs",
        // Camera and host JNI calls stay behind the same reviewed activity
        // boundary as the other Android services.
        "android_camera.rs",
        "android_host.rs",
        // The media JNI surface: the exported symbols `CranposeMedia` pushes
        // playback state, position, focus and lock-screen buttons through.
        "android_media.rs",
        "android_services.rs",
        "android_surface.rs",
        "android_file_picker.rs",
        // The Play Billing bridge: the exported symbols
        // `dev.cranpose.android.CranposeBilling` calls back through. Decoding
        // what they carry lives in safe Rust next door, in
        // android_purchase_wire.rs.
        "android_purchases.rs",
        "android_text_input.rs",
        // One `AChoreographer_postFrameCallback64` and the callback it posts,
        // which is how the frame loop learns when the display is ready for the
        // next frame.
        "android_vsync.rs",
        "android_writable_folder.rs",
        // The process readings every application that watches its own
        // footprint would otherwise write for itself: `getrusage`, `sysconf`,
        // `mallopt` and `os_proc_available_memory`, each behind one contract.
        "process_info.rs",
        "ios_file_picker.rs",
        "ios_uri_handler.rs",
        "ios_clipboard.rs",
        "ios_share_sheet.rs",
        "ios_image_picker.rs",
        "ios_notifier.rs",
        "ios_writable_folder.rs",
        "ios_camera.rs",
        // AVAudioPlayer, the audio-session interruption observer and the
        // MediaPlayer remote commands, behind the same reviewed boundary as
        // the other iOS services.
        "ios_media.rs",
        "ios_keyboard.rs",
        "ios_back_gesture.rs",
        "ios_background.rs",
        "ios_host.rs",
        "ios_accessibility.rs",
        "desktop_accessibility.rs",
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
        "unsafe code must stay in reviewed platform boundary modules; found in {offenders:?}"
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
    let launcher_source = crate_source("src/app_launcher.rs");

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
    let launcher_source = crate_source("src/app_launcher.rs");

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
    let launcher_source = crate_source("src/app_launcher.rs");

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
                "crate::app_launcher::exit_after_launch_error(\"desktop launch failed\", error)"
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
        // The display-shape read behind the renderer's visible-region cull:
        // one `dlsym`-resolved `AConfiguration_getScreenRound` call (the
        // symbol is API 30+, so it must not be linked) and the call through
        // it.
        "crates/cranpose/src/android_display.rs",
        // The entry-point macro: one `#[unsafe(no_mangle)]` in the expansion it writes,
        // which is the symbol `NativeActivity` resolves after loading the
        // library. It replaced the same attribute in every application.
        "crates/cranpose/src/android_entry.rs",
        // The display refresh-rate vote: `dlsym`/`dlopen` resolution of the
        // `ANativeWindow_setFrameRate*` NDK symbols and the calls through
        // them, mirroring how HWUI votes the panel's frame rate.
        "crates/cranpose/src/android_frame_rate.rs",
        "crates/cranpose/src/android_perf_hint.rs",
        "crates/cranpose/src/android_frame_telemetry.rs",
        "crates/cranpose/src/android_jni.rs",
        "crates/cranpose/src/android_accessibility.rs",
        // Camera and host JNI calls stay behind the same reviewed activity
        // boundary as the other Android services.
        "crates/cranpose/src/android_camera.rs",
        "crates/cranpose/src/android_host.rs",
        // The media JNI surface: the exported symbols `CranposeMedia` pushes
        // playback state, position, focus and lock-screen buttons through.
        "crates/cranpose/src/android_media.rs",
        "crates/cranpose/src/android_services.rs",
        "crates/cranpose/src/android_surface.rs",
        "crates/cranpose/src/android_file_picker.rs",
        // The Play Billing bridge: the exported symbols
        // `dev.cranpose.android.CranposeBilling` calls back through, and
        // nothing else. Decoding the payloads they carry lives in safe Rust in
        // android_purchase_wire.rs, which is built and tested on the host.
        "crates/cranpose/src/android_purchases.rs",
        "crates/cranpose/src/android_text_input.rs",
        // One `AChoreographer_postFrameCallback64` and the callback it posts,
        // which is how the frame loop learns when the display is ready for the
        // next frame.
        "crates/cranpose/src/android_vsync.rs",
        "crates/cranpose/src/android_writable_folder.rs",
        // The process readings every application that watches its own
        // footprint would otherwise write for itself: `getrusage`, `sysconf`,
        // `mallopt` and `os_proc_available_memory`, each behind one contract.
        "crates/cranpose/src/process_info.rs",
        "crates/cranpose/src/ios_file_picker.rs",
        "crates/cranpose/src/ios_uri_handler.rs",
        "crates/cranpose/src/ios_clipboard.rs",
        "crates/cranpose/src/ios_share_sheet.rs",
        "crates/cranpose/src/ios_image_picker.rs",
        "crates/cranpose/src/ios_notifier.rs",
        "crates/cranpose/src/ios_writable_folder.rs",
        "crates/cranpose/src/ios_camera.rs",
        // AVAudioPlayer, the audio-session interruption observer and the
        // MediaPlayer remote commands, behind the same reviewed boundary as
        // the other iOS services.
        "crates/cranpose/src/ios_media.rs",
        "crates/cranpose/src/ios_keyboard.rs",
        "crates/cranpose/src/ios_back_gesture.rs",
        "crates/cranpose/src/ios_background.rs",
        "crates/cranpose/src/ios_host.rs",
        "crates/cranpose/src/ios_accessibility.rs",
        "crates/cranpose/src/desktop_accessibility.rs",
        // The StoreKit 2 bridge: `extern "C"` declarations for the Swift shim
        // plus the callback it invokes. The crate root denies unsafe code and
        // opts this one module back in by name.
        "crates/cranpose-storekit/src/apple.rs",
        // The audio engine's two boundaries: the lock-free queue that carries
        // commands to the real-time thread, and the AAudio callback that turns
        // the device's raw output pointer into a slice. The crate root denies
        // unsafe code and opts these back in by name.
        "crates/cranpose-audio/src/ring.rs",
        "crates/cranpose-audio/src/backend/aaudio.rs",
        // The renderer's fixed frame worker pool: lending frame-local borrows
        // to persistent parked workers cannot be expressed safely in std (the
        // problem rayon exists for). The unsafety is two pointer wrappers
        // whose invariants the pool's completion barrier enforces; the crate
        // root denies unsafe code and opts this one module back in by name.
        "crates/cranpose-render/wgpu/src/worker_pool.rs",
        // The pipeline disk cache: one wgpu create_pipeline_cache call,
        // unsafe because seeding data is trusted; the module writes that
        // data itself from get_data, keys the file by adapter identity,
        // and asks wgpu to validate the header besides (fallback: true).
        "crates/cranpose-render/wgpu/src/pipeline_disk_cache.rs",
        // The shape-run entry borrows its DrawPrimitive, whose TYPE is !Sync
        // (the Text variant holds Rc) even though the constructor only ever
        // admits the Sync-payload shape variants. The module is kept tiny so
        // the constructor invariant and the two manual Send/Sync impls stay
        // on one screen; the crate root denies unsafe code and opts this one
        // module back in by name.
        "crates/cranpose-render/wgpu/src/run_entry.rs",
        // The stage executor's spare-capacity map_fill: chunks write map
        // results straight into the output vec's reserved capacity, which
        // std vectors cannot express safely. The unsafety lives in the
        // constructor-limited spare_fill module — claim flags turn a
        // double-filled chunk into a panic, per-chunk watermarks bound the
        // unwind path's drops — and the crate root denies unsafe code and
        // opts this one module back in by name.
        "crates/cranpose-render/wgpu/src/stage_executor.rs",
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
        "crates/cranpose/src/ios_accessibility.rs",
        "crates/cranpose-audio/src/ring.rs",
        "crates/cranpose-audio/src/backend/aaudio.rs",
        "crates/cranpose-render/wgpu/src/pipeline_disk_cache.rs",
        // The exported `android_main` symbol, written once by the framework's
        // `android_main!` macro rather than once per application.
        "crates/cranpose/src/android_entry.rs",
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

/// Unsafe code is denied once, by the workspace, for every member.
///
/// This used to be thirty-one copies of `#![deny(unsafe_code)]`, one per crate
/// root, so a new crate stayed unprotected until somebody remembered the line.
/// Cargo applies `[workspace.lints]` instead, and the only thing a new crate
/// carries is `[lints] workspace = true`. The FFI modules that genuinely need
/// unsafe still opt in with a module-level `#![allow(unsafe_code)]`: a source
/// attribute outranks a lint level Cargo passes on the command line.
#[test]
fn every_workspace_member_inherits_the_unsafe_code_denial() {
    let cranpose_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let workspace_dir = cranpose_dir
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under workspace crates directory");
    let manifest = std::fs::read_to_string(workspace_dir.join("Cargo.toml"))
        .expect("failed to read workspace manifest");

    assert!(
        manifest.contains("[workspace.lints.rust]"),
        "workspace manifest must declare a `[workspace.lints.rust]` table"
    );
    assert!(
        manifest.contains("unsafe_code = \"deny\""),
        "`[workspace.lints.rust]` must deny `unsafe_code`"
    );

    let members = workspace_members(&manifest);
    assert!(
        !members.is_empty(),
        "workspace manifest declared no members"
    );

    let missing = members
        .iter()
        .filter(|member| {
            let path = workspace_dir.join(member).join("Cargo.toml");
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
            !manifest_inherits_workspace_lints(&text)
        })
        .cloned()
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "every workspace member must carry `[lints] workspace = true`; missing in {missing:?}"
    );
}

/// The member paths declared in the workspace manifest's `members` array.
fn workspace_members(manifest: &str) -> Vec<String> {
    let Some(rest) = manifest.split_once("members = [").map(|(_, rest)| rest) else {
        return Vec::new();
    };
    let Some((list, _)) = rest.split_once(']') else {
        return Vec::new();
    };
    list.lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let inner = trimmed.strip_prefix('"')?;
            inner.split('"').next().map(str::to_owned)
        })
        .collect()
}

/// Whether a member manifest opts in to the workspace lint table.
fn manifest_inherits_workspace_lints(manifest: &str) -> bool {
    let Some(rest) = manifest.split_once("[lints]").map(|(_, rest)| rest) else {
        return false;
    };
    rest.lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .any(|line| {
            let normalised = line.split_whitespace().collect::<String>();
            normalised == "workspace=true"
        })
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
        // `apps/isolated-demo` is its own workspace, so it cannot inherit
        // `[workspace.lints]` and still carries the crate-root attribute. The
        // attribute names the lint; it is not an FFI boundary.
        (trimmed.contains("unsafe") || trimmed.contains("#[unsafe(no_mangle)]"))
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

    if !pending_use.is_empty()
        && let Some(reason) = forbidden_std_time_import_reason(&pending_use)
    {
        offenders.push(format!(
            "{}:{}: {reason}",
            relative.display(),
            pending_use_start_line
        ));
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

/// Every store backend has to announce news, not only wake the loop.
///
/// `observe_store_news` exists because `take_event`/`store_state` are polling
/// APIs and an idle app has no frame loop to poll from. A backend that only
/// calls `wake_native_loop()` leaves the app to notice on some later frame it
/// may never run: measured on a watch app, zero CPU jiffies over ten seconds on
/// the screen showing a price, so a purchase approved while sitting there would
/// never have been seen. iOS announced from the day the listener landed and
/// Android did not, which is the asymmetry this pins down -- one backend
/// growing a new decode path and forgetting to tell anyone is the same bug
/// again.
#[test]
fn every_store_backend_tells_the_app_rather_than_leaving_it_to_ask() {
    let android = crate_source("src/android_purchases.rs");
    let apple = workspace_source("crates/cranpose-storekit/src/apple.rs");

    for (backend, source) in [("android", &android), ("apple", &apple)] {
        assert!(
            source.contains("note_store_news()"),
            "the {backend} store backend must announce news through note_store_news()"
        );
    }

    // Both JNI entry points, not just whichever one was noticed first: a
    // snapshot and an event are separate ways for the store to have news.
    for entry_point in [
        "Java_dev_cranpose_android_CranposeBilling_nativeBillingSnapshot",
        "Java_dev_cranpose_android_CranposeBilling_nativeBillingEvent",
    ] {
        let body = android
            .split(entry_point)
            .nth(1)
            .unwrap_or_else(|| panic!("{entry_point} should exist in android_purchases.rs"));
        let body = body
            .split("pub extern \"system\"")
            .next()
            .expect("an entry point body should be delimited by the next one");
        assert!(
            body.contains("note_store_news()"),
            "{entry_point} decodes store news and must announce it, not only wake the loop"
        );
    }
}

#[test]
fn storekit_bridge_exposes_listener_liveness_and_rebuilds_it() {
    let apple = workspace_source("crates/cranpose-storekit/src/apple.rs");
    let swift = workspace_source("crates/cranpose-storekit/swift/storekit.swift");
    assert!(apple.contains("cranpose_storekit_is_connected"));
    assert!(apple.contains("fn is_connected(&self) -> bool"));
    assert!(swift.contains("cranpose_storekit_is_connected"));
    assert!(swift.contains("_listenerActive"));
    assert!(swift.contains("_listenerActive = false"));
    assert!(swift.contains("if !_listenerActive"));
}

#[test]
fn android_service_registration_replaces_the_relaunch_waker() {
    let services = crate_source("src/android_services.rs");
    assert!(services.contains("LOOP_WAKER.get_or_init"));
    assert!(services.contains("*waker = Some(app.create_waker())"));
    assert!(!services.contains("let _ = LOOP_WAKER.set"));
}

/// The Gradle task that runs `cargo ndk` must declare the directory it writes
/// as an output.
///
/// The `.so` lands in a directory the Android plugin has already been told to
/// read as `jniLibs`, and the merge/package tasks depend on the cargo task, so
/// the ordering looks right. It is not enough: a task that writes files outside
/// its declared outputs leaves Gradle's file-system snapshot of that directory
/// untouched, so the packaging tasks downstream check a pre-cargo snapshot,
/// report UP-TO-DATE, and build the APK around the PREVIOUS run's library.
///
/// Nothing about that is visible from the build log — it says BUILD SUCCESSFUL
/// — and nothing is visible on device either, beyond code behaving as it did
/// one build ago. It costs whole debugging sessions: a fix verified on the host
/// "does not reproduce" on the phone or the watch, which reads as a
/// platform-specific defect and sends the search into the framework. Run the
/// build twice and the symptom evaporates, which makes it look intermittent on
/// top of that.
///
/// `outputs.upToDateWhen { false }` does not cover this. It decides whether the
/// cargo task itself re-runs; it says nothing about what the task changed.
///
/// One Gradle plugin registers that task for every Cranpose application, so
/// this is asserted once, where it is written.
#[test]
fn the_gradle_plugin_declares_the_jni_library_directory_cargo_writes() {
    let plugin = workspace_source(CRANPOSE_GRADLE_PLUGIN);

    assert!(
        plugin.contains("jniLibs.directories.add(nativeOutput.absolutePath)"),
        "the plugin must point the Android source sets at the directory cargo-ndk writes"
    );
    assert!(
        plugin.contains("outputs.dir(nativeOutput)"),
        "the cargo-ndk task must declare the directory it writes as its output, or Gradle \
         keeps a stale snapshot of the jniLibs directory and the APK ships the previous \
         build's .so"
    );
    assert!(
        plugin.contains("outputs.upToDateWhen { false }"),
        "the cargo-ndk task declares an output directory, so it also needs \
         outputs.upToDateWhen {{ false }} or Gradle will skip the cargo build whenever that \
         directory happens to be unchanged"
    );

    // `mergeJniLibFolders` collects the source directories and `mergeNativeLibs`
    // collects the libraries inside them; both read the directory cargo writes,
    // so both have to wait for it. Wiring only one is what Gradle reports as an
    // implicit dependency once the output is declared above.
    assert!(
        plugin.contains("task.name.contains(\"NativeLibs\")")
            && plugin.contains("task.name.contains(\"JniLibFolders\")"),
        "the cargo build must be wired to mergeJniLibFolders as well as mergeNativeLibs -- \
         both consume the directory it writes"
    );
}

/// No application re-implements the native build the plugin owns.
///
/// A hand-rolled `cargo ndk` task in an application's build file is how the
/// stale-`.so` failure above comes back: the fix lives in the plugin, and a
/// copy that predates it keeps shipping the previous build.
#[test]
fn android_applications_build_their_native_library_through_the_plugin() {
    for relative in ANDROID_APPLICATION_BUILD_FILES {
        let source = workspace_source(relative);
        assert!(
            source.contains("id(\"dev.cranpose.android\")"),
            "{relative} must apply the Cranpose Gradle plugin rather than configuring an \
             Android application by hand"
        );
        assert!(
            !source.contains("cargo ndk"),
            "{relative} runs cargo ndk itself; the plugin owns the native build, the ABIs, \
             the Cargo profiles and the output declaration that keeps the APK from shipping \
             a stale library"
        );
        assert!(
            !source.contains("jniLibs.directories.add"),
            "{relative} points a source set at the native output itself; the plugin does \
             that, together with declaring the task output that keeps it fresh"
        );
    }
}

/// The activity, its launcher entry and the `android.app.lib_name` metadata are
/// the framework's contract with `NativeActivity`. An application that declares
/// its own copy silently owns a contract it cannot see change.
#[test]
fn android_applications_do_not_declare_the_framework_activity() {
    for relative in ANDROID_APPLICATION_MANIFESTS {
        let manifest = strip_xml_comments(&workspace_source(relative));
        assert!(
            !manifest.contains("<activity"),
            "{relative} declares an activity; the Cranpose library contributes the activity, \
             its launcher filter and its lib_name metadata to every application's manifest"
        );
        assert!(
            !manifest.contains("android.app.lib_name"),
            "{relative} names the cdylib itself; the plugin supplies that name from \
             cranpose {{ cargoPackage }} so it cannot drift from what Cargo builds"
        );
    }
}

/// `web.rs` compiles only for `wasm32`, so nothing in `cargo test` links it —
/// reading the source is the only guard available on the host, and the
/// alternative is finding out in a browser. This contract regressed silently
/// once already: the wheel listener grew its own copy of the desktop's policy,
/// inverted, and never offered the wheel to rotary at all.
#[test]
fn the_browser_host_shares_the_wheel_policy() {
    let web = crate_source("src/web.rs");

    assert!(
        web.contains("app_mut.wheel_scrolled(wheel)"),
        "the browser wheel listener must go through the shell's shared wheel policy, \
         so zoom, rotary and scroll mean the same thing they do on every other host"
    );
    assert!(
        !web.contains("app_mut.pointer_scrolled("),
        "the browser host must not reach past wheel_scrolled to the scroll step: that \
         skips rotary and re-opens the sign question the shared policy settles"
    );
}

/// The same unreachable-source problem for the clipboard: with no bridge
/// installed the in-tree selection menu's Copy reaches an in-process clipboard
/// that nothing outside the page can read, and every platform but the browser
/// had one.
#[test]
fn the_browser_host_installs_a_platform_clipboard() {
    assert!(
        crate_source("src/web.rs").contains("crate::web_clipboard::install("),
        "the browser host must install a platform clipboard, or the in-tree selection \
         menu's Copy/Cut never leave the page"
    );
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

/// Density, the viewport, the platform's fonts and the log tag are all things
/// an application needs and none of them are things it should discover for
/// itself: each answer sits behind a different platform API, and a call site
/// that reaches for one is wrong on the target it did not write.
///
/// Every host therefore publishes its surface the same way, and the launcher
/// resolves the font directory and the log tag, so no application repeats any
/// of it.
#[test]
fn every_host_reports_its_surface_the_same_way() {
    for (relative, host) in [
        ("src/android.rs", "Android"),
        ("src/desktop.rs", "the desktop"),
        ("src/ios.rs", "iOS"),
        ("src/web_host_surface.rs", "the browser"),
    ] {
        let source = crate_source(relative);
        assert!(
            source.contains("publish_host_surface_size("),
            "{host} must publish its surface size, or `host_density` and \
             `rememberHostSurfaceSize` answer for every target but this one"
        );
    }
}

/// The framework owns where a platform keeps its fonts, so an application never
/// names a system path and never draws in the wrong typeface on the target it
/// did not name one for.
#[test]
fn the_launcher_resolves_the_platform_font_directory_itself() {
    let launcher = crate_source("src/app_launcher.rs");
    assert!(
        launcher.contains("crate::system_font_directory()"),
        "with_system_fonts must resolve the platform's font directory rather than \
         asking the application for a path"
    );
    assert!(
        crate_source("src/host_environment.rs").contains("ANDROID_SYSTEM_FONT_DIR"),
        "the resolver must name Android's directory rather than leaving the app to"
    );
}

/// An application that wants its own name on its log lines had to initialise the
/// platform logger before the framework did and hope the ordering held; the tag
/// is a launcher setting instead.
#[test]
fn the_android_host_takes_its_log_tag_from_the_launcher() {
    let android = crate_source("src/android.rs");
    assert!(
        android.contains("settings.log_tag.as_deref().unwrap_or(DEFAULT_LOG_TAG)"),
        "the Android host must log under the tag the application named"
    );
    assert!(
        !android.contains("\"ComposeRS\""),
        "the framework is Cranpose; a stale name in logcat sends anyone reading \
         them looking for the wrong project"
    );
}

/// No application writes the Android entry point by hand.
///
/// It cost every application the same four lines — an `unsafe_code` allowance
/// for the export attribute, a `#[unsafe(no_mangle)]` it must not misspell, a
/// dependency on `android_activity` for nothing but a parameter type, and a
/// `target_os` guard — none of which is about the application. It is one macro
/// now, and the `#[unsafe(no_mangle)]` lives in one reviewed module rather than in
/// every consumer.
#[test]
fn applications_declare_their_android_entry_through_the_macro() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("cranpose crate should live under the workspace crates directory");

    let mut offenders = Vec::new();
    let mut declarations = 0usize;
    for root in ["apps"] {
        let mut sources = Vec::new();
        collect_rust_sources(&workspace.join(root), &mut sources);
        for path in sources {
            let source = std::fs::read_to_string(&path).expect("failed to read application source");
            let relative = path
                .strip_prefix(workspace)
                .expect("source should live under the workspace")
                .display()
                .to_string();
            if source.contains("cranpose::android_main!") {
                declarations += 1;
            }
            if source.contains("pub fn android_main(") || source.contains("fn android_main(") {
                offenders.push(relative);
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "an application must declare its entry point with `cranpose::android_main!` rather \
         than exporting the symbol itself; found in {offenders:?}"
    );
    assert!(
        declarations >= 2,
        "expected the demo and the standalone starter to declare entry points, saw \
         {declarations}"
    );
}

/// An application update is the one download that can replace the application,
/// so what arrives is checked against what the release feed promised *before*
/// it reaches the platform installer.
///
/// Android's own signature check still runs afterwards and catches a package
/// signed by someone else. It does not catch one that arrived corrupted, or one
/// swapped for a differently-signed build the device would happily install as a
/// new application. Committing the session first and finding out afterwards is
/// not a check.
#[test]
fn the_android_installer_verifies_a_package_before_committing_it() {
    let java =
        workspace_source("crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java");

    let install = java
        .split("public void cranposeInstallUpdate(")
        .nth(1)
        .expect("the Android installer entry point");
    let commit = install
        .find("session.commit(")
        .expect("the installer must commit a session");
    let verify = install
        .find("digest.digest()")
        .expect("the installer must compute the package's digest");
    assert!(
        verify < commit,
        "the digest must be checked before the session is committed, or the check \
         happens after the package is already on its way to being installed"
    );
    assert!(
        install.contains("does not match its digest"),
        "a mismatch must fail the install rather than being logged and ignored"
    );
    assert!(
        java.contains("throw new IOException(\"unsupported package digest algorithm: \""),
        "a digest this platform cannot compute must fail rather than being skipped: a \
         check nobody performs reads as a package that was verified"
    );
    assert!(
        !install.contains("digest != null"),
        "there is no unverified path through the installer: a package reaches it with a \
         digest or it does not reach it at all"
    );
}

/// The framework computes a digest one way, so no platform's installer can
/// compute it differently and disagree with the one the release feed published.
#[test]
fn the_framework_owns_one_package_digest() {
    let update = workspace_source("crates/cranpose-services/src/app_update.rs");
    assert!(
        update.contains("pub struct DigestVerifier"),
        "the framework must own a package verifier rather than leaving each platform \
         installer to write its own"
    );
    assert!(
        update.contains("pub fn install_app_update(package: &UpdatePackage)"),
        "an install must take the package the feed described — its size and digest \
         included — rather than a bare URL nothing can be checked against"
    );
    let install = update
        .split("pub fn install_app_update(package: &UpdatePackage)")
        .nth(1)
        .expect("the install entry point");
    assert!(
        install.contains("AppUpdateError::Unverifiable"),
        "a package the framework cannot check must be refused: this is the one download \
         that replaces the application, and a feed that publishes no digest is a feed to \
         fix rather than a check to skip"
    );
}

/// A camera preview is the highest-rate path in the framework, so what it does
/// per frame is worth pinning down.
///
/// Routing frames through the filesystem — JPEG-compress on the Java side,
/// write to the cache directory, rename, read back, decode — costs an encode, a
/// write, a rename, a read and a decode on every single frame, which is a bill
/// only a preview capped at fifteen frames a second can pay. Waiting for a
/// still the same way means sleeping in twenty-millisecond steps until a marker
/// file appears, on whichever thread asked for it. This test pins down that
/// neither happens.
#[test]
fn the_android_camera_pushes_frames_rather_than_writing_them_to_files() {
    let camera =
        workspace_source("crates/cranpose/android/java/dev/cranpose/android/CranposeCamera.java");
    let activity =
        workspace_source("crates/cranpose/android/java/dev/cranpose/android/CranposeActivity.java");

    assert!(
        camera.contains("CranposeActivity.onCameraFrame("),
        "preview frames must be pushed to native code rather than left in a file to be found"
    );
    assert!(
        !camera.contains("compressToJpeg"),
        "a preview frame must not be JPEG-encoded to cross the language boundary"
    );
    for name in ["preview.jpg", "capture.jpg", "capture.ok"] {
        assert!(
            !camera.contains(name) && !activity.contains(name),
            "the camera must not transport {name} through the filesystem"
        );
    }
    assert!(
        !activity.contains("Thread.sleep(20)"),
        "a still must arrive rather than be waited for in a sleep loop"
    );
    assert!(
        camera.contains("CranposeActivity.onCameraFrameDropped()"),
        "a frame the device produced while the previous one was in flight must be counted, \
         so a detector that falls behind falls behind by frames rather than by memory"
    );
}

/// The framework's camera contract has no method that hands back a frame or a
/// picture, because both would mean waiting: a poll returns whatever was there
/// and a blocking capture stops whoever asked for as long as the device takes.
#[test]
fn the_camera_service_is_published_to_rather_than_polled() {
    let camera = workspace_source("crates/cranpose-services/src/camera.rs");
    assert!(
        !camera.contains("fn latest_frame(&self)"),
        "a camera backend must publish frames, not answer polls for them"
    );
    assert!(
        !camera.contains("fn capture_still(&self)"),
        "a still must be asked for and arrive, not be returned by a call that waits"
    );
    assert!(
        camera.contains("pub fn publish_camera_frame(")
            && camera.contains("fn request_still(&self)"),
        "the contract is publish-a-frame and ask-for-a-still"
    );
}

/// Every media backend the framework ships, so a contract test covers all of
/// them rather than whichever one was written last.
const MEDIA_BACKENDS: [&str; 4] = [
    "crates/cranpose-media/src/player.rs",
    ANDROID_MEDIA_BACKEND,
    "crates/cranpose/src/ios_media.rs",
    "crates/cranpose/src/web_media.rs",
];

/// The backends that carry the transport themselves. Android's is not one of
/// them: it is the in-process player wearing the platform's session, so it
/// delegates the answers these give directly. See
/// [`the_android_media_backend_wraps_the_in_process_player_rather_than_decoding`].
const TRANSPORT_BACKENDS: [&str; 3] = [
    "crates/cranpose-media/src/player.rs",
    "crates/cranpose/src/ios_media.rs",
    "crates/cranpose/src/web_media.rs",
];

const ANDROID_MEDIA_BACKEND: &str = "crates/cranpose/src/android_media.rs";

/// A media player that is asked "where are you now?" every frame does that work
/// whether or not anything moved, and learns about a failure only by noticing
/// that the position stopped. The contract is the other way round: the backend
/// publishes, and a screen reacts.
#[test]
fn the_media_service_is_published_to_rather_than_polled() {
    let media = workspace_source("crates/cranpose-services/src/media.rs");
    assert!(
        !media.contains("fn position(&self)") && !media.contains("fn state(&self)"),
        "a media backend must publish where it is and what it is doing, not answer polls for them"
    );
    assert!(
        media.contains("pub fn publish_playback_state(")
            && media.contains("pub fn publish_playback_progress("),
        "the contract is publish-what-happened"
    );
    for backend in TRANSPORT_BACKENDS {
        let source = workspace_source(backend);
        assert!(
            source.contains("publish_playback_state"),
            "{backend} must publish what it is doing"
        );
    }
}

/// Android's backend is the framework's own decoder wearing the platform's
/// session, not a second transport. What it must not go back to is decoding
/// with `android.media.MediaPlayer`: that plays a file, and a document provider
/// whose bytes come off a network hands back a pipe instead — which is how a
/// network library stopped playing between 0.1.41 and 0.1.42.
#[test]
fn the_android_media_backend_wraps_the_in_process_player_rather_than_decoding() {
    let source = workspace_source(ANDROID_MEDIA_BACKEND);
    assert!(
        source.contains("SoftwareMediaPlayer"),
        "the Android backend must play through the framework's own decoder"
    );
    let java =
        workspace_source("crates/cranpose/android/java/dev/cranpose/android/CranposeMedia.java");
    assert!(
        !java.contains("android.media.MediaPlayer"),
        "Android's MediaPlayer cannot read a document a provider streams, so the \
         session class must not reach for it again"
    );
    assert!(
        java.contains("AudioManager") && java.contains("MediaSession"),
        "what is left to Java is the half only Java has: audio focus and the lock screen"
    );
}

/// A control the device will not honour is worse than a control that is not
/// there: the user presses it and nothing happens. Every backend states what it
/// can do so a screen can leave out the rest.
#[test]
fn every_media_backend_states_what_it_can_do() {
    for backend in MEDIA_BACKENDS {
        let source = workspace_source(backend);
        assert!(
            source.contains("fn capabilities(&self)"),
            "{backend} must report its capabilities rather than let a screen assume them"
        );
    }
}

/// An equalizer has the bands its implementation has: ten octave bands where
/// the framework builds the filters, whatever the device offers where the
/// platform owns the effect, and none at all on a backend with nowhere to put
/// one. Every backend has to say which of those it is, because a screen draws
/// as many controls as there are bands.
#[test]
fn every_media_backend_states_the_equalizer_it_has() {
    for backend in TRANSPORT_BACKENDS {
        let source = workspace_source(backend);
        assert!(
            source.contains("equalizer:"),
            "{backend} must state whether it has an equalizer in its capabilities"
        );
        // A backend that claims one has to be able to report its bands.
        if source.contains("equalizer: false") {
            continue;
        }
        assert!(
            source.contains("fn equalizer_bands(&self)"),
            "{backend} claims an equalizer but never reports the bands it has"
        );
        assert!(
            source.contains("fn set_equalizer(&self"),
            "{backend} claims an equalizer but never applies a curve"
        );
    }
}

/// Ducking and forgetting to un-duck, or resuming after a phone call that was
/// never paused for, is the same bug written once per application. The policy
/// lives in the framework; a backend only reports what the platform told it.
#[test]
fn the_audio_focus_policy_lives_in_the_framework() {
    let media = workspace_source("crates/cranpose-services/src/media.rs");
    assert!(
        media.contains("pub fn publish_audio_focus(") && media.contains("PAUSED_BY_FOCUS"),
        "the framework decides what a lost focus means for playback, and remembers whether it \
         was the one that paused"
    );
    for backend in MEDIA_BACKENDS {
        let source = workspace_source(backend);
        for decision in ["pause_media(", "stop_media(", "play_media("] {
            assert!(
                !source.contains(decision),
                "{backend} must publish what the device did and leave `{decision}` to the \
                 framework's one policy"
            );
        }
    }
}

/// Media that carries on with the app off screen is the one service Android
/// requires a typed foreground service for, and `dataSync` — what the
/// background-work lease starts — is not that type.
#[test]
fn android_media_declares_the_foreground_service_it_needs() {
    let manifest = workspace_source(&cranpose_manifest("media"));
    assert!(
        manifest.contains("android:foregroundServiceType=\"mediaPlayback\""),
        "playback that outlives the surface needs a mediaPlayback service"
    );
    assert!(
        manifest.contains("android.permission.FOREGROUND_SERVICE_MEDIA_PLAYBACK"),
        "the mediaPlayback service needs its own permission"
    );
    let plugin = workspace_source(CRANPOSE_GRADLE_PLUGIN);
    assert!(
        plugin.contains("\"media\","),
        "an application asks for the media service by name, so the plugin must know it"
    );
}

/// A permission an application writes into its own manifest is a permission the
/// framework's module system cannot leave out of an application that does not
/// use the service. Applications ask by service name; the module contributes
/// the permission.
#[test]
fn applications_ask_for_platform_permissions_by_service() {
    for relative in ANDROID_APPLICATION_MANIFESTS {
        let manifest = strip_xml_comments(&workspace_source(relative));
        for permission in [
            "android.permission.VIBRATE",
            "android.permission.POST_NOTIFICATIONS",
            "android.permission.CAMERA",
            "android.permission.FOREGROUND_SERVICE",
            "android.permission.SYSTEM_ALERT_WINDOW",
        ] {
            assert!(
                !manifest.contains(permission),
                "{relative} declares {permission}; a Cranpose application asks for the service \
                 that needs it through `cranpose {{ services }}` instead"
            );
        }
    }
}

/// `CranposeBilling` is the one framework class that needs the Play Billing
/// library. The plugin contributes both directly to the `billing` service, so
/// an application that sells something adds a service name — not a source
/// directory pointing into the framework's tree and a third-party dependency.
#[test]
fn the_framework_packages_its_own_billing_java() {
    let plugin = workspace_source(CRANPOSE_GRADLE_PLUGIN);
    assert!(
        plugin.contains("\"billing\" to \"java-billing\""),
        "the plugin must add the framework's billing sources for the billing service"
    );
    assert!(
        plugin.contains("com.android.billingclient:billing"),
        "the plugin must add the library that class compiles against"
    );
    for relative in ANDROID_APPLICATION_BUILD_FILES {
        let source = workspace_source(relative);
        assert!(
            !source.contains("java-billing"),
            "{relative} must not point a source set at the framework's billing sources"
        );
    }
}

/// Sharing a file out needs a content provider, because a `file://` URI has
/// been refused since Android 7. The provider is the framework's own class, so
/// the framework's manifest declares it -- an application that shares a file
/// writes nothing, and two Cranpose applications installed together do not
/// collide over one authority.
#[test]
fn the_framework_declares_the_provider_its_own_sharing_needs() {
    let library = workspace_source(&cranpose_manifest("base"));
    assert!(
        library.contains("dev.cranpose.android.CranposeShareProvider"),
        "the library manifest must declare the provider that serves shared files"
    );
    assert!(
        library.contains("${applicationId}.cranpose.share"),
        "the share provider authority must be derived from the application id"
    );
    for relative in ANDROID_APPLICATION_MANIFESTS {
        let manifest = strip_xml_comments(&workspace_source(relative));
        assert!(
            !manifest.contains("CranposeShareProvider"),
            "{relative} declares the framework's share provider; the library declares it"
        );
    }
}

/// Installing a downloaded package is a framework capability, and Android
/// refuses the installer session without a permission for it. It rides on its
/// own service module rather than the library, so an application that never
/// updates itself does not ask to install packages.
#[test]
fn installing_an_update_asks_for_its_permission_through_a_service() {
    let module = workspace_source(&cranpose_manifest("update"));
    assert!(
        module.contains("android.permission.REQUEST_INSTALL_PACKAGES"),
        "the update module must contribute the permission PackageInstaller requires"
    );
    let library = workspace_source(&cranpose_manifest("base"));
    assert!(
        !library.contains("REQUEST_INSTALL_PACKAGES"),
        "every Cranpose application would ask to install packages; keep it in the update module"
    );
    for relative in ANDROID_APPLICATION_MANIFESTS {
        let manifest = strip_xml_comments(&workspace_source(relative));
        assert!(
            !manifest.contains("REQUEST_INSTALL_PACKAGES"),
            "{relative} declares REQUEST_INSTALL_PACKAGES; add the `update` service instead"
        );
    }
}

/// The architectures a release carries are stated once, in `releaseAbis`. An
/// application that ships one APK per architecture says only that it does: the
/// plugin writes the same list into the split, so a split can never name an
/// architecture the native build never produced a library for.
#[test]
fn the_plugin_drives_abi_splits_from_the_architectures_it_builds() {
    let plugin = workspace_source(CRANPOSE_GRADLE_PLUGIN);
    assert!(
        plugin.contains("split.include(*releaseAbis.toTypedArray())"),
        "the plugin must write the release architectures into an enabled ABI split"
    );
    for relative in ANDROID_APPLICATION_BUILD_FILES {
        let source = workspace_source(relative);
        assert!(
            !source.contains("abiFilters"),
            "{relative} sets abiFilters; the plugin constrains packaging to what it builds"
        );
    }
}

/// Every service the plugin knows how to add must have a manifest fragment for
/// the plugin to contribute, or an application naming it gets a missing-file
/// failure instead of a permission.
#[test]
fn every_service_the_plugin_offers_has_a_manifest() {
    let plugin = workspace_source(CRANPOSE_GRADLE_PLUGIN);
    let known = plugin
        .split("val KNOWN_SERVICES = setOf(")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .expect("the plugin should list the services it knows");
    let services: Vec<&str> = known
        .split(',')
        .map(|entry| entry.trim().trim_matches('"'))
        .filter(|entry| !entry.is_empty())
        .collect();
    assert!(
        services.len() >= 5,
        "the plugin should know several services, found {services:?}"
    );
    for service in services {
        assert!(
            workspace_path(&cranpose_manifest(service)).is_file(),
            "the plugin offers `{service}` but has no manifest fragment for it"
        );
    }
}
