//! The Performance tab reads the run index the nightly publishes and shows
//! the latest night, the trend, and the framework comparison on each device.

use std::{collections::BTreeMap, rc::Rc};

use cranpose_testing::{
    robot::{create_headless_robot_test, RobotTestRule, TestRenderer},
    ComposeTestRule,
};
use cranpose_ui::Size;
use desktop_app::app::performance_dashboard::{
    launch_fps, FrameSource, LegSamples, PerfIndex, PerformanceDashboard,
};

/// Two nights and framework comparisons on two devices, the phone's twice, in
/// `publish.py`'s index format, each start apart: oldest first. The Pixel's
/// run predates keeping the starts apart.
const INDEX: &str = r#"{"runs": [
 {"file": "runs/g.json", "kind": "frameworks", "started_at": "2026-10-03T12:00:00+00:00",
  "device": "Pixel 8", "protocol": {"run_s": 10, "from_launch": true},
  "subjects": [{"name": "compose", "label": "compose"}, {"name": "cranpose", "label": "cranpose"}],
  "scenarios": {"gauntlet": {"legs": 4, "failures": {"compose": "an older run's failure"},
    "summary": {"compose": {"fps": 3.3}, "cranpose": {"fps": 7.7}}, "verdicts": {}}},
  "confirmed_regressions": {"gauntlet": ["fps"]}},
 {"file": "runs/d.json", "kind": "frameworks", "started_at": "2026-10-04T12:00:00+00:00",
  "device": "EVR-AL00", "protocol": {"run_s": 10, "starts": ["first", "second"]},
  "subjects": [{"name": "compose", "label": "compose"}, {"name": "cranpose", "label": "cranpose"}],
  "scenarios": {"gauntlet": {"legs": 4, "starts": {
    "first": {"summary": {"compose": {"fps": 4.1}, "cranpose": {"fps": 8.2}}},
    "second": {"summary": {"compose": {"fps": 4.7}, "cranpose": {"fps": 8.6}}}}}}},
 {"file": "runs/a.json", "kind": "nightly", "started_at": "2026-10-05T01:30:00+00:00",
  "device": "EVR-AL00", "main": "aaaaaaaaa111", "release": "v0.9.7", "protocol": {"run_s": 10, "starts": ["first", "second"]},
  "subjects": [{"name": "cranpose-release", "label": "v0.9.7"}, {"name": "cranpose", "label": "aaaaaaaaa"}],
  "scenarios": {"gauntlet": {"legs": 8, "starts": {
    "first": {"summary": {"cranpose-release": {"fps": 30.0, "cpu_ms_per_frame": 60.0}, "cranpose": {"fps": 31.0, "cpu_ms_per_frame": 59.0}},
              "verdicts": {"fps": "same", "cpu_ms_per_frame": "same", "desired_to_present_p50_ms": "same"}},
    "second": {"summary": {"cranpose-release": {"fps": 50.0, "cpu_ms_per_frame": 33.0, "desired_to_present_p50_ms": 26.0},
                           "cranpose": {"fps": 52.0, "cpu_ms_per_frame": 32.0, "desired_to_present_p50_ms": 26.0}},
               "verdicts": {"fps": "same", "cpu_ms_per_frame": "same", "desired_to_present_p50_ms": "same"}}}}}},
 {"file": "runs/b.json", "kind": "nightly", "started_at": "2026-10-06T01:30:00+00:00",
  "device": "EVR-AL00", "main": "bbbbbbbbb222", "release": "v0.9.8", "protocol": {"run_s": 10, "starts": ["first", "second"]},
  "subjects": [{"name": "cranpose-release", "label": "v0.9.8"}, {"name": "cranpose", "label": "bbbbbbbbb"}],
  "scenarios": {"gauntlet": {"legs": 16, "starts": {
    "first": {"summary": {"cranpose-release": {"fps": 31.0, "cpu_ms_per_frame": 61.0, "desired_to_present_p50_ms": 30.0},
                          "cranpose": {"fps": 33.5, "cpu_ms_per_frame": 58.0, "desired_to_present_p50_ms": 29.0}},
              "verdicts": {"fps": "better", "cpu_ms_per_frame": "same", "desired_to_present_p50_ms": "same"}},
    "second": {"summary": {"cranpose-release": {"fps": 52.0, "cpu_ms_per_frame": 32.0, "desired_to_present_p50_ms": 26.0},
                           "cranpose": {"fps": 49.4, "cpu_ms_per_frame": 34.0, "desired_to_present_p50_ms": 26.1}},
               "verdicts": {"fps": "worse", "cpu_ms_per_frame": "worse", "desired_to_present_p50_ms": "same"},
               "confirmed": ["cpu_ms_per_frame", "fps"]}}},
   "feed": {"legs": 8, "starts": {
    "first": {"summary": {"cranpose-release": {"fps": 58.0, "cpu_ms_per_frame": 16.1}, "cranpose": {"fps": 58.5, "cpu_ms_per_frame": 16.0}},
              "verdicts": {"fps": "same", "cpu_ms_per_frame": "same", "desired_to_present_p50_ms": "same"}},
    "second": {"summary": {"cranpose-release": {"fps": 60.0, "cpu_ms_per_frame": 14.8}, "cranpose": {"fps": 60.0, "cpu_ms_per_frame": 14.6}},
               "verdicts": {"fps": "same", "cpu_ms_per_frame": "same", "desired_to_present_p50_ms": "same"}}}}}},
 {"file": "runs/c.json", "kind": "ab", "started_at": "2026-10-05T12:00:00+00:00",
  "device": "EVR-AL00", "protocol": {"run_s": 10, "starts": ["first", "second"]},
  "subjects": [{"name": "compose", "label": "BOM 2026.09.00"}, {"name": "cranpose", "label": "main"}],
  "scenarios": {"gauntlet": {"legs": 16, "starts": {
    "first": {"summary": {"compose": {"fps": 12.0}, "cranpose": {"fps": 40.1}}},
    "second": {"summary": {"compose": {"fps": 26.5}, "cranpose": {"fps": 52.8}}}}}}},
 {"file": "runs/f.json", "kind": "browser", "started_at": "2026-10-06T06:10:00+00:00",
  "device": "Apple M3 Pro · Chrome 154", "main": "ccccccccc333", "protocol": {"run_s": 10, "starts": ["first", "second"]},
  "subjects": [
   {"name": "egui", "label": "egui 0.36.2", "source": "benchmarks/compose-vs-cranpose/egui-app/src/lib.rs"},
   {"name": "web", "label": "Web Chrome 154.0.8037.98",
    "source": "benchmarks/compose-vs-cranpose/web-app/src/gauntlet.ts"}],
  "scenarios": {"gauntlet": {"legs": 4, "starts": {
    "first": {"summary": {"egui": {"fps": 45.0, "ram_mb": 940.1}, "web": {"fps": 30.2, "ram_mb": 1010.5}}},
    "second": {"summary": {"egui": {"fps": 49.8, "ram_mb": 935.3}, "web": {"fps": 37.8, "ram_mb": 1002.7}}}}}}},
 {"file": "runs/e.json", "kind": "frameworks", "started_at": "2026-10-06T06:40:00+00:00",
  "device": "Apple M3 Pro", "main": "ccccccccc333", "protocol": {"run_s": 20, "starts": ["first", "second"], "rounds": 3},
  "subjects": [
   {"name": "egui", "label": "egui 0.36.2", "source": "benchmarks/compose-vs-cranpose/egui-app/src/lib.rs"},
   {"name": "compose", "label": "Compose Multiplatform 1.12.1",
    "source": "benchmarks/compose-vs-cranpose/shared-compose/dev/perfcompare/compose/Gauntlet.kt"},
   {"name": "avalonia", "label": "Avalonia 12.1.3", "source": "benchmarks/compose-vs-cranpose/avalonia-app/Gauntlet.cs"},
   {"name": "iced", "label": "iced 0.14.0"}],
  "scenarios": {"gauntlet": {"legs": 12, "starts": {
    "first": {"summary": {"compose": {"fps": 17.1, "first_frame_s": 2.5, "ram_mb": 412.0, "gpu_ram_mb": 96.4},
                          "egui": {"fps": 55.7, "first_frame_s": 0.9, "ram_mb": 120.4, "gpu_ram_mb": 33.0},
                          "avalonia": {"fps": 17.3, "ram_mb": 180.2}},
              "failures": {"iced": "process 4242 shows no window"}},
    "second": {"summary": {"compose": {"fps": 19.0, "first_frame_s": 2.0, "ram_mb": 400.0, "gpu_ram_mb": 95.0},
                           "egui": {"fps": 58.1, "first_frame_s": 0.6, "ram_mb": 118.0, "gpu_ram_mb": 32.0},
                           "avalonia": {"fps": 18.0, "ram_mb": 175.0}}}}}}}
]}"#;

