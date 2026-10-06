//! The Performance tab reads the run index the nightly publishes and shows
//! the latest night, the trend, and the framework comparison on each device.

use std::rc::Rc;

use cranpose_testing::ComposeTestRule;
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
  "device": "Apple M3 Pro",
  "subjects": [{"name": "compose", "label": "compose"}, {"name": "egui", "label": "egui"}],
  "scenarios": {"gauntlet": {"legs": 6,
    "summary": {"compose": {"fps": 17.1}, "egui": {"fps": 55.7}}, "verdicts": {}}}}
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

#[test]
fn the_frameworks_card_shows_each_frameworks_frame_rate() {
    let labels = labels();
    assert!(shows(&labels, "compose  26.5"), "{labels:?}");
    assert!(shows(&labels, "cranpose  52.8"), "{labels:?}");
}

#[test]
fn each_device_shows_its_latest_framework_comparison() {
    let labels = labels();
    assert!(shows(&labels, "egui  55.7"), "the desktop run: {labels:?}");
    assert!(
        !shows(&labels, "compose  4.7"),
        "the phone's older comparison gives way to its latest: {labels:?}"
    );
}
