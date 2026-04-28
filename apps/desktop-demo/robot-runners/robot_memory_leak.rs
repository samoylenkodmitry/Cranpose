//! Framework memory release regression test.
//!
//! The critical scenario: increase Recursive Layout depth to 15, then switch
//! directly back to Counter App without decreasing depth. Framework-owned
//! runtime/render memory must return to the warmed baseline after the switch.
//! RSS is logged diagnostically because allocator retention can keep the heap
//! mapped even after the framework releases its data structures.
//!
//! Run with:
//! ```bash
//! cargo run --profile robot --package desktop-app --example robot_memory_leak --features robot-app
//! ```

use cranpose::AppLauncher;
use cranpose_testing::find_text_by_prefix_in_semantics;
use desktop_app::app;
#[cfg(target_os = "linux")]
use std::collections::HashMap;
use std::time::Duration;

const WINDOW_WIDTH: u32 = 1024;
const WINDOW_HEIGHT: u32 = 768;
const SPIKE_DEPTH: usize = 15;
const MEASURE_CYCLES: usize = 1;
const VIEWPORT_TAG: &str = "RecursiveLayoutViewport";
const SET_RECURSIVE_LAYOUT_DEPTH_HOOK: &str = "desktop-demo.set_recursive_layout_depth";

#[cfg(target_os = "linux")]
fn read_rss_kb() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            let mut parts = rest.split_whitespace();
            if let Some(value) = parts.next() {
                return value.parse::<u64>().ok();
            }
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn read_rss_kb() -> Option<u64> {
    None
}

fn memory_gate_diag_enabled() -> bool {
    std::env::var_os("CRANPOSE_MEMORY_GATE_DIAG").is_some()
}

#[cfg(target_os = "linux")]
fn memory_gate_smaps_enabled() -> bool {
    std::env::var_os("CRANPOSE_MEMORY_GATE_SMAPS").is_some()
}

fn settle(robot: &cranpose::Robot, delay_ms: u64) {
    robot.wait_for_idle().expect("robot wait_for_idle");
    std::thread::sleep(Duration::from_millis(delay_ms));
}

fn click_tab(robot: &cranpose::Robot, label: &str) -> bool {
    if robot.click_by_text(label).is_ok() {
        settle(robot, 100);
        return true;
    }

    let _ = robot.move_to(512.0, 30.0);
    std::thread::sleep(Duration::from_millis(30));

    for _ in 0..20 {
        let _ = robot.mouse_scroll(-200.0, 0.0);
        std::thread::sleep(Duration::from_millis(30));
        if robot.click_by_text(label).is_ok() {
            settle(robot, 100);
            return true;
        }
    }

    for _ in 0..40 {
        let _ = robot.mouse_scroll(200.0, 0.0);
        std::thread::sleep(Duration::from_millis(30));
        if robot.click_by_text(label).is_ok() {
            settle(robot, 100);
            return true;
        }
    }

    false
}

fn scroll_tab_bar_home(robot: &cranpose::Robot) {
    let _ = robot.move_to(512.0, 30.0);
    std::thread::sleep(Duration::from_millis(30));
    for _ in 0..20 {
        let _ = robot.mouse_scroll(200.0, 0.0);
        std::thread::sleep(Duration::from_millis(15));
    }
    settle(robot, 30);
}

fn increase_depth(robot: &cranpose::Robot, target: usize) {
    let applied = robot
        .invoke_app_hook(SET_RECURSIVE_LAYOUT_DEPTH_HOOK, &target.to_string())
        .unwrap_or_else(|err| panic!("failed to invoke recursive layout depth hook: {err}"))
        .unwrap_or_else(|| panic!("recursive layout depth hook returned no depth value"));
    settle(robot, 300);
    let current = applied.parse::<usize>().unwrap_or_else(|err| {
        panic!("recursive layout depth hook returned invalid depth {applied:?}: {err}")
    });
    assert_eq!(
        current, target,
        "recursive layout depth hook failed to reach target depth",
    );
}

fn assert_active_tab_content(robot: &cranpose::Robot, expected_text: &str, tab_label: &str) {
    robot.validate_content(expected_text).unwrap_or_else(|err| {
        panic!(
            "{tab_label} did not become active after click; expected content {:?}: {err}",
            expected_text
        )
    });
}

