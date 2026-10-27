// The run index the nightly publishes on the perf-data branch, and a run's
// own file with every leg's frames.

use std::collections::BTreeMap;

use serde::Deserialize;

/// `index.json` on the perf-data branch.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfIndex {
    pub runs: Vec<PerfRun>,
}

/// One comparison: two builds on one device, scenario by scenario.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfRun {
    /// The run's own file on the branch, which holds every leg's frames.
    #[serde(default)]
    pub file: String,
    pub kind: String,
    pub started_at: String,
    pub device: String,
    #[serde(default)]
    pub main: Option<String>,
    #[serde(default)]
    pub release: Option<String>,
    pub subjects: Vec<PerfSubject>,
    pub scenarios: BTreeMap<String, PerfScenario>,
    #[serde(default)]
    pub protocol: PerfProtocol,
}

/// How a run measured each launch.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PerfProtocol {
    /// Seconds each launch was measured for, from its start.
    #[serde(default)]
    pub run_s: Option<f64>,
    /// The starts each round measured of every subject: its first after its
    /// install and its second. An older run did not keep them apart, and the
    /// dashboard leaves it out.
    #[serde(default)]
    pub starts: Vec<String>,
}

/// A run's own file: every leg, with the time of each frame it presented.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfRunFrames {
    pub scenarios: Vec<PerfFramesScenario>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfFramesScenario {
    pub scenario: String,
    pub legs: Vec<PerfFramesLeg>,
}

/// One launch of one subject.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfFramesLeg {
    pub subject: String,
    /// Which start of its round the launch was: `first`, after the subject's
    /// data was cleared as its install leaves it, or `second`.
    #[serde(default)]
    pub start: String,
    /// When each frame was presented, in milliseconds since the launch.
    #[serde(default)]
    pub frame_ms: Vec<f64>,
    /// Another process spent more than the run allows in this leg: it is
    /// shown, and left out of the medians.
    #[serde(default)]
    pub disturbed: bool,
    /// The run at each of the sampler's intervals; none in a run before the
    /// sampler.
    #[serde(default)]
    pub samples: LegSamples,
}

/// A launch sampled at each interval of its run, each sample standing for
/// the interval since the one before; the first since the launch.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct LegSamples {
    /// When each sample was taken, in milliseconds since the launch.
    #[serde(default)]
    pub t_ms: Vec<f64>,
    /// Each metric's value at each sample, by its key; none where it was not
    /// read.
    #[serde(flatten)]
    pub values: BTreeMap<String, Vec<Option<f64>>>,
}

impl LegSamples {
    /// The metric under `key` every `step_ms` up to `run_ms`: at each moment
    /// the sample whose interval holds it; not a number past the last
    /// sample, or where the sample has no value.
    pub fn every(&self, key: &str, run_ms: f64, step_ms: f64) -> Vec<f64> {
        let Some(values) = self.values.get(key) else {
            return Vec::new();
        };
        let mut sample = 0;
        (0..=(run_ms / step_ms) as usize)
            .map(|step| {
                let now = step as f64 * step_ms;
                while sample < self.t_ms.len() && self.t_ms[sample] < now {
                    sample += 1;
                }
                values.get(sample).copied().flatten().unwrap_or(f64::NAN)
            })
            .collect()
    }
}

impl PerfRunFrames {
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json)
            .map_err(|error| format!("The run's frames did not parse: {error}"))
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfSubject {
    pub name: String,
    /// The framework and the version measured, as `egui 0.36.2`.
    pub label: String,
    /// The file the app draws the gauntlet in, from the repository's root.
    #[serde(default)]
    pub source: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfScenario {
    pub legs: usize,
    /// The results of each start, by its name in [`STARTS`].
    #[serde(default)]
    pub starts: BTreeMap<String, PerfStart>,
}

/// One start's results in a scenario.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
pub struct PerfStart {
    /// Each subject's medians over its legs of the start.
    pub summary: BTreeMap<String, BTreeMap<String, f64>>,
    /// Between two builds, each deciding metric's verdict on the second.
    #[serde(default)]
    pub verdicts: BTreeMap<String, String>,
    /// Why a subject has no legs of the start: its first failure there.
    #[serde(default)]
    pub failures: BTreeMap<String, String>,
    /// The metrics a second run judged worse again.
    #[serde(default)]
    pub confirmed: Vec<String>,
}

/// The starts a run measures of every subject in each round, each with its
/// title: its first start after its install, its data cleared as the install
/// leaves it, and its second start.
pub const STARTS: [(&str, &str); 2] = [("first", "First start"), ("second", "Second start")];

impl PerfRun {
    /// The day the run started, as its start time writes it.
    pub(super) fn date(&self) -> &str {
        self.started_at.get(..10).unwrap_or(&self.started_at)
    }

    pub(super) fn start(&self, scenario: &str, start: &str) -> Option<&PerfStart> {
        self.scenarios.get(scenario)?.starts.get(start)
    }

    pub(super) fn median(
        &self,
        scenario: &str,
        start: &str,
        subject: usize,
        metric: &str,
    ) -> Option<f64> {
        let name = &self.subjects.get(subject)?.name;
        self.start(scenario, start)?
            .summary
            .get(name)?
            .get(metric)
            .copied()
    }

    /// Whether the run kept each subject's first and second starts apart, as
    /// every chart shows them.
    pub(super) fn keeps_starts(&self) -> bool {
        STARTS
            .iter()
            .all(|(start, _)| self.protocol.starts.iter().any(|kept| kept == start))
    }

    /// Whether the run compares frameworks: Compose is among its subjects,
    /// or it is the browser run, whose Compose may have failed to start.
    pub(super) fn compares_frameworks(&self) -> bool {
        self.kind == "browser"
            || self
                .subjects
                .iter()
                .any(|subject| subject.name == "compose")
    }
}

impl PerfIndex {
    /// Parses `index.json`.
    pub fn parse(json: &str) -> Result<Self, String> {
        serde_json::from_str(json).map_err(|error| format!("The run index did not parse: {error}"))
    }

    pub(super) fn nightly(&self) -> Vec<&PerfRun> {
        self.runs
            .iter()
            .filter(|run| run.kind == "nightly" && run.keeps_starts())
            .collect()
    }

    /// The latest framework comparison on each device, the newest first.
    pub(super) fn latest_framework_runs(&self) -> Vec<&PerfRun> {
        let mut devices = Vec::new();
        self.runs
            .iter()
            .rev()
            .filter(|run| {
                let first = run.compares_frameworks()
                    && run.keeps_starts()
                    && !devices.contains(&run.device.as_str());
                if first {
                    devices.push(run.device.as_str());
                }
                first
            })
            .collect()
    }
}
