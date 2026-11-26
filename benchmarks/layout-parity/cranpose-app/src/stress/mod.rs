mod container_cases;
mod scroll_cases;

use std::{
    cell::RefCell,
    fs,
    panic::{self, AssertUnwindSafe},
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use cranpose::{LazyItems, prelude::*};
use cranpose_app_shell::AppShell;
use cranpose_foundation::lazy::{LazyListScope, LazyListState, rememberLazyListState};
use cranpose_render_pixels::PixelsRenderer;

use crate::FACES;

const FRAME: Duration = Duration::from_nanos(16_666_667);

pub(super) type StateSlot<T> = Rc<RefCell<Option<T>>>;

pub(super) struct LazyChurnConfig<'a> {
    pub width: u32,
    pub height: u32,
    pub count: usize,
    pub spacing: f32,
    pub scroll_targets: &'a [usize],
    pub scroll_offset: f32,
    pub remount_after_hide: bool,
}

pub(super) fn run_toggleable_lazy_column(
    cfg: LazyChurnConfig<'_>,
    render_item: impl Fn(usize) + Copy + 'static,
) {
    let cell: StateSlot<(MutableState<bool>, LazyListState)> = Rc::new(RefCell::new(None));
    let cap = Rc::clone(&cell);
    let count = cfg.count;
    let spacing = cfg.spacing;
    let mut shell = make_shell(cfg.width, cfg.height, 1.0, move || {
        let show = rememberMutableStateOf(|| true);
        let lc = rememberLazyListState();
        *cap.borrow_mut() = Some((show, lc));
        if !show.get() {
            return;
        }
        let spec = if spacing > 0.0 {
            LazyColumnSpec::new()
                .vertical_arrangement(cranpose_ui_layout::LinearArrangement::spaced_by(spacing))
        } else {
            LazyColumnSpec::default()
        };
        LazyColumn(Modifier::empty().fill_max_size(), lc, spec, move |scope| {
            scope.items(LazyItems::new(count).key(|i| i as u64), render_item);
        });
    });
    step_frames(&mut shell, 1, 1.0);
    let (show, lc) = cell.borrow().unwrap();
    for &idx in cfg.scroll_targets {
        lc.scroll_to_item(idx, cfg.scroll_offset);
        step_frames(&mut shell, 1, 1.0);
    }
    show.set(false);
    step_frames(&mut shell, 1, 1.0);
    if cfg.remount_after_hide {
        show.set(true);
        step_frames(&mut shell, 1, 1.0);
    }
    drop(shell);
}

pub(super) struct ScenarioOutcome {
    name: &'static str,
    passed: bool,
    panic: Option<String>,
}

pub(super) fn make_shell(
    width: u32,
    height: u32,
    density: f32,
    content: impl FnMut() + 'static,
) -> AppShell<PixelsRenderer> {
    let px_w = ((width as f32) * density).round().max(1.0) as u32;
    let px_h = ((height as f32) * density).round().max(1.0) as u32;
    AppShell::new_with_size_and_density(
        PixelsRenderer::with_fonts(&FACES),
        cranpose_core::location_key(file!(), line!(), column!()),
        content,
        (px_w, px_h),
        (width as f32, height as f32),
        density,
    )
}

pub(super) fn step_frames(shell: &mut AppShell<PixelsRenderer>, count: usize, density: f32) {
    for _ in 0..count {
        shell.update_after_exact_interval(FRAME);
        let (w, h) = shell.buffer_size();
        let w = w.max(1);
        let h = h.max(1);
        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        shell.renderer().draw_scaled(&mut rgba, w, h, density);
    }
}

fn escape_json(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn write_stress_json(path: &Path, outcomes: &[ScenarioOutcome]) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut json = String::from("{\n  \"framework\": \"cranpose\",\n  \"scenarios\": [\n");
    for (idx, outcome) in outcomes.iter().enumerate() {
        let comma = if idx + 1 < outcomes.len() { "," } else { "" };
        let panic_json = match &outcome.panic {
            Some(detail) => format!("\"{}\"", escape_json(detail)),
            None => "null".to_owned(),
        };
        json.push_str(&format!(
            "    {{\"name\": \"{}\", \"passed\": {}, \"panic\": {}}}{comma}\n",
            escape_json(outcome.name),
            outcome.passed,
            panic_json,
        ));
    }
    json.push_str("  ]\n}\n");
    fs::write(path, json).expect("write cranpose stress json");
}

