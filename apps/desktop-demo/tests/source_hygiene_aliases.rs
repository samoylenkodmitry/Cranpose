use std::fs;
use std::path::PathBuf;

const CHECKED_FILES: &[&str] = &[
    "src/app.rs",
    "src/app/animations.rs",
    "src/app/hacker_news.rs",
    "src/app/images.rs",
    "src/app/lazy_list.rs",
    "src/app/markdown.rs",
    "src/app/mineswapper2.rs",
    "src/app/shaders.rs",
    "src/app/web_fetch.rs",
    "src/tests/main_tests.rs",
    "robot-runners/robot_fling_precise.rs",
    "robot-runners/robot_lazy_lifecycle.rs",
    "robot-runners/robot_lazy_list.rs",
    "robot-runners/robot_lazy_varheight_lifecycle.rs",
    "robot-runners/robot_markdown_scrollbar.rs",
    "robot-runners/robot_measure_shaders.rs",
    "robot-runners/robot_perf_harness.rs",
    "robot-runners/robot_progress_bar.rs",
    "robot-runners/robot_shader_backdrop_drag.rs",
    "robot-runners/robot_shadow_fields.rs",
    "robot-runners/robot_subcompose_invalidation.rs",
    "robot-runners/robot_subcompose_lazy.rs",
    "robot-runners/robot_tab_selection.rs",
    "robot-runners/robot_text_showcase_gradient.rs",
    "robot-runners/robot_ui_breakage.rs",
];

#[test]
fn demo_sources_do_not_introduce_identifier_alias_boilerplate() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();

    for relative_path in CHECKED_FILES {
        let path = root.join(relative_path);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));

        for (line_number, line) in source.lines().enumerate() {
            if let Some((lhs, rhs)) = bare_identifier_alias(line) {
                offenders.push(format!(
                    "{}:{}: let {} = {};",
                    relative_path,
                    line_number + 1,
                    lhs,
                    rhs
                ));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "found bare identifier alias boilerplate:\n{}",
        offenders.join("\n")
    );
}

fn bare_identifier_alias(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    if trimmed.starts_with("//") || !trimmed.starts_with("let ") || !trimmed.ends_with(';') {
        return None;
    }

    let body = trimmed.strip_prefix("let ")?.strip_suffix(';')?.trim();
    if body.contains(':') {
        return None;
    }

    let (lhs, rhs) = body.split_once('=')?;
    let lhs = lhs.trim().strip_prefix("mut ").unwrap_or(lhs.trim());
    let rhs = rhs.trim();
    if lhs.starts_with('_') || rhs.starts_with('_') {
        return None;
    }

    is_local_identifier(lhs).then_some(())?;
    is_local_identifier(rhs).then_some(())?;
    if matches!(rhs, "true" | "false") {
        return None;
    }
    Some((lhs, rhs))
}

fn is_local_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    match chars.next() {
        Some(ch) if ch == '_' || ch.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

#[test]
fn robot_runners_do_not_hardcode_tmpfs_output_paths() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("robot-runners");
    let hardcoded_tmpfs = ["/", "tmp/"].concat();
    let hardcoded_tmpfs_root = ["\"", "/", "tmp", "\""].concat();
    let process_temp_dir = ["std::env::temp_", "dir()"].concat();
    let mut offenders = Vec::new();
    collect_rust_files(&root, &mut |path| {
        let source =
            fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
        for (line_number, line) in source.lines().enumerate() {
            if line.contains(&hardcoded_tmpfs)
                || line.contains(&hardcoded_tmpfs_root)
                || line.contains(&process_temp_dir)
            {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    line_number + 1,
                    line.trim()
                ));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "robot diagnostics must write under CRANPOSE_ROBOT_OUTPUT_DIR, non-tmpfs TMPDIR, or the repo-local fallback:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn leetcodedaily_robot_checks_initial_presented_frame_health() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_path =
        root.join("robot-runners/robot_leetcodedaily_full_layout_scroll_stability.rs");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("failed to read {source_path:?}: {err}"));
    assert!(
        source.contains("fn assert_initial_leetcodedaily_frame_health("),
        "LeetCodeDaily robot must keep an explicit first-frame presented-window health check"
    );

    let driver_source = source
        .split(".with_test_driver(move |robot| {")
        .nth(1)
        .expect("LeetCodeDaily robot should define a test driver");
    let idle = driver_source
        .find("let _ = robot.wait_for_idle();")
        .expect("LeetCodeDaily robot should wait for the first idle frame");
    let health_check = driver_source
        .find("assert_initial_leetcodedaily_frame_health(&robot);")
        .expect("LeetCodeDaily robot should check first-frame health");
    let first_mode_branch = driver_source
        .find("if perf_only {")
        .expect("LeetCodeDaily robot should keep mode branches after setup");
    assert!(
        idle < health_check && health_check < first_mode_branch,
        "first-frame health must run after the initial idle frame and before optional robot branches"
    );
}

#[test]
fn leetcodedaily_x11_capture_state_is_not_process_global() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_path =
        root.join("robot-runners/robot_leetcodedaily_full_layout_scroll_stability.rs");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("failed to read {source_path:?}: {err}"));

    assert!(!source.contains("static NEXT_X11_CAPTURE_ID"));
    assert!(!source.contains("static WINDOW_ID"));
}