fn assert_current_depth(robot: &cranpose::Robot, expected_depth: usize, phase: &str) {
    let current = current_depth(robot, phase);
    assert_eq!(
        current, expected_depth,
        "{phase} reported an unexpected depth label",
    );
}

fn current_depth(robot: &cranpose::Robot, phase: &str) -> usize {
    let (_, _, _, _, text) = find_text_by_prefix_in_semantics(robot, "Current depth:")
        .unwrap_or_else(|| panic!("{phase} did not expose a current depth label"));
    let Some(value) = text
        .strip_prefix("Current depth: ")
        .and_then(|value| value.parse::<usize>().ok())
    else {
        panic!("{phase} exposed a malformed current depth label: {text:?}");
    };
    value
}

fn assert_recursive_layout_visible(robot: &cranpose::Robot, expected_depth: usize, phase: &str) {
    let viewport = robot
        .find_text_bounds(VIEWPORT_TAG)
        .unwrap_or_else(|err| panic!("failed to query viewport semantics during {phase}: {err}"))
        .unwrap_or_else(|| panic!("{phase} missing recursive layout viewport tag"));
    assert!(
        viewport.2 > 1.0 && viewport.3 > 1.0,
        "{phase} recursive layout viewport has zero extent: {viewport:?}",
    );

    let root_depth = format!("Depth {expected_depth}");
    let depth_label = robot
        .find_text_bounds(&root_depth)
        .unwrap_or_else(|err| panic!("failed to query depth label during {phase}: {err}"))
        .unwrap_or_else(|| panic!("{phase} missing recursive layout depth label {root_depth:?}"));
    assert!(
        depth_label.2 > 1.0 && depth_label.3 > 1.0,
        "{phase} recursive layout depth label has zero extent: {depth_label:?}",
    );
    assert!(
        depth_label.0 >= viewport.0
            && depth_label.1 >= viewport.1
            && depth_label.0 + depth_label.2 <= viewport.0 + viewport.2
            && depth_label.1 + depth_label.3 <= viewport.1 + viewport.3,
        "{phase} depth label {root_depth:?} escaped viewport bounds: viewport={viewport:?} depth={depth_label:?}",
    );
}

fn button_center(robot: &cranpose::Robot, label: &str) -> (f32, f32) {
    let (x, y, width, height) = robot
        .find_button_bounds_exact(label)
        .unwrap_or_else(|err| panic!("failed to query button {label:?}: {err}"))
        .unwrap_or_else(|| panic!("button {label:?} missing"));
    (x + width / 2.0, y + height / 2.0)
}

fn log_render_stats(robot: &cranpose::Robot, phase: &str) {
    if !memory_gate_diag_enabled() {
        return;
    }

    match robot.get_render_stats() {
        Ok(Some(stats)) => {
            eprintln!(
                "[render-stats:{phase}] submits={} offscreen_total_mb={:.1} offscreen_pool_mb={:.1} layer_cache_entries={} layer_cache_mb={:.1} image_cache={} text_cache={}",
                stats.submits,
                stats.offscreen_total_bytes as f64 / (1024.0 * 1024.0),
                stats.offscreen_pool_bytes as f64 / (1024.0 * 1024.0),
                stats.layer_cache_size,
                stats.layer_cache_bytes as f64 / (1024.0 * 1024.0),
                stats.image_cache_size,
                stats.text_cache_size,
            );
            for (index, layer) in stats.top_isolated_layers().take(4).enumerate() {
                eprintln!(
                    "  [isolated:{phase}:{index}] node={:?} rect=({:.1},{:.1},{:.1},{:.1}) target={}x{} reasons={}",
                    layer.node_id,
                    layer.logical_rect.x,
                    layer.logical_rect.y,
                    layer.logical_rect.width,
                    layer.logical_rect.height,
                    layer.width,
                    layer.height,
                    layer.reasons.display(),
                );
            }
        }
        Ok(None) => eprintln!("[render-stats:{phase}] unavailable"),
        Err(err) => eprintln!("[render-stats:{phase}] failed: {err}"),
    }

    match robot.get_render_cpu_allocation_stats() {
        Ok(stats) => {
            eprintln!(
                "[render-cpu:{phase}] graph_nodes={} graph_heap_mb={:.1} hits={}/{} node_index={}/{} text_pool={}/{} swash_image={}/{} swash_outline={}/{} image_cache={}/{} scratch_vertices={} scratch_indices={} scratch_layers={} staged_bytes={} layer_cache={}/{} rect_cache={}/{} req_cache={}/{}",
                stats.scene_graph_node_count,
                stats.scene_graph_heap_bytes as f64 / (1024.0 * 1024.0),
                stats.scene_hits_len,
                stats.scene_hits_cap,
                stats.scene_node_index_len,
                stats.scene_node_index_cap,
                stats.text_renderer_pool_len,
                stats.text_renderer_pool_cap,
                stats.swash_image_cache_len,
                stats.swash_image_cache_cap,
                stats.swash_outline_cache_len,
                stats.swash_outline_cache_cap,
                stats.image_texture_cache_len,
                stats.image_texture_cache_cap,
                stats.scratch_vertices_cap,
                stats.scratch_indices_cap,
                stats.scratch_layer_events_cap,
                stats.staged_upload_bytes_cap,
                stats.layer_surface_cache_len,
                stats.layer_surface_cache_cap,
                stats.layer_surface_rect_cache_len,
                stats.layer_surface_rect_cache_cap,
                stats.layer_surface_requirements_cache_len,
                stats.layer_surface_requirements_cache_cap,
            );
        }
        Err(err) => eprintln!("[render-cpu:{phase}] failed: {err}"),
    }
}