fn install_panic_capture(
    capture: Arc<Mutex<Option<String>>>,
) -> Box<dyn Fn(&panic::PanicHookInfo<'_>) + Send + Sync> {
    let prev_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<unknown>".to_owned());
        let msg = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "<non-string panic payload>".to_owned()
        };
        let bt = std::backtrace::Backtrace::force_capture().to_string();
        let relevant: Vec<&str> = bt
            .lines()
            .filter(|line| {
                line.contains("cranpose")
                    || line.contains("layout_parity")
                    || line.contains("remove_invalidate")
                    || line.contains("on_detach")
            })
            .take(10)
            .collect();
        let trace = if relevant.is_empty() {
            String::new()
        } else {
            format!(" | frames: {}", relevant.join(" ; "))
        };
        *capture.lock().unwrap() = Some(format!("{loc}: {msg}{trace}"));
    }));
    prev_hook
}

fn run_one(
    name: &'static str,
    last_panic: &Arc<Mutex<Option<String>>>,
    func: fn(),
) -> ScenarioOutcome {
    *last_panic.lock().unwrap() = None;
    match panic::catch_unwind(AssertUnwindSafe(func)) {
        Ok(()) => {
            println!("  [PASS] {name}");
            ScenarioOutcome {
                name,
                passed: true,
                panic: None,
            }
        }
        Err(_) => {
            let detail = last_panic
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_else(|| "<unknown panic>".to_owned());
            println!("  [PANIC] {name}\n          -> {detail}");
            ScenarioOutcome {
                name,
                passed: false,
                panic: Some(detail),
            }
        }
    }
}

const SCENARIOS: &[(&str, fn())] = &[
    (
        "drop_shell_with_full_20zone_atlas_and_scroll_nodes",
        container_cases::drop_full_atlas,
    ),
    (
        "toggle_scroll_mount_unmount_local_scroll_state",
        scroll_cases::toggle_local_scroll_state,
    ),
    (
        "toggle_scroll_hoisted_state_mutated_while_detached",
        scroll_cases::toggle_hoisted_scroll_state,
    ),
    (
        "swap_scroll_state_on_live_node",
        scroll_cases::swap_scroll_state,
    ),
    (
        "lazy_column_scroll_detaches_nested_scrolls_and_lazy_rows",
        container_cases::lazy_col_nested_scrolls,
    ),
    (
        "lazy_row_scroll_detaches_nested_vertical_scrolls",
        container_cases::lazy_row_nested_vscrolls,
    ),
    (
        "nested_scroll_both_axes_extreme_deltas",
        scroll_cases::nested_extreme_deltas,
    ),
    (
        "animated_scroll_then_immediate_unmount",
        scroll_cases::animated_scroll_unmount,
    ),
    (
        "viewport_resize_extremes_including_1x1_and_0x0",
        container_cases::viewport_resize_extremes,
    ),
    (
        "runtime_density_switches",
        container_cases::runtime_density_switches,
    ),
    (
        "dynamic_rtl_ltr_flip_full_20zone_atlas",
        container_cases::dynamic_rtl_ltr_flip,
    ),
    (
        "toggle_lazy_grid_staggered_pager_empty_full_unmount",
        container_cases::toggle_lazy_containers,
    ),
    (
        "modifier_chain_hot_permutation_on_live_node",
        container_cases::hot_modifier_permutation,
    ),
    (
        "pointer_drag_wheel_and_unmount_during_fling",
        container_cases::pointer_drag_and_unmount_mid_fling,
    ),
    (
        "subcompose_slot_churn_with_scroll_states",
        container_cases::subcompose_slot_churn,
    ),
    (
        "conditional_scroll_modifier_keyed_branch_local_scroll_state_on_detach",
        scroll_cases::conditional_keyed_scroll_detach,
    ),
    (
        "lazy_column_reuse_pool_eviction_local_scroll_states",
        scroll_cases::reuse_pool_eviction,
    ),
    (
        "same_frame_owner_and_scroll_node_removal_pure_on_detach",
        scroll_cases::same_frame_owner_and_node_removal,
    ),
    (
        "wrapped_lazy_row_in_lazy_column_scroll_and_unmount",
        container_cases::wrapped_lazy_row_in_lazy_col,
    ),
    (
        "scroll_state_owner_disposed_before_scroll_node_detaches",
        scroll_cases::owner_disposed_before_node_detaches,
    ),
];

pub fn run_cranpose_stress_suite(json_out: Option<&Path>) -> i32 {
    println!("=== Cranpose Behavioral, Detach & No-Panic Stress Suite ===");
    let last_panic: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let prev_hook = install_panic_capture(Arc::clone(&last_panic));

    let outcomes: Vec<ScenarioOutcome> = SCENARIOS
        .iter()
        .map(|&(name, func)| run_one(name, &last_panic, func))
        .collect();

    panic::set_hook(prev_hook);
    if let Some(path) = json_out {
        write_stress_json(path, &outcomes);
    }
    let passed = outcomes.iter().filter(|o| o.passed).count();
    println!(
        "Cranpose stress summary: {passed}/{} passed",
        outcomes.len()
    );
    if passed == outcomes.len() { 0 } else { 1 }
}