/// The desktop run's own file: egui's first starts draw from 900 ms at 60
/// fps, its second from 600 ms; Compose's first from 2500 ms at 20 fps, its
/// second from 2000 ms. egui's first start sampled its memory, no leg its
/// cores.
fn frames() -> FrameSource {
    let steady = |first: f64, interval: f64| -> Vec<f64> {
        (0..)
            .map(|frame| first + f64::from(frame) * interval)
            .take_while(|time| *time < 3000.0)
            .collect()
    };
    let run = serde_json::json!({"scenarios": [{"scenario": "gauntlet", "legs": [
        {"subject": "egui", "start": "first", "frame_ms": steady(900.0, 1000.0 / 60.0),
         "samples": {"t_ms": [500, 1000, 2000, 3000], "ram_mb": [110, 120, 130, 150]}},
        {"subject": "egui", "start": "second", "frame_ms": steady(600.0, 1000.0 / 60.0)},
        {"subject": "compose", "start": "first", "frame_ms": steady(2500.0, 50.0)},
        {"subject": "compose", "start": "second", "frame_ms": steady(2000.0, 50.0)},
        {"subject": "compose", "start": "second", "frame_ms": steady(2000.0, 50.0), "disturbed": true}
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
    rule.set_content(move || PerformanceDashboard(Rc::clone(&index), None, source.clone()))
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

const DESKTOP_CARD: &str = "Frameworks · Apple M3 Pro · 2026-10-06";

/// Where `text` first appears after `marker`.
fn after(labels: &[String], marker: &str, text: &str) -> usize {
    let start = labels
        .iter()
        .position(|label| label == marker)
        .unwrap_or_else(|| panic!("no {marker}: {labels:?}"));
    start
        + labels[start..]
            .iter()
            .position(|label| label == text)
            .unwrap_or_else(|| panic!("no {text} after {marker}: {labels:?}"))
}

#[test]
fn the_latest_night_lists_each_scenario_of_the_start_chosen_with_both_builds() {
    let labels = labels();
    assert!(
        shows(
            &labels,
            "Nightly · v0.9.8 against main bbbbbbbbb · EVR-AL00 · 2026-10-06"
        ),
        "{labels:?}"
    );
    assert!(
        shows(&labels, "+8.1%"),
        "the first start's change, shown first: {labels:?}"
    );
    assert!(shows(&labels, "feed"), "{labels:?}");
    assert!(
        !shows(&labels, "gauntlet ⚠"),
        "the first start had no confirmed regression: {labels:?}"
    );
    let mut robot = dashboard();
    choose(&mut robot, "Second start", 0);
    let texts = robot.get_all_text();
    // The row: its name, main's fps over the release's, the change, then
    // main's CPU ms over the release's.
    let row = after(&texts, "Second start", "gauntlet ⚠");
    assert_eq!(
        texts[row + 3..row + 6],
        ["-5.0%", "34.0", "32.0"],
        "the second start's table: {texts:?}"
    );
}

#[test]
fn a_card_shows_one_start_at_a_time_as_its_chips_choose() {
    let mut robot = dashboard();
    let texts = robot.get_all_text();
    assert!(
        texts.iter().any(|text| text == "55.7") && !texts.iter().any(|text| text == "58.1"),
        "the first start, shown first: {texts:?}"
    );
    choose(&mut robot, "Second start", 2);
    let texts = robot.get_all_text();
    assert!(
        texts.iter().any(|text| text == "58.1") && !texts.iter().any(|text| text == "55.7"),
        "the desktop card's second start: {texts:?}"
    );
}

#[test]
fn a_run_that_did_not_keep_the_starts_apart_is_left_out() {
    let labels = labels();
    assert!(
        !labels.iter().any(|label| label.contains("Pixel 8")),
        "{labels:?}"
    );
}

#[test]
fn the_frameworks_card_shows_each_frameworks_frame_rate() {
    let labels = labels();
    assert!(shows(&labels, "12.0"), "{labels:?}");
    assert!(shows(&labels, "40.1"), "{labels:?}");
}

#[test]
fn each_device_shows_its_latest_framework_comparison() {
    let labels = labels();
    assert!(shows(&labels, "55.7"), "the desktop run: {labels:?}");
    assert!(
        !shows(&labels, "4.1"),
        "the phone's older comparison gives way to its latest: {labels:?}"
    );
}

#[test]
fn the_browser_runs_have_a_card_of_their_own_beside_the_native_ones() {
    let labels = labels();
    assert!(
        shows(&labels, "45.0") && shows(&labels, "30.2"),
        "the browser run, which has no Compose to give it a card: {labels:?}"
    );
    assert!(
        shows(&labels, "55.7"),
        "the Mac's native comparison stays: {labels:?}"
    );
    assert!(
        shows(
            &labels,
            "Frameworks in the browser · Apple M3 Pro · Chrome 154 · 2026-10-06"
        ),
        "the card says it is the browser's: {labels:?}"
    );
}

#[test]
fn frameworks_show_the_highest_frame_rate_first_and_the_reason_a_framework_has_none() {
    let labels = labels();
    let egui = after(&labels, DESKTOP_CARD, "egui 0.36.2");
    let avalonia = after(&labels, DESKTOP_CARD, "Avalonia 12.1.3");
    let compose = after(&labels, DESKTOP_CARD, "Compose Multiplatform 1.12.1");
    let iced = after(&labels, DESKTOP_CARD, "iced 0.14.0");
    assert!(
        egui < avalonia && avalonia < compose && compose < iced,
        "{labels:?}"
    );
    assert_eq!(
        labels[egui + 1],
        "55.7",
        "the frame rate sits on the framework's bar: {labels:?}"
    );
    assert_eq!(
        labels[iced + 1],
        "process 4242 shows no window",
        "a framework whose launches failed shows why in place of its bar: {labels:?}"
    );
    let legend = labels
        .iter()
        .filter(|label| *label == "Compose Multiplatform 1.12.1")
        .count();
    assert_eq!(
        legend, 1,
        "its bar carries its line's colour: no legend beside it: {labels:?}"
    );
    assert!(shows(&labels, "20000 ms"), "the chart's end: {labels:?}");
}

#[test]
fn the_first_frame_is_a_metric_of_each_start() {
    let mut robot = dashboard();
    choose(&mut robot, "first frame ms", 1);
    let texts = robot.get_all_text();
    for value in ["900", "2500"] {
        assert!(texts.iter().any(|text| text == value), "{value}: {texts:?}");
    }
    choose(&mut robot, "Second start", 2);
    let texts = robot.get_all_text();
    for value in ["600", "2000"] {
        assert!(texts.iter().any(|text| text == value), "{value}: {texts:?}");
    }
}

#[test]
fn the_frame_rate_since_the_launch_is_zero_before_the_first_frame_and_after_a_stall() {
    // 60 fps from 1000 ms to 2000 ms, then nothing.
    let frames: Vec<f64> = (0..=60)
        .map(|frame| 1000.0 + f64::from(frame) * 1000.0 / 60.0)
        .collect();
    let rates = launch_fps(&frames, 4000.0, 500.0);
    assert_eq!(rates.len(), 9, "0, 500, ... 4000 ms");
    assert_eq!(&rates[..2], &[0.0, 0.0], "nothing before the first frame");
    assert!((rates[3] - 60.0).abs() < 2.5, "a steady 60 fps: {rates:?}");
    assert_eq!(
        &rates[6..],
        &[0.0, 0.0, 0.0],
        "a window without a frame reads 0: {rates:?}"
    );
}

#[test]
fn frames_shown_together_at_the_start_never_read_more_than_they_are() {
    // Two frames 0.05 ms apart, then 60 fps.
    let mut frames = vec![100.0, 100.05];
    frames.extend((1..120).map(|frame| 100.05 + f64::from(frame) * 1000.0 / 60.0));
    let rates = launch_fps(&frames, 2000.0, 50.0);
    let highest = rates.iter().copied().fold(0.0, f64::max);
    assert!(highest <= 62.0, "no spike: {rates:?}");
    assert!(
        (rates[2] - 2.0).abs() < 0.01 && (rates[3] - 8.0).abs() < 0.01,
        "one frame in the last 500 ms reads 2 fps, four read 8: {rates:?}"
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
        PerformanceDashboard(Rc::clone(&index), None, source.clone());
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
fn a_framework_card_shows_the_metric_chosen_and_a_dash_where_a_run_has_none() {
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

#[test]
fn a_framework_card_charts_the_sampled_metric_chosen_on_a_scale_of_its_own() {
    let mut robot = dashboard();
    assert!(
        !robot.get_all_text().iter().any(|text| text == "150"),
        "the frame rate's grid stops at 60"
    );
    choose(&mut robot, "RAM MB", 1);
    let texts = robot.get_all_text();
    for grid in ["50", "100", "150"] {
        assert!(
            texts.iter().any(|text| text == grid),
            "egui's memory, up to 150 MB, on a grid every 50: {grid}: {texts:?}"
        );
    }
}

#[test]
fn a_metric_no_launch_sampled_or_no_one_can_sample_says_why_it_has_no_curve() {
    let mut robot = dashboard();
    choose(&mut robot, "CPU cores", 1);
    assert!(
        robot
            .get_all_text()
            .iter()
            .any(|text| text.starts_with("No launch of this start kept samples of it")),
        "a run from before the sampler"
    );
    choose(&mut robot, "GPU RAM MB", 1);
    assert!(
        robot
            .get_all_text()
            .iter()
            .any(|text| text.starts_with("No curve: the GPU's memory is read once")),
        "read once, at the run's end"
    );
}

#[test]
fn a_sample_stands_for_the_interval_since_the_one_before_and_nothing_past_the_last() {
    let samples: LegSamples =
        serde_json::from_str(r#"{"t_ms": [500, 1000, 1500], "cores": [1.0, null, 2.0]}"#)
            .expect("the samples parse");
    let cores = samples.every("cores", 2000.0, 250.0);
    let read: Vec<Option<f64>> = cores
        .iter()
        .map(|value| value.is_finite().then_some(*value))
        .collect();
    assert_eq!(
        read,
        [
            Some(1.0),
            Some(1.0),
            Some(1.0),
            None,
            None,
            Some(2.0),
            Some(2.0),
            None,
            None
        ],
        "from the launch to 500 ms the first sample; a sample without a value, and past the last, nothing"
    );
    assert!(samples.every("ram_mb", 2000.0, 250.0).is_empty());
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
    let shown = robot.get_all_text();
    let at: Vec<usize> = frameworks
        .iter()
        .map(|name| after(&shown, DESKTOP_CARD, name))
        .collect();
    assert!(
        at[0] < at[1] && at[1] < at[2],
        "egui 120, Avalonia 180, Compose 412: {at:?}"
    );
    choose(&mut robot, "GPU RAM MB", 1);
    let shown = robot.get_all_text();
    let at: Vec<usize> = frameworks
        .iter()
        .map(|name| after(&shown, DESKTOP_CARD, name))
        .collect();
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
