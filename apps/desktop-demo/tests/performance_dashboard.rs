//! The Performance tab reads the run index the nightly publishes and shows
//! the latest night, the trend, and the framework comparison on each device.

use std::{collections::BTreeMap, rc::Rc};

use cranpose_testing::{
    robot::{create_headless_robot_test, RobotTestRule, TestRenderer},
    ComposeTestRule,
};
use cranpose_ui::Size;
use desktop_app::app::performance_dashboard::{
    launch_fps, FrameSource, PerfIndex, PerformanceDashboard,
};

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
 {"file": "runs/f.json", "kind": "browser", "started_at": "2026-10-06T06:10:00+00:00",
  "device": "Apple M3 Pro · Chrome 154", "main": "ccccccccc333",
  "subjects": [
   {"name": "egui", "label": "egui 0.36.2", "source": "benchmarks/compose-vs-cranpose/egui-app/src/lib.rs"},
   {"name": "web", "label": "Web Chrome 154.0.8037.98",
    "source": "benchmarks/compose-vs-cranpose/web-app/src/gauntlet.ts"}],
  "scenarios": {"gauntlet": {"legs": 4,
    "summary": {"egui": {"fps": 49.8, "ram_mb": 935.3}, "web": {"fps": 37.8, "ram_mb": 1002.7}},
    "verdicts": {}}}},
 {"file": "runs/e.json", "kind": "frameworks", "started_at": "2026-10-06T06:40:00+00:00",
  "device": "Apple M3 Pro", "main": "ccccccccc333", "protocol": {"run_s": 20, "from_launch": true, "rounds": 3},
  "subjects": [
   {"name": "egui", "label": "egui 0.36.2", "source": "benchmarks/compose-vs-cranpose/egui-app/src/lib.rs"},
   {"name": "compose", "label": "Compose Multiplatform 1.12.1",
    "source": "benchmarks/compose-vs-cranpose/shared-compose/dev/perfcompare/compose/Gauntlet.kt"},
   {"name": "avalonia", "label": "Avalonia 12.1.3", "source": "benchmarks/compose-vs-cranpose/avalonia-app/Gauntlet.cs"}],
  "scenarios": {"gauntlet": {"legs": 6,
    "summary": {"compose": {"fps": 17.1, "ram_mb": 412.0, "gpu_ram_mb": 96.4},
                "egui": {"fps": 55.7, "ram_mb": 120.4, "gpu_ram_mb": 33.0},
                "avalonia": {"fps": 17.3, "ram_mb": 180.2}}, "verdicts": {},
    "failures": {"avalonia": "process 4242 shows no window"}}}}
]}"#;

/// The desktop run's own file: egui draws from 600 ms at 60 fps, Compose from
/// 2000 ms at 20 fps.
fn frames() -> FrameSource {
    let steady = |first: f64, interval: f64| -> Vec<f64> {
        (0..)
            .map(|frame| first + f64::from(frame) * interval)
            .take_while(|time| *time < 3000.0)
            .collect()
    };
    let run = serde_json::json!({"scenarios": [{"scenario": "gauntlet", "legs": [
        {"subject": "egui", "frame_ms": steady(600.0, 1000.0 / 60.0)},
        {"subject": "compose", "frame_ms": steady(2000.0, 50.0)},
        {"subject": "compose", "frame_ms": steady(2000.0, 50.0), "disturbed": true}
    ]}]});
    FrameSource::Files(Rc::new(BTreeMap::from([(
        "runs/e.json".to_string(),
        run.to_string(),
    )])))
}

fn labels() -> Vec<String> {
    let index = Rc::new(PerfIndex::parse(INDEX).expect("the fixture parses"));
    let source = frames();
    let mut rule = ComposeTestRule::new();
    rule.set_content(move || PerformanceDashboard(index.clone(), None, source.clone()))
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

/// Where the first chart's caption ends: the framework bars follow it, after
/// the chart's legend.
fn bars_start(labels: &[String]) -> usize {
    let caption = labels
        .iter()
        .position(|label| label.starts_with("Frame rate against milliseconds since the launch"))
        .unwrap_or_else(|| panic!("no chart: {labels:?}"));
    // The legend: a label, its frame rate and its first frame, or why it has
    // none, for each of the run's three frameworks.
    caption + 1 + 3 * 3
}

/// Where `text` first appears among the bars.
fn bar_position(labels: &[String], text: &str) -> usize {
    let start = bars_start(labels);
    start
        + labels[start..]
            .iter()
            .position(|label| label == text)
            .unwrap_or_else(|| panic!("no {text} among the bars: {labels:?}"))
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
fn the_browser_runs_have_a_card_of_their_own_beside_the_native_ones() {
    let labels = labels();
    assert!(
        shows(&labels, "49.8") && shows(&labels, "37.8"),
        "the browser run, which has no Compose to give it a card: {labels:?}"
    );
    assert!(
        shows(&labels, "55.7"),
        "the Mac's native comparison stays: {labels:?}"
    );
    assert!(
        shows(
            &labels,
            "Frameworks in the browser, Apple M3 Pro · Chrome 154, 2026-10-06"
        ),
        "the card says it is the browser's: {labels:?}"
    );
}

#[test]
fn frameworks_show_the_highest_frame_rate_first() {
    let labels = labels();
    let egui = bar_position(&labels, "egui 0.36.2");
    let avalonia = bar_position(&labels, "Avalonia 12.1.3");
    let compose = bar_position(&labels, "Compose Multiplatform 1.12.1");
    assert!(egui < avalonia && avalonia < compose, "{labels:?}");
    assert_eq!(
        labels[egui + 1],
        "55.7",
        "the frame rate sits on the framework's bar: {labels:?}"
    );
}

#[test]
fn a_run_measured_from_each_launch_charts_every_framework_from_its_launch() {
    let labels = labels();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("over the whole 20000 ms from the launch")),
        "the chart says what its numbers are: {labels:?}"
    );
    let legend = labels
        .iter()
        .filter(|label| *label == "Compose Multiplatform 1.12.1")
        .count();
    assert_eq!(legend, 2, "the chart's legend and the bars: {labels:?}");
    assert!(
        shows(&labels, "first frame 600 ms"),
        "egui's first frame: {labels:?}"
    );
    assert!(
        shows(&labels, "first frame 2000 ms"),
        "Compose's first frame: {labels:?}"
    );
    assert!(shows(&labels, "20000 ms"), "the chart's end: {labels:?}");
    assert!(
        shows(&labels, "no frames: process 4242 shows no window"),
        "a framework whose launches failed is shown with why: {labels:?}"
    );
}

