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
    // The desktop sampler's limit on CPU use by other processes.
    #[serde(default)]
    pub max_other_cores: Option<f64>,
    /// The starts each round measured of every subject: its first after its
    /// install and its second. An older run did not keep them apart, and the
    /// dashboard leaves it out.
    #[serde(default)]
    pub starts: Vec<String>,
    // The MangoHud that counted every present of a Linux run, as `mangohud
    // --version` names it, and the present setting every subject had.
    #[serde(default)]
    pub mangohud: Option<String>,
    #[serde(default)]
    pub present: Option<String>,
}

/// A run's own file: every leg, with the time of each frame it presented.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfRunFrames {
    pub scenarios: Vec<PerfFramesScenario>,
}

#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct PerfFramesScenario {
    pub scenario: String,
    #[serde(default)]
    extras: String,
    pub legs: Vec<PerfFramesLeg>,
}

impl PerfFramesScenario {
    fn tier(&self) -> Option<u16> {
        let mut words = self.extras.split(|c: char| c.is_whitespace() || c == ',');
        words.find(|word| *word == "tier")?;
        words.next()?.parse().ok()
    }
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
    // On Linux, when the app moved on to each frame; `present_ms` holds when
    // MangoHud counted each present.
    #[serde(default)]
    pub frame_ms: Vec<f64>,
    #[serde(default)]
    pub present_ms: Vec<f64>,
    /// Another process spent more than the run allows in this leg, or the
    /// leg's page was hidden: it is shown, and left out of the medians.
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
    pub(super) fn tier(&self, scenario: &str) -> Option<u16> {
        self.scenarios
            .iter()
            .find(|case| case.scenario == scenario)?
            .tier()
    }

    pub(super) fn is_disturbed(&self, scenario: &str, start: &str, subject: &str) -> bool {
        let Some(entry) = self.scenarios.iter().find(|case| case.scenario == scenario) else {
            return false;
        };
        let mut legs = entry
            .legs
            .iter()
            .filter(|leg| leg.subject == subject && leg.start == start)
            .peekable();
        legs.peek().is_some() && legs.all(|leg| leg.disturbed)
    }

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
    /// Packaged app size in bytes for each subject that builds this screen.
    #[serde(default)]
    pub app_bytes: BTreeMap<String, u64>,
    // Subjects whose benchmark app lacked this layout at the measured revision.
    #[serde(default)]
    pub unimplemented: Vec<String>,
    /// The results of each start, by its name in [`STARTS`].
    #[serde(default)]
    pub starts: BTreeMap<String, PerfStart>,
}

impl PerfScenario {
    pub(super) fn includes(&self, name: &str) -> bool {
        if self.unimplemented.iter().any(|subject| subject == name) {
            return false;
        }
        self.app_bytes.is_empty()
            || self.app_bytes.contains_key(name)
            || self.starts.values().any(|start| {
                start.summary.contains_key(name)
                    || start.failures.contains_key(name)
                    || start.disturbed.iter().any(|subject| subject == name)
            })
    }
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
    // Subjects whose legs of the start all stayed disturbed.
    #[serde(default)]
    pub disturbed: Vec<String>,
    /// The metrics a second run judged worse again.
    #[serde(default)]
    pub confirmed: Vec<String>,
    // Subjects MangoHud logged no present of, with what it did in their
    // processes.
    #[serde(default)]
    pub no_presents: BTreeMap<String, String>,
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

    pub(super) fn duration_ms(&self) -> f64 {
        self.protocol.run_s.unwrap_or(10.0) * 1000.0
    }

    pub(super) fn omission(&self, scenario: &str, subject: &PerfSubject) -> String {
        let measured = &self.scenarios[scenario];
        // The desktop sampler first recorded only the app sizes of supported layouts.
        let desktop = self.kind == "frameworks" && self.protocol.max_other_cores.is_some();
        if measured.unimplemented.contains(&subject.name)
            || (desktop && !measured.app_bytes.is_empty())
        {
            let layout = match scenario {
                "gauntlet" => "Gauntlet",
                "patchwork" => "Patchwork",
                "wall" => "Still Wall",
                name => name,
            };
            format!(
                "The {} benchmark app did not implement {layout} in this run",
                subject.label
            )
        } else {
            format!("This run recorded no measurement for {}", subject.label)
        }
    }