#[test]
fn leetcodedaily_robot_image_cache_is_not_process_global() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_path =
        root.join("robot-runners/robot_leetcodedaily_full_layout_scroll_stability.rs");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("failed to read {source_path:?}: {err}"));

    assert!(!source.contains("OnceLock<Option<ImageBitmap>>"));
    assert!(!source.contains("OnceLock<ImageBitmap>"));
    assert!(!source.contains("OnceLock<Mutex<HashMap<SharpBitmapKey, ImageBitmap>>>"));
}

#[test]
fn winamp_robot_monitor_cache_is_not_process_global() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_path = root.join("robot-runners/robot_winamp_native_window_geometry.rs");
    let source = fs::read_to_string(&source_path)
        .unwrap_or_else(|err| panic!("failed to read {source_path:?}: {err}"));

    assert!(!source.contains("static MONITORS"));
    assert!(!source.contains("OnceLock<Vec<WindowGeometry>>"));
}

#[test]
fn robot_runners_do_not_unwrap_partial_float_ordering() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("robot-runners");
    let mut offenders = Vec::new();

    collect_rust_files(&root, &mut |path| {
        let source =
            fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
        let lines = source.lines().collect::<Vec<_>>();

        for (line_index, line) in lines.iter().enumerate() {
            if !line.contains("partial_cmp(") {
                continue;
            }

            let comparison_block = lines[line_index..]
                .iter()
                .take(6)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");
            if comparison_block.contains(".unwrap()") {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    line_index + 1,
                    line.trim()
                ));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "robot runners must use total_cmp or explicit fallback ordering for float geometry:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn demo_runtime_paths_do_not_expect_entropy() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut offenders = Vec::new();

    collect_rust_files(&root, &mut |path| {
        let source =
            fs::read_to_string(path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
        for (line_number, line) in source.lines().enumerate() {
            if line.contains("getrandom::fill") && line.contains(".expect(") {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.strip_prefix(&root).unwrap_or(path).display(),
                    line_number + 1,
                    line.trim()
                ));
            }
        }
    });

    assert!(
        offenders.is_empty(),
        "demo app runtime paths must degrade without panicking when platform entropy is unavailable:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn desktop_demo_default_features_enable_http_for_network_tabs() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest_path = root.join("Cargo.toml");
    let manifest = fs::read_to_string(&manifest_path)
        .unwrap_or_else(|err| panic!("failed to read {manifest_path:?}: {err}"));

    let default_features =
        manifest_feature_value(&manifest, "default").expect("desktop demo must declare defaults");
    assert!(
        default_features.contains("\"renderer-wgpu\"")
            && default_features.contains("\"desktop-http\""),
        "desktop demo defaults must include the renderer and the native HTTP capability required by visible network tabs"
    );

    let desktop =
        manifest_feature_value(&manifest, "desktop").expect("desktop feature must be declared");
    assert!(
        desktop.contains("\"cranpose/desktop\"")
            && !desktop.contains("cranpose-services/uri-native")
            && !desktop.contains("cranpose-services/file-picker-native"),
        "desktop feature must keep only the desktop platform; service capabilities (URI, file picker, theme, notifier) come centralized through cranpose's own desktop feature"
    );

    let desktop_http = manifest_feature_value(&manifest, "desktop-http")
        .expect("desktop-http feature must be declared");
    assert!(
        desktop_http.contains("\"desktop\"")
            && desktop_http.contains("\"cranpose-services/http-native\""),
        "native HTTP must be available through an explicit desktop app feature"
    );

    let android =
        manifest_feature_value(&manifest, "android").expect("android feature must be declared");
    assert!(
        android.contains("\"cranpose/android\"")
            && android.contains("\"cranpose-services/http-native\"")
            && !android.contains("cranpose-services/uri-android"),
        "Android keeps native HTTP under the Android app feature; the URI capability comes centralized through cranpose's android feature"
    );
}

#[test]
fn binary_size_budget_targets_minimal_isolated_app() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = root
        .parent()
        .and_then(std::path::Path::parent)
        .expect("desktop demo should live under workspace/apps")
        .to_path_buf();
    let workflow = fs::read_to_string(workspace_root.join(".github/workflows/rust.yml"))
        .expect("failed to read rust workflow");

    assert!(
        workflow.contains("--package isolated-demo")
            && workflow.contains("--bin isolated-demo")
            && workflow.contains("--profile release-small")
            && workflow.contains("--patch-workspace-cranpose")
            && workflow.contains("--max-bytes 15728640"),
        "release-small binary-size gate must measure the accessibility-enabled isolated app with local Cranpose patches before the release version is published"
    );
}