#[test]
fn a_run_measured_after_a_warm_up_says_it_kept_no_frame_times() {
    let labels = labels();
    assert!(
        shows(
            &labels,
            "This run measured a window after a warm-up and kept no frame times from the launch."
        ),
        "the older browser run: {labels:?}"
    );
}

#[test]
fn the_frame_rate_since_the_launch_is_zero_before_the_first_frame_and_falls_in_a_stall() {
    // 60 fps from 1000 ms to 2000 ms, then nothing.
    let frames: Vec<f64> = (0..=60)
        .map(|frame| 1000.0 + f64::from(frame) * 1000.0 / 60.0)
        .collect();
    let rates = launch_fps(&frames, 4000.0, 500.0);
    assert_eq!(rates.len(), 9, "0, 500, ... 4000 ms");
    assert_eq!(&rates[..2], &[0.0, 0.0], "nothing before the first frame");
    assert!((rates[3] - 60.0).abs() < 0.5, "a steady 60 fps: {rates:?}");
    assert!(
        (rates[6] - 1.0).abs() < 0.01,
        "a second without a frame reads 1 fps: {rates:?}"
    );
    assert!(
        (rates[8] - 0.5).abs() < 0.01,
        "two seconds without one, 0.5: {rates:?}"
    );
}

#[test]
fn frames_that_alternate_short_and_long_read_their_average_rate() {
    // 16.7 ms then 33.3 ms, over and over: 40 frames a second.
    let mut frames = vec![0.0];
    while frames.len() < 200 {
        let last = frames[frames.len() - 1];
        let next = if frames.len() % 2 == 1 {
            1000.0 / 60.0
        } else {
            1000.0 / 30.0
        };
        frames.push(last + next);
    }
    let rates = launch_fps(&frames, 4000.0, 250.0);
    for rate in &rates[4..] {
        assert!(
            (rate - 40.0).abs() < 2.5,
            "the average, not 60 or 30: {rates:?}"
        );
    }
}

fn dashboard() -> RobotTestRule<TestRenderer> {
    let index = Rc::new(PerfIndex::parse(INDEX).expect("the fixture parses"));
    let source = frames();
    create_headless_robot_test(1100, 4000, move || {
        PerformanceDashboard(index.clone(), None, source.clone());
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
        "the desktop card's bars no longer show frame rates: {texts:?}"
    );
    choose(&mut robot, "GPU RAM MB", 1);
    let texts = robot.get_all_text();
    for value in ["96", "33", "–"] {
        assert!(texts.iter().any(|text| text == value), "{value}: {texts:?}");
    }
}

/// Where each of `texts` first appears among the bars.
fn order(robot: &mut RobotTestRule<TestRenderer>, texts: &[&str]) -> Vec<usize> {
    let shown = robot.get_all_text();
    texts
        .iter()
        .map(|text| bar_position(&shown, text))
        .collect()
}

#[test]
fn memory_puts_the_least_first_and_a_framework_without_a_value_last() {
    let mut robot = dashboard();
    let frameworks = [
        "egui 0.36.2",
        "Avalonia 12.1.3",
        "Compose Multiplatform 1.12.1",
    ];
    choose(&mut robot, "RAM MB", 1);
    let at = order(&mut robot, &frameworks);
    assert!(
        at[0] < at[1] && at[1] < at[2],
        "egui 120, Avalonia 180, Compose 412: {at:?}"
    );
    choose(&mut robot, "GPU RAM MB", 1);
    let at = order(&mut robot, &frameworks);
    assert!(
        at[0] < at[2] && at[2] < at[1],
        "egui 33, Compose 96, Avalonia none: {at:?}"
    );
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