fn log_runtime_stats(robot: &cranpose::Robot, phase: &str) {
    if !memory_gate_diag_enabled() {
        return;
    }

    match robot.get_runtime_leak_debug_stats() {
        Ok(stats) => {
            eprintln!(
                "[runtime:{phase}] nodes={}/{} live_heap_mb={:.1} recycled_heap_mb={:.1} slot_heap_mb={:.1} retained_slot_heap_mb={:.1} groups={}/{} payloads={}/{} payload_anchors_active={}/{} payload_anchor_slots={} payload_anchors_detached={} payload_anchors_invalidated={} payload_anchors_free={} payload_anchor_heap_kb={} payload_anchor_index={}/{} slot_nodes={}/{} pending_drops={}/{} anchors_active={}/{} anchor_slots={} anchor_sparse={} anchors_detached={} anchors_invalidated={} anchors_free={} anchor_heap_kb={} retained_subtrees={} retained_groups={} retained_payloads={} retained_nodes={} retained_scopes={} retained_anchors={} scope_index={}/{} scopes={}/{} commands={}/{} observer_states={}/{}",
                stats.applier_stats.nodes_len,
                stats.applier_stats.nodes_cap,
                stats.live_node_heap_bytes as f64 / (1024.0 * 1024.0),
                stats.recycled_node_heap_bytes as f64 / (1024.0 * 1024.0),
                stats.slot_table_heap_bytes as f64 / (1024.0 * 1024.0),
                stats.slot_stats.retained_heap_bytes as f64 / (1024.0 * 1024.0),
                stats.slot_stats.group_count,
                stats.slot_stats.group_capacity,
                stats.slot_stats.payload_count,
                stats.slot_stats.payload_capacity,
                stats.slot_stats.active_payload_anchor_count,
                stats.slot_stats.payload_anchor_capacity,
                stats.slot_stats.payload_anchor_slot_count,
                stats.slot_stats.detached_payload_anchor_count,
                stats.slot_stats.invalidated_payload_anchor_count,
                stats.slot_stats.free_payload_anchor_count,
                stats.slot_stats.payload_anchor_heap_bytes / 1024,
                stats.slot_stats.payload_anchor_index_count,
                stats.slot_stats.payload_anchor_index_capacity,
                stats.slot_stats.node_count,
                stats.slot_stats.node_capacity,
                stats.slot_stats.pending_drop_count,
                stats.slot_stats.pending_drop_capacity,
                stats.slot_stats.active_anchor_count,
                stats.slot_stats.anchor_capacity,
                stats.slot_stats.anchor_slot_count,
                stats.slot_stats.anchor_sparse_count,
                stats.slot_stats.detached_anchor_count,
                stats.slot_stats.invalidated_anchor_count,
                stats.slot_stats.free_anchor_count,
                stats.slot_stats.anchor_heap_bytes / 1024,
                stats.slot_stats.retained_subtree_count,
                stats.slot_stats.retained_group_count,
                stats.slot_stats.retained_payload_count,
                stats.slot_stats.retained_node_count,
                stats.slot_stats.retained_scope_count,
                stats.slot_stats.retained_anchor_count,
                stats.slot_stats.scope_index_count,
                stats.slot_stats.scope_index_capacity,
                stats.recompose_scope_stats.len,
                stats.recompose_scope_stats.capacity,
                stats.pass_stats.commands_len,
                stats.pass_stats.commands_cap,
                stats.observer_stats.observed_state_count,
                stats.observer_stats.observed_state_capacity,
            );
        }
        Err(err) => eprintln!("[runtime:{phase}] failed: {err}"),
    }
}