#[test]
fn external_visual_contracts_cover_text_tab_after_tab_walk() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = root
        .parent()
        .and_then(std::path::Path::parent)
        .expect("desktop demo should live under workspace/apps")
        .to_path_buf();

    let visual_runner =
        fs::read_to_string(root.join("examples/robot_tab_walk_text_visual_contract.rs"))
            .expect("failed to read tab-walk visual runner");
    assert!(
        visual_runner.contains("TEST_ACTIVE_TAB_STATE")
            && visual_runner.contains("capture_x11_window")
            && visual_runner.contains("Serif Bold Italic")
            && visual_runner.contains("Decorated shadow text")
            && visual_runner.contains("is_serif_yellow_pixel"),
        "tab-walk visual contract must switch real tabs and inspect presented X11 pixels for the text corruption regression"
    );

    let text_scroll_runner =
        fs::read_to_string(root.join("robot-runners/robot_text_scroll_exact_external_contract.rs"))
            .expect("failed to read text scroll external runner");
    assert!(
        text_scroll_runner.contains("walk_tabs_to_text")
            && text_scroll_runner.contains("TEST_ACTIVE_TAB_STATE")
            && text_scroll_runner.contains("run_scroll_stability_capture"),
        "external text scroll contract must exercise the tab-walk path before checking underline scroll stability"
    );

    let markdown_scroll_runner = fs::read_to_string(
        root.join("robot-runners/robot_markdown_scroll_exact_external_contract.rs"),
    )
    .expect("failed to read markdown scroll external runner");
    let scroll_helper =
        fs::read_to_string(root.join("robot-runners/scroll_stability_external_helpers.rs"))
            .expect("failed to read scroll stability helper");
    assert!(
        markdown_scroll_runner.contains("ActiveFrameConfig")
            && markdown_scroll_runner.contains("pump_frames_after_scroll: 1")
            && scroll_helper.contains("active_frame_movement_report")
            && scroll_helper.contains("expected_score < zero_score"),
        "markdown external scroll contract must assert that the first presented X11 frame after scroll input has moved, not only the settled frame"
    );

    let underline_runner =
        fs::read_to_string(root.join("robot-runners/robot_underline_screenshot.rs"))
            .expect("failed to read underline external runner");
    assert!(
        underline_runner.contains("capture_x11_window_screenshot")
            && underline_runner.contains("underline_metrics")
            && underline_runner.contains("MAX_UNDERLINE_GAP_PX")
            && underline_runner.contains("MAX_THICKNESS_DELTA_PX")
            && underline_runner.contains("max_red_gap"),
        "underline screenshot runner must assert presented X11 underline continuity and thickness instead of only dumping screenshots"
    );

    let run_robot = fs::read_to_string(workspace_root.join("run_robot_test.sh"))
        .expect("failed to read robot runner");
    assert!(
        run_robot.contains("ROBOT_EXAMPLES_DIR=\"apps/desktop-demo/examples\"")
            && run_robot.contains("robot_tab_walk_text_visual_contract")
            && run_robot.contains("robot_underline_screenshot")
            && run_robot.contains("CRANPOSE_HEADLESS=0"),
        "robot suite must discover and run the tab-walk visual contract as a visible presented-window test"
    );

    let origin_compare =
        fs::read_to_string(workspace_root.join("scripts/x11_compare_origin_main.sh"))
            .expect("failed to read origin/main X11 comparator");
    let window_compare = fs::read_to_string(workspace_root.join("scripts/x11_compare_window.sh"))
        .expect("failed to read X11 window comparator");
    assert!(
        origin_compare.contains("--overlay-current-file")
            && window_compare.contains("--app-env")
            && window_compare.contains("app_env_vars"),
        "origin/main X11 comparator must support overlaying current harness files and holding driven robot windows for capture"
    );
}

