//! The Performance tab reads the run index the nightly publishes and shows
//! the latest night, the trend, and the framework comparison on each device.

use std::rc::Rc;

use cranpose_testing::{
    robot::{create_headless_robot_test, RobotTestRule, TestRenderer},
    ComposeTestRule,
};
use cranpose_ui::Size;
use desktop_app::app::performance_dashboard::{PerfIndex, PerformanceDashboard};

/// Two nights and framework comparisons on two devices, the phone's twice, in
/// `publish.py`'s index format: oldest first.
const INDEX: &str = r#"{"runs": [
 {"file": "runs/d.json", "kind": "frameworks", "started_at": "2026-10-04T12:00:00+00:00",
  "device": "EVR-AL00",
  "subjects": [{"name": "compose", "label": "compose"}, {"name": "cranpose", "label": "cranpose"}],
  "scenarios": {"gauntlet": {"legs": 4,
    "summary": {"compose": {"fps": 4.7}, "cranpose": {"fps": 8.6}}, "verdicts": {}}}},
 {"file": "runs/a.json", "kind": "nightly", "started_at": "2026-10-05T01:30:00+00:00",
  "device": "EVR-AL00", "main": "aaaaaaaaa111", "release": "v0.9.7",
  "subjects": [{"name": "cranpose-release", "label": "v0.9.7"}, {"name": "cranpose", "label": "aaaaaaaaa"}],
  "scenarios": {"gauntlet": {"legs": 4,
    "summary": {"cranpose-release": {"fps": 50.0, "cpu_ms_per_frame": 33.0, "desired_to_present_p50_ms": 26.0},
                "cranpose": {"fps": 52.0, "cpu_ms_per_frame": 32.0, "desired_to_present_p50_ms": 26.0}},
    "verdicts": {"fps": "same", "cpu_ms_per_frame": "same", "desired_to_present_p50_ms": "same"}}},
  "confirmed_regressions": {}},
 {"file": "runs/b.json", "kind": "nightly", "started_at": "2026-10-06T01:30:00+00:00",
  "device": "EVR-AL00", "main": "bbbbbbbbb222", "release": "v0.9.8",
  "subjects": [{"name": "cranpose-release", "label": "v0.9.8"}, {"name": "cranpose", "label": "bbbbbbbbb"}],
  "scenarios": {"gauntlet": {"legs": 8,
    "summary": {"cranpose-release": {"fps": 52.0, "cpu_ms_per_frame": 32.0, "desired_to_present_p50_ms": 26.0},
                "cranpose": {"fps": 49.4, "cpu_ms_per_frame": 34.0, "desired_to_present_p50_ms": 26.1}},
    "verdicts": {"fps": "worse", "cpu_ms_per_frame": "worse", "desired_to_present_p50_ms": "same"}},
   "feed": {"legs": 4,
    "summary": {"cranpose-release": {"fps": 60.0, "cpu_ms_per_frame": 14.8}, "cranpose": {"fps": 60.0, "cpu_ms_per_frame": 14.6}},
    "verdicts": {"fps": "same", "cpu_ms_per_frame": "same", "desired_to_present_p50_ms": "same"}}},
  "confirmed_regressions": {"gauntlet": ["cpu_ms_per_frame", "fps"]}},
 {"file": "runs/c.json", "kind": "ab", "started_at": "2026-10-05T12:00:00+00:00",
  "device": "EVR-AL00",
  "subjects": [{"name": "compose", "label": "BOM 2026.09.00"}, {"name": "cranpose", "label": "main"}],
  "scenarios": {"gauntlet": {"legs": 8,
    "summary": {"compose": {"fps": 26.5}, "cranpose": {"fps": 52.8}},
    "verdicts": {"fps": "better", "cpu_ms_per_frame": "better", "desired_to_present_p50_ms": "worse"}}}},
 {"file": "runs/e.json", "kind": "frameworks", "started_at": "2026-10-06T06:40:00+00:00",
  "device": "Apple M3 Pro", "main": "ccccccccc333",
  "subjects": [
   {"name": "egui", "label": "egui 0.36.2", "source": "benchmarks/compose-vs-cranpose/egui-app/src/lib.rs"},
   {"name": "compose", "label": "Compose Multiplatform 1.12.1",
    "source": "benchmarks/compose-vs-cranpose/shared-compose/dev/perfcompare/compose/Gauntlet.kt"},
   {"name": "avalonia", "label": "Avalonia 12.1.3", "source": "benchmarks/compose-vs-cranpose/avalonia-app/Gauntlet.cs"}],
  "scenarios": {"gauntlet": {"legs": 6,
    "summary": {"compose": {"fps": 17.1, "ram_mb": 412.0, "gpu_ram_mb": 96.4},
                "egui": {"fps": 55.7, "ram_mb": 120.4, "gpu_ram_mb": 33.0},
                "avalonia": {"fps": 17.3, "ram_mb": 180.2}}, "verdicts": {}}}}
]}"#;