    pub(super) fn start(&self, scenario: &str, start: &str) -> Option<&PerfStart> {
        self.scenarios.get(scenario)?.starts.get(start)
    }

    pub(super) fn is_disturbed(&self, scenario: &str, start: &str, subject: &str) -> bool {
        self.start(scenario, start)
            .is_some_and(|entry| entry.disturbed.iter().any(|name| name == subject))
    }

    pub(super) fn undisturbed_median(
        &self,
        scenario: &str,
        start: &str,
        subject: usize,
        metric: &str,
    ) -> Option<f64> {
        if metric != "app_mib"
            && self
                .subjects
                .get(subject)
                .is_some_and(|entry| self.is_disturbed(scenario, start, &entry.name))
        {
            return None;
        }
        self.median(scenario, start, subject, metric)
    }

    pub(super) fn median(
        &self,
        scenario: &str,
        start: &str,
        subject: usize,
        metric: &str,
    ) -> Option<f64> {
        let name = &self.subjects.get(subject)?.name;
        if metric == "app_mib" {
            return self
                .scenarios
                .get(scenario)?
                .app_bytes
                .get(name)
                .map(|bytes| *bytes as f64 / 1_048_576.0);
        }
        self.start(scenario, start)?
            .summary
            .get(name)?
            .get(metric)
            .copied()
    }

    // Where a leg of the subject presented more pictures than the frames it
    // moved on to, by MangoHud's count against the app's own, or moved on to
    // frames it never presented: what the subject's rates must say.
    pub(super) fn count_mismatch(
        &self,
        scenario: &str,
        start: &str,
        subject: usize,
    ) -> Option<&'static str> {
        let marked = |key| {
            self.median(scenario, start, subject, key)
                .is_some_and(|legs| legs > 0.0)
        };
        if marked("presents_exceed_updates_legs") {
            Some("presents exceed updates")
        } else if marked("updates_exceed_presents_legs") {
            Some("updates exceed presents")
        } else {
            None
        }
    }

    // What MangoHud did in a subject's processes when it logged no present of
    // the subject's launches.
    pub(super) fn no_presents(&self, scenario: &str, start: &str, subject: usize) -> Option<&str> {
        let name = &self.subjects.get(subject)?.name;
        self.start(scenario, start)?
            .no_presents
            .get(name)
            .map(String::as_str)
    }

    // The folder on the perf-data branch that holds MangoHud's raw log of
    // each leg of the run, beside the run's own file.
    pub(super) fn mangohud_logs(&self) -> Option<String> {
        self.protocol.mangohud.as_ref()?;
        let stem = self.file.strip_prefix("runs/")?.strip_suffix(".json")?;
        Some(format!("mangohud/{stem}"))
    }

    /// Whether the run kept each subject's first and second starts apart, as
    /// every chart shows them.
    pub(super) fn keeps_starts(&self) -> bool {
        STARTS
            .iter()
            .all(|(start, _)| self.protocol.starts.iter().any(|kept| kept == start))
    }

    // Older framework comparisons can be identified by the Compose subject.
    pub(super) fn compares_frameworks(&self) -> bool {
        matches!(self.kind.as_str(), "browser" | "frameworks" | "esp32")
            || self
                .subjects
                .iter()
                .any(|subject| subject.name == "compose")
    }

    // Where the run measured: the machine, and whether in the browser. A
    // browser run's device also names the browser's version, as `Apple M3
    // Pro · Chrome 156` does, and a new version takes the card of the old.
    fn place(&self) -> (&str, bool) {
        let machine = self.device.split(" · ").next().unwrap_or(&self.device);
        (machine, self.kind == "browser")
    }

    // Whether the run measured any launch: a run whose every launch failed
    // has nothing to show.
    fn measured(&self) -> bool {
        self.scenarios.values().any(|scenario| scenario.legs > 0)
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

    /// The latest framework comparison in each place that measured a launch.
    pub(super) fn latest_framework_runs(&self) -> Vec<&PerfRun> {
        let mut places = Vec::new();
        self.runs
            .iter()
            .rev()
            .filter(|run| {
                let first = run.compares_frameworks()
                    && run.keeps_starts()
                    && run.measured()
                    && !places.contains(&run.place());
                if first {
                    places.push(run.place());
                }
                first
            })
            .collect()
    }
}