#[test]
fn workspace_tests_do_not_default_to_tmpfs_paths() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("desktop demo should live under workspace/apps")
        .to_path_buf();
    let hardcoded_tmpfs = ["/", "tmp/"].concat();
    let hardcoded_tmpfs_root = ["\"", "/", "tmp", "\""].concat();
    let process_temp_dir = ["std::env::temp_", "dir()"].concat();
    let mut offenders = Vec::new();

    for relative_root in [
        "crates",
        "apps/desktop-demo/robot-runners",
        "apps/desktop-demo/tests",
        "xtask",
    ] {
        let root = workspace_root.join(relative_root);
        collect_rust_files(&root, &mut |path| {
            let source = fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));
            for (line_number, line) in source.lines().enumerate() {
                if line.contains(&hardcoded_tmpfs)
                    || line.contains(&hardcoded_tmpfs_root)
                    || line.contains(&process_temp_dir)
                {
                    offenders.push(format!(
                        "{}:{}: {}",
                        path.strip_prefix(&workspace_root).unwrap_or(path).display(),
                        line_number + 1,
                        line.trim()
                    ));
                }
            }
        });
    }

    assert!(
        offenders.is_empty(),
        "tests and diagnostics must write under workspace target/test-output, CRANPOSE_ROBOT_OUTPUT_DIR, non-tmpfs TMPDIR, or the repo-local robot fallback:\n{}",
        offenders.join("\n")
    );
}