fn labels() -> Vec<String> {
    let index = Rc::new(PerfIndex::parse(INDEX).expect("the fixture parses"));
    let mut rule = ComposeTestRule::new();
    rule.set_content(move || PerformanceDashboard(index.clone(), None))
        .expect("the dashboard composes");
    let root = rule
        .placed_semantics(Size::new(1100.0, 1800.0))
        .expect("the dashboard lays out")
        .expect("the dashboard places something");
    root.flatten()
        .into_iter()
        .filter_map(|node| node.label.clone())
        .collect()
}

fn shows(labels: &[String], text: &str) -> bool {
    labels.iter().any(|label| label == text)
}

#[test]
fn the_latest_night_lists_each_scenario_with_both_builds_and_the_change() {
    let labels = labels();
    assert!(
        shows(
            &labels,
            "v0.9.8 against main bbbbbbbbb on EVR-AL00, 2026-10-06"
        ),
        "{labels:?}"
    );
    assert!(
        shows(&labels, "gauntlet ⚠"),
        "the confirmed regression is marked: {labels:?}"
    );
    assert!(
        shows(&labels, "-5.0%"),
        "main's fps change against the release: {labels:?}"
    );
    assert!(
        shows(&labels, "32.0 → 34.0"),
        "CPU per frame, release then main: {labels:?}"
    );
    assert!(shows(&labels, "feed"), "{labels:?}");
}

/// Where `text` first appears among the labels.
fn position(labels: &[String], text: &str) -> usize {
    labels
        .iter()
        .position(|label| label == text)
        .unwrap_or_else(|| panic!("no {text}: {labels:?}"))
}

#[test]
fn the_frameworks_card_shows_each_frameworks_frame_rate() {
    let labels = labels();
    assert!(shows(&labels, "26.5"), "{labels:?}");
    assert!(shows(&labels, "52.8"), "{labels:?}");
}

#[test]
fn each_device_shows_its_latest_framework_comparison() {
    let labels = labels();
    assert!(shows(&labels, "55.7"), "the desktop run: {labels:?}");
    assert!(
        !shows(&labels, "4.7"),
        "the phone's older comparison gives way to its latest: {labels:?}"
    );
}

#[test]
fn frameworks_show_their_versions_in_alphabetical_order() {
    let labels = labels();
    let avalonia = position(&labels, "Avalonia 12.1.3");
    let compose = position(&labels, "Compose Multiplatform 1.12.1");
    let egui = position(&labels, "egui 0.36.2");
    assert!(avalonia < compose && compose < egui, "{labels:?}");
    assert_eq!(
        labels[egui + 1],
        "55.7",
        "the frame rate sits on the framework's bar: {labels:?}"
    );
}

fn dashboard() -> RobotTestRule<TestRenderer> {
    let index = Rc::new(PerfIndex::parse(INDEX).expect("the fixture parses"));
    create_headless_robot_test(1100, 4000, move || {
        PerformanceDashboard(index.clone(), None)
    })
}

/// Clicks the `nth` chip named `name`, counted from the top.
fn choose(robot: &mut RobotTestRule<TestRenderer>, name: &str, nth: usize) {
    let chips: Vec<cranpose_ui::Rect> = robot
        .get_all_rects()
        .into_iter()
        .filter(|(_, text)| text.as_deref() == Some(name))
        .map(|(bounds, _)| bounds)
        .collect();
    let chip = chips
        .get(nth)
        .unwrap_or_else(|| panic!("no chip {nth} named {name}: {chips:?}"));
    assert!(robot.click_at(chip.x + chip.width / 2.0, chip.y + chip.height / 2.0));
}

#[test]
fn a_framework_card_shows_the_metric_chosen_and_leaves_a_dash_where_a_run_has_none() {
    let mut robot = dashboard();
    // The trend's chips come first, then the newest framework card's: the
    // desktop's.
    choose(&mut robot, "RAM MB", 1);
    let texts = robot.get_all_text();
    for value in ["412", "120", "180"] {
        assert!(texts.iter().any(|text| text == value), "{value}: {texts:?}");
    }
    assert!(
        !texts.iter().any(|text| text == "55.7"),
        "the desktop card no longer shows frame rates: {texts:?}"
    );
    choose(&mut robot, "GPU RAM MB", 1);
    let texts = robot.get_all_text();
    for value in ["96", "33", "–"] {
        assert!(texts.iter().any(|text| text == value), "{value}: {texts:?}");
    }
}

#[test]
fn the_trend_follows_the_metric_chosen() {
    let mut robot = dashboard();
    choose(&mut robot, "CPU MHz", 0);
    let texts = robot.get_all_text();
    assert!(
        texts
            .iter()
            .any(|text| text == "gauntlet: CPU MHz, night by night"),
        "{texts:?}"
    );
}