#[cfg(target_os = "linux")]
fn smaps_mapping_name(header: &str) -> String {
    let mut parts = header.split_whitespace();
    let _range = parts.next();
    let _perms = parts.next();
    let _offset = parts.next();
    let _dev = parts.next();
    let _inode = parts.next();
    let remainder = parts.collect::<Vec<_>>().join(" ");
    if remainder.is_empty() {
        "[anon]".to_string()
    } else {
        remainder
    }
}

#[cfg(target_os = "linux")]
fn log_smaps_top(phase: &str) {
    if !memory_gate_smaps_enabled() {
        return;
    }

    let Ok(smaps) = std::fs::read_to_string("/proc/self/smaps") else {
        eprintln!("[smaps:{phase}] failed to read /proc/self/smaps");
        return;
    };

    let mut current_mapping = String::from("[unknown]");
    let mut rss_by_mapping: HashMap<String, (u64, usize)> = HashMap::new();

    for line in smaps.lines() {
        let is_header = line
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_hexdigit())
            && line.contains('-');
        if is_header {
            current_mapping = smaps_mapping_name(line);
            continue;
        }

        if let Some(rest) = line.strip_prefix("Rss:") {
            let Some(value_kb) = rest
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u64>().ok())
            else {
                continue;
            };
            let entry = rss_by_mapping
                .entry(current_mapping.clone())
                .or_insert((0, 0));
            entry.0 += value_kb;
            entry.1 += 1;
        }
    }

    let mut top_mappings: Vec<_> = rss_by_mapping.into_iter().collect();
    top_mappings.sort_by(|(_, (rss_a, _)), (_, (rss_b, _))| rss_b.cmp(rss_a));

    eprintln!("[smaps:{phase}] top mappings by RSS:");
    for (index, (name, (rss_kb, regions))) in top_mappings.into_iter().take(12).enumerate() {
        eprintln!(
            "  [smaps:{phase}:{index}] rss_mb={:.1} regions={} name={}",
            rss_kb as f64 / 1024.0,
            regions,
            name,
        );
    }
}