#[test]
fn heavy_shell_entrypoints_use_local_resource_guards() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("desktop demo should live under workspace/apps")
        .to_path_buf();

    let shared_helper = workspace_root.join("scripts/dev_build_common.sh");
    let helper_source = fs::read_to_string(&shared_helper)
        .unwrap_or_else(|err| panic!("failed to read {shared_helper:?}: {err}"));
    for required in [
        "CRANPOSE_TMPDIR",
        "XDG_CACHE_HOME/cranpose/tmp",
        "$HOME/.cache/cranpose/tmp",
        "CRANPOSE_BUILD_JOBS",
        "CRANPOSE_GRADLE_USER_HOME",
        "GRADLE_USER_HOME",
    ] {
        assert!(
            helper_source.contains(required),
            "shared build helper must keep {required} in its local resource guard contract"
        );
    }

    for (relative_path, heavy_marker) in [
        ("cargo-dev.sh", "exec cargo"),
        ("run_robot_test.sh", "\"${CARGO_RUNNER[@]}\" build"),
        ("perf_robot_cpu.sh", "\"${CARGO_RUNNER[@]}\" build"),
        ("perf_robot_fps.sh", "\"${CARGO_RUNNER[@]}\" build"),
        ("perf_robot_heap.sh", "\"${CARGO_RUNNER[@]}\" build"),
        ("perf_slot_table_v2.sh", "run_bench()"),
        ("verify_slot_table.sh", "run_core_validation()"),
        (
            "scripts/x11_compare_window.sh",
            "cargo \"${cargo_build_args[@]}\"",
        ),
        (
            "scripts/x11_compare_downstream_cranpose.sh",
            "x11_compare_window.sh",
        ),
        ("apps/desktop-demo/build-web.sh", "build --"),
    ] {
        assert_shell_entrypoint_guarded(&workspace_root, relative_path, heavy_marker);
    }

    let web_build_source =
        fs::read_to_string(workspace_root.join("apps/desktop-demo/build-web.sh"))
            .expect("failed to read desktop demo web build script");
    let desktop_manifest = fs::read_to_string(workspace_root.join("apps/desktop-demo/Cargo.toml"))
        .expect("failed to read desktop demo manifest");
    let desktop_platform_manifest =
        fs::read_to_string(workspace_root.join("apps/desktop-demo-platform/Cargo.toml"))
            .expect("failed to read desktop demo platform manifest");
    let workspace_manifest = fs::read_to_string(workspace_root.join("Cargo.toml"))
        .expect("failed to read workspace manifest");
    assert!(
        web_build_source.contains("build --release")
            && web_build_source
                .contains("CARGO_PROFILE_RELEASE_OPT_LEVEL=\"${CARGO_PROFILE_RELEASE_OPT_LEVEL:-z}\"")
            && web_build_source
                .contains("CARGO_PROFILE_RELEASE_LTO=\"${CARGO_PROFILE_RELEASE_LTO:-thin}\"")
            && web_build_source.contains(
                "CARGO_PROFILE_RELEASE_CODEGEN_UNITS=\"${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-16}\""
            )
            && workspace_manifest.contains("[profile.wasm-release]")
            && workspace_manifest.contains("lto = \"thin\"")
            && workspace_manifest.contains("codegen-units = 16")
            && desktop_platform_manifest.contains("[package.metadata.wasm-pack.profile.release]")
            && desktop_platform_manifest.contains("\"-Oz\"")
            && desktop_platform_manifest.contains("\"--all-features\"")
            && web_build_source.contains("CRANPOSE_WEB_RELEASE_MAX_WASM_BYTES:-14680064")
            && web_build_source.contains("WASM release size budget")
            && web_build_source.contains("SIZE_BYTES=$(wc -c < pkg/desktop_app_bg.wasm")
            && !desktop_platform_manifest.contains("[package.metadata.wasm-pack.profile.custom]")
            && !desktop_manifest.contains("[package.metadata.wasm-pack.profile.wasm-release]")
            && !desktop_platform_manifest.contains("[package.metadata.wasm-pack.profile.wasm-release]"),
        "optimized web builds must use wasm-oriented Cargo release overrides, wasm-opt release metadata, and a release WASM size budget instead of native release LTO/codegen settings"
    );

    let fps_gate_source = fs::read_to_string(workspace_root.join("perf_robot_fps.sh"))
        .expect("failed to read FPS perf gate script");
    let perf_common_source =
        fs::read_to_string(workspace_root.join("scripts/perf_robot_common.sh"))
            .expect("failed to read perf robot common script");
    assert!(
        fps_gate_source.contains("MIN_FPS=\"${CRANPOSE_PERF_MIN_FPS:-120}\"")
            && fps_gate_source.contains("CRANPOSE_PERF_MIN_FPS=\"$MIN_FPS\"")
            && perf_common_source.contains("PERF_FPS_BUDGET"),
        "FPS perf gate must enforce the production >120 FPS budget through the robot harness"
    );

    let x11_compare_source =
        fs::read_to_string(workspace_root.join("scripts/x11_compare_window.sh"))
            .expect("failed to read x11 comparison script");
    assert!(
        x11_compare_source.contains("TMPDIR=\"${TMPDIR:-$(ensure_local_temp_root)}\""),
        "x11 comparison builds must use the shared local temp-root helper for fallback TMPDIR"
    );
    assert!(
        x11_compare_source.contains("--expected-size")
            && x11_compare_source.contains("baseline capture size")
            && x11_compare_source.contains("current capture size"),
        "x11 comparison must expose a hard expected capture-size guard for presented-window scale checks"
    );
    assert!(
        x11_compare_source.contains("--window-wait-ms")
            && x11_compare_source.contains("waiting up to ${window_wait_ms}ms")
            && x11_compare_source.contains("window_wait_attempts"),
        "x11 comparison must expose an explicit window-discovery budget for slow downstream launches"
    );
    assert!(
        x11_compare_source.contains("--max-content-bounds-delta-px")
            && x11_compare_source.contains("baseline content bounds")
            && x11_compare_source.contains("current content bounds"),
        "x11 comparison must expose a foreground-bounds guard for same-window scale regressions"
    );
    assert!(
        x11_compare_source.contains("--window-position")
            && x11_compare_source.contains("--window-size")
            && x11_compare_source.contains("--launch-pointer-position")
            && x11_compare_source.contains("xdotool mousemove")
            && x11_compare_source.contains("xdotool windowmove")
            && x11_compare_source.contains("xdotool windowsize"),
        "x11 comparison must be able to place and size both captures deterministically across multi-monitor X11 setups"
    );
    assert!(
        x11_compare_source.contains("xprop -id \"$window_id\" _NET_WM_PID")
            && x11_compare_source.contains("window_belongs_to_process_tree")
            && x11_compare_source.contains("process_is_descendant_of")
            && x11_compare_source.contains("app_pid"),
        "x11 comparison must prove the captured window belongs to the launched process tree"
    );
    assert!(
        x11_compare_source.contains("--expected-owner-command")
            && x11_compare_source.contains("process_command_name")
            && x11_compare_source.contains("captured X11 window")
            && x11_compare_source.contains("captured window owner"),
        "x11 comparison must reject browser/helper windows when a native app owner command is required"
    );
    assert!(
        x11_compare_source.contains("--isolated-app-home")
            && x11_compare_source.contains("XDG_CONFIG_HOME")
            && x11_compare_source.contains("XDG_DATA_HOME")
            && x11_compare_source.contains("XDG_CACHE_HOME")
            && x11_compare_source.contains("XAUTHORITY")
            && x11_compare_source.contains("$variant-home"),
        "x11 comparison must isolate app runtime state without breaking X11 authentication"
    );
    assert!(
        x11_compare_source.contains("xdotool getwindowname \"$window_id\"")
            && x11_compare_source.contains("window_exact_title"),
        "x11 comparison title mode must reject prefix/substring matches and capture the exact requested window title"
    );
    assert!(
        x11_compare_source.contains("log_phase()")
            && x11_compare_source.contains("$variant: building")
            && x11_compare_source.contains("starting baseline capture")
            && x11_compare_source.contains("starting current capture")
            && x11_compare_source.contains("comparing captures"),
        "x11 comparison must print stderr phase progress during long downstream builds"
    );
    let downstream_compare_source =
        fs::read_to_string(workspace_root.join("scripts/x11_compare_downstream_cranpose.sh"))
            .expect("failed to read downstream x11 comparison script");
    assert!(
        downstream_compare_source.contains("patch.crates-io.$package.path")
            && downstream_compare_source.contains("cranpose-render-wgpu")
            && downstream_compare_source.contains("package_in_cargo_lock")
            && downstream_compare_source.contains("skipping unlocked Cranpose package patch")
            && downstream_compare_source.contains("worktree add --detach"),
        "downstream x11 comparison must patch only locked Cranpose crates from isolated worktrees"
    );
    assert!(
        downstream_compare_source.contains("--use-real-home")
            && downstream_compare_source.contains("x11_home_args")
            && downstream_compare_source.contains("--isolated-app-home"),
        "downstream x11 comparison must default to isolated app state so persisted user data cannot look like a renderer diff"
    );
    let robot_runner_source = fs::read_to_string(workspace_root.join("run_robot_test.sh"))
        .expect("failed to read run_robot_test.sh");
    assert!(
        robot_runner_source.contains("robot_presented_window_geometry")
            && robot_runner_source.contains("robot_presented_window_hidpi_geometry")
            && robot_runner_source.contains("robot_presented_window_redraw")
            && robot_runner_source.contains("robot_renderer_micro_contract"),
        "robot runner must discover the external presented-window renderer contracts"
    );
    assert!(
        robot_runner_source.contains("robot_presented_window_geometry|robot_presented_window_hidpi_geometry|robot_presented_window_redraw|robot_renderer_micro_contract")
            && robot_runner_source.contains("headless_env=\"CRANPOSE_HEADLESS=0\""),
        "presented-window renderer contracts must run with visible X11 windows"
    );

    let verify_source = fs::read_to_string(workspace_root.join("verify_slot_table.sh"))
        .expect("failed to read verify_slot_table.sh");
    assert!(
        verify_source.contains("enable_local_gradle_home")
            && verify_source.find("enable_local_gradle_home") < verify_source.find("./gradlew"),
        "verify_slot_table.sh must configure local Gradle storage before Android verification"
    );
}

#[test]
fn robot_runner_enforces_timeouts_without_gnu_coreutils() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let workspace_root = manifest_dir
        .parent()
        .and_then(std::path::Path::parent)
        .expect("desktop demo should live under workspace/apps");
    let source = fs::read_to_string(workspace_root.join("run_robot_test.sh"))
        .expect("failed to read run_robot_test.sh");

    assert!(
        source.contains("run_with_portable_timeout")
            && source.contains("timeout_marker")
            && source.contains("kill -TERM")
            && source.contains("exit_code=124")
            && source.contains("robot_regression_fused_viewport_contract"),
        "robot runners must still time out on macOS hosts without GNU timeout"
    );
}

fn manifest_feature_value(source: &str, feature: &str) -> Option<String> {
    let prefix = format!("{feature} = ");
    let mut lines = source.lines();

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix(&prefix) else {
            continue;
        };

        let mut value = rest.to_owned();
        while !value.contains(']') {
            let next_line = lines.next()?;
            value.push('\n');
            value.push_str(next_line);
        }
        return Some(value);
    }

    None
}

fn collect_rust_files(dir: &std::path::Path, visit: &mut impl FnMut(&std::path::Path)) {
    for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("failed to read {dir:?}: {err}")) {
        let path = entry
            .unwrap_or_else(|err| panic!("failed to read entry in {dir:?}: {err}"))
            .path();
        if path.is_dir() {
            collect_rust_files(&path, visit);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
            visit(&path);
        }
    }
}