fn main() {
    env_logger::init();
    println!("=== Robot Memory Leak Test ===");
    println!(
        "Scenario: depth 3→{} then switch tab without decreasing depth, {} cycle",
        SPIKE_DEPTH, MEASURE_CYCLES,
    );

    AppLauncher::new()
        .with_title("Robot Memory Leak Test")
        .with_size(WINDOW_WIDTH, WINDOW_HEIGHT)
        .with_headless(true)
        .with_robot_app_hook(|name, argument| match name.as_str() {
            SET_RECURSIVE_LAYOUT_DEPTH_HOOK => {
                let depth = argument
                    .parse::<usize>()
                    .map_err(|err| format!("invalid recursive depth {argument:?}: {err}"))?
                    .max(1);
                desktop_app::app::TEST_RECURSIVE_LAYOUT_DEPTH_STATE.with(|cell| {
                    let state = (*cell.borrow())
                        .ok_or_else(|| "recursive layout depth state unavailable".to_string())?;
                    state.set(depth);
                    Ok(Some(state.get().to_string()))
                })
            }
            _ => Err(format!("unknown robot app hook: {name}")),
        })
        .with_test_driver(|robot| {
            std::thread::sleep(Duration::from_secs(2));

            if read_rss_kb().is_none() {
                eprintln!("Skipping robot_memory_leak: RSS measurement unsupported on this platform");
                robot.exit().ok();
                return;
            }

            scroll_tab_bar_home(&robot);
            assert!(click_tab(&robot, "Counter App"), "Counter App not found");
            assert_active_tab_content(&robot, "Increment", "Counter App");
            let counter_tab_center = button_center(&robot, "Counter App");

            assert!(
                click_tab(&robot, "Recursive Layout"),
                "Recursive Layout not found"
            );
            assert_active_tab_content(&robot, "Increase depth", "Recursive Layout");
            assert_current_depth(&robot, 3, "warmup");
            assert_recursive_layout_visible(&robot, 3, "warmup");

            assert!(click_tab(&robot, "Counter App"), "Counter App not found");
            assert_active_tab_content(&robot, "Increment", "Counter App");
            settle(&robot, 500);

            let baseline = read_rss_kb().expect("RSS became unavailable on a supported platform");
            let baseline_render_cpu = robot
                .get_render_cpu_allocation_stats()
                .expect("baseline render CPU allocation stats");
            let baseline_runtime = robot
                .get_runtime_leak_debug_stats()
                .expect("baseline runtime leak stats");
            log_render_stats(&robot, "baseline");
            log_runtime_stats(&robot, "baseline");
            #[cfg(target_os = "linux")]
            log_smaps_top("baseline");
            println!("\n--- Baseline: {:.1} MB ---", baseline as f64 / 1024.0);

            let mut all_passed = true;

            for cycle in 1..=MEASURE_CYCLES {
                assert!(
                    click_tab(&robot, "Recursive Layout"),
                    "Recursive Layout not found"
                );
                assert_active_tab_content(&robot, "Increase depth", "Recursive Layout");
                assert_current_depth(&robot, 3, "before spike");
                assert_recursive_layout_visible(&robot, 3, "before spike");
                robot
                    .set_semantics_enabled(false)
                    .expect("disable semantics for spike");

                increase_depth(&robot, SPIKE_DEPTH);

                let peak =
                    read_rss_kb().expect("RSS became unavailable on a supported platform");
                log_render_stats(&robot, "peak");
                log_runtime_stats(&robot, "peak");
                #[cfg(target_os = "linux")]
                log_smaps_top("peak");

                scroll_tab_bar_home(&robot);
                robot
                    .click(counter_tab_center.0, counter_tab_center.1)
                    .expect("click cached Counter App tab");
                settle(&robot, 100);
                robot
                    .set_semantics_enabled(true)
                    .expect("re-enable semantics after spike");
                settle(&robot, 100);
                assert_active_tab_content(&robot, "Increment", "Counter App");
                settle(&robot, 500);

                let after =
                    read_rss_kb().expect("RSS became unavailable on a supported platform");
                let after_render_cpu = robot
                    .get_render_cpu_allocation_stats()
                    .expect("after render CPU allocation stats");
                let after_runtime = robot
                    .get_runtime_leak_debug_stats()
                    .expect("after runtime leak stats");
                log_render_stats(&robot, "after");
                log_runtime_stats(&robot, "after");
                #[cfg(target_os = "linux")]
                log_smaps_top("after");
                let growth = after as i64 - baseline as i64;
                let mut framework_issues = Vec::new();
                let baseline_nodes_cap = baseline_runtime.applier_stats.nodes_cap.max(256);
                if after_runtime.applier_stats.nodes_len
                    > baseline_runtime.applier_stats.nodes_len + 24
                {
                    framework_issues.push(format!(
                        "active nodes grew from {} to {}",
                        baseline_runtime.applier_stats.nodes_len,
                        after_runtime.applier_stats.nodes_len
                    ));
                }
                if after_runtime.applier_stats.nodes_cap > baseline_nodes_cap * 4 {
                    framework_issues.push(format!(
                        "node capacity grew from {} to {}",
                        baseline_runtime.applier_stats.nodes_cap,
                        after_runtime.applier_stats.nodes_cap
                    ));
                }
                if after_runtime.slot_stats.group_count > baseline_runtime.slot_stats.group_count + 128
                {
                    framework_issues.push(format!(
                        "group count grew from {} to {}",
                        baseline_runtime.slot_stats.group_count,
                        after_runtime.slot_stats.group_count
                    ));
                }
                if after_runtime.slot_stats.payload_count
                    > baseline_runtime.slot_stats.payload_count + 256
                {
                    framework_issues.push(format!(
                        "payload count grew from {} to {}",
                        baseline_runtime.slot_stats.payload_count,
                        after_runtime.slot_stats.payload_count
                    ));
                }
                if after_runtime.slot_stats.node_count > baseline_runtime.slot_stats.node_count + 128
                {
                    framework_issues.push(format!(
                        "slot node count grew from {} to {}",
                        baseline_runtime.slot_stats.node_count,
                        after_runtime.slot_stats.node_count
                    ));
                }
                if after_runtime.live_node_heap_bytes
                    > baseline_runtime.live_node_heap_bytes + 256 * 1024
                {
                    framework_issues.push(format!(
                        "live node heap grew from {} to {} bytes",
                        baseline_runtime.live_node_heap_bytes, after_runtime.live_node_heap_bytes
                    ));
                }
                if after_runtime.recycled_node_heap_bytes
                    > baseline_runtime.recycled_node_heap_bytes + 512 * 1024
                {
                    framework_issues.push(format!(
                        "recycled node heap grew from {} to {} bytes",
                        baseline_runtime.recycled_node_heap_bytes,
                        after_runtime.recycled_node_heap_bytes
                    ));
                }
                if after_runtime.slot_table_heap_bytes
                    > baseline_runtime.slot_table_heap_bytes + 2 * 1024 * 1024
                {
                    framework_issues.push(format!(
                        "slot table heap grew from {} to {} bytes",
                        baseline_runtime.slot_table_heap_bytes, after_runtime.slot_table_heap_bytes
                    ));
                }
                if after_runtime.recompose_scope_stats.len
                    > baseline_runtime.recompose_scope_stats.len + 32
                {
                    framework_issues.push(format!(
                        "recompose scopes grew from {} to {}",
                        baseline_runtime.recompose_scope_stats.len,
                        after_runtime.recompose_scope_stats.len
                    ));
                }
                if after_runtime.observer_stats.observed_state_count
                    > baseline_runtime.observer_stats.observed_state_count + 16
                {
                    framework_issues.push(format!(
                        "observed state count grew from {} to {}",
                        baseline_runtime.observer_stats.observed_state_count,
                        after_runtime.observer_stats.observed_state_count
                    ));
                }
                if after_render_cpu.scene_graph_node_count
                    > baseline_render_cpu.scene_graph_node_count + 256
                {
                    framework_issues.push(format!(
                        "scene graph node count grew from {} to {}",
                        baseline_render_cpu.scene_graph_node_count,
                        after_render_cpu.scene_graph_node_count
                    ));
                }
                if after_render_cpu.scene_graph_heap_bytes
                    > baseline_render_cpu.scene_graph_heap_bytes + 512 * 1024
                {
                    framework_issues.push(format!(
                        "scene graph heap grew from {} to {} bytes",
                        baseline_render_cpu.scene_graph_heap_bytes,
                        after_render_cpu.scene_graph_heap_bytes
                    ));
                }
                if after_render_cpu.layer_surface_cache_len
                    > baseline_render_cpu.layer_surface_cache_len + 4
                {
                    framework_issues.push(format!(
                        "layer surface cache len grew from {} to {}",
                        baseline_render_cpu.layer_surface_cache_len,
                        after_render_cpu.layer_surface_cache_len
                    ));
                }

                println!(
                    "  Cycle {cycle}: peak={:.1} MB, after switch={:.1} MB, growth={growth:+} KB",
                    peak as f64 / 1024.0,
                    after as f64 / 1024.0,
                );

                if framework_issues.is_empty() {
                    if growth > 0 {
                        eprintln!(
                            "NOTE: cycle {cycle} retained {growth} KB of RSS even though framework-owned memory returned to baseline",
                        );
                    }
                } else {
                    eprintln!(
                        "FAIL: cycle {cycle} retained framework memory after tab switch:",
                    );
                    for issue in framework_issues {
                        eprintln!("  - {issue}");
                    }
                    eprintln!(
                        "  RSS diagnostic: baseline={baseline} KB, after={after} KB, growth={growth:+} KB",
                    );
                    all_passed = false;
                }
            }

            println!();
            if !all_passed {
                eprintln!("=== MEMORY LEAK DETECTED ===");
                std::process::exit(1);
            }
            println!("=== ALL TESTS PASSED (0 bytes growth) ===");
            robot.exit().ok();
        })
        .run(app::combined_app);
}