fn assert_shell_entrypoint_guarded(
    workspace_root: &std::path::Path,
    relative_path: &str,
    heavy_marker: &str,
) {
    let path = workspace_root.join(relative_path);
    let source =
        fs::read_to_string(&path).unwrap_or_else(|err| panic!("failed to read {path:?}: {err}"));

    assert!(
        source.contains("dev_build_common.sh"),
        "{relative_path} must source the shared build helper before heavy work"
    );
    assert!(
        source.contains("enable_local_tmpdir"),
        "{relative_path} must route large transient files away from tmpfs-backed defaults"
    );
    assert!(
        source.contains("enable_local_cargo_job_limit"),
        "{relative_path} must cap local cargo parallelism unless the caller opts out"
    );

    let heavy_work = source.find(heavy_marker).unwrap_or_else(|| {
        panic!("{relative_path} should contain heavy-work marker {heavy_marker:?}")
    });
    let tmpdir_call = source
        .find("enable_local_tmpdir")
        .expect("checked above: missing enable_local_tmpdir");
    let job_limit_call = source
        .find("enable_local_cargo_job_limit")
        .expect("checked above: missing enable_local_cargo_job_limit");

    assert!(
        tmpdir_call < heavy_work,
        "{relative_path} must enable the local TMPDIR before heavy work marker {heavy_marker:?}"
    );
    assert!(
        job_limit_call < heavy_work,
        "{relative_path} must limit local cargo jobs before heavy work marker {heavy_marker:?}"
    );
}
