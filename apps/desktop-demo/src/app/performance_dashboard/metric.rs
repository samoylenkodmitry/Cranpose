// The metrics the trend and the framework bars show, and where each chart
// reads them.

use std::cmp::Ordering;

use super::PerfRun;

/// Where a metric's chart reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Curve {
    /// The frame rate, from the times of the frames the display showed.
    Frames,
    // The present rate, from the times of the presents MangoHud counted.
    Presents,
    /// The run's samples of the metric under this key.
    Samples(&'static str),
    /// No curve, and why.
    None(&'static str),
}

impl Curve {
    // A rate of frames or of presents, which a chart scales and orders as a
    // frame rate.
    pub(super) fn is_rate(self) -> bool {
        matches!(self, Self::Frames | Self::Presents)
    }
}

/// What the trend and the framework bars show of each run's medians.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Metric {
    Fps,
    MangoHudFps,
    FirstFrame,
    CpuPerFrame,
    CpuCores,
    Ram,
    GpuRam,
    CpuClock,
    GpuClock,
    AppSize,
}

impl Metric {
    pub(super) const ALL: [Self; 9] = [
        Self::Fps,
        Self::FirstFrame,
        Self::CpuPerFrame,
        Self::CpuCores,
        Self::Ram,
        Self::GpuRam,
        Self::CpuClock,
        Self::GpuClock,
        Self::AppSize,
    ];

    // A run MangoHud counted presents in shows their rate beside the app's.
    const WITH_MANGOHUD: [Self; 10] = [
        Self::Fps,
        Self::MangoHudFps,
        Self::FirstFrame,
        Self::CpuPerFrame,
        Self::CpuCores,
        Self::Ram,
        Self::GpuRam,
        Self::CpuClock,
        Self::GpuClock,
        Self::AppSize,
    ];

    // The metrics a run's card offers.
    pub(super) fn of(run: &PerfRun) -> &'static [Self] {
        if run.kind == "esp32" {
            &[Self::Fps, Self::FirstFrame, Self::AppSize]
        } else if run.protocol.mangohud.is_some() {
            &Self::WITH_MANGOHUD
        } else {
            &Self::ALL
        }
    }

    /// The median's name in the run index.
    pub(super) fn key(self) -> &'static str {
        match self {
            Self::Fps => "fps",
            Self::MangoHudFps => "mangohud_fps",
            Self::FirstFrame => "first_frame_s",
            Self::CpuPerFrame => "cpu_ms_per_frame",
            Self::CpuCores => "cpu_cores",
            Self::Ram => "ram_mb",
            Self::GpuRam => "gpu_ram_mb",
            Self::CpuClock => "cpu_mhz",
            Self::GpuClock => "gpu_mhz",
            Self::AppSize => "app_mib",
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Fps => "fps",
            Self::MangoHudFps => "MangoHud fps",
            Self::FirstFrame => "first frame ms",
            Self::CpuPerFrame => "CPU ms per frame",
            Self::CpuCores => "CPU cores",
            Self::Ram => "RAM MB",
            Self::GpuRam => "GPU RAM MB",
            Self::CpuClock => "CPU MHz",
            Self::GpuClock => "GPU MHz",
            Self::AppSize => "App size MiB",
        }
    }

    // How `run` measured it: a board's firmware counts its own frames, and a
    // Linux run's harness the frames each app moved on to.
    pub(super) fn about_in(self, run: &PerfRun) -> &'static str {
        if run.kind == "esp32" && self != Self::AppSize {
            "The firmware counts drawn frames in each second from ESP timer startup. First frame shows milliseconds on that clock."
        } else if run.protocol.mangohud.is_some() && self == Self::Fps {
            "Frames the app moved on to per second over the whole run from the launch, each \
             line the app wrote for one stamped by the harness as it arrived, a page's at \
             the page's own clock; beside each, the presents MangoHud counted for the same \
             launch."
        } else {
            self.about()
        }
    }

    /// How each platform measures it.
    pub(super) fn about(self) -> &'static str {
        match self {
            Self::Fps => {
                "Frames the display showed per second over the whole run from the launch, counted \
                 outside the app; 60 at most."
            }
            Self::MangoHudFps => {
                "Presents per second over the whole run from the launch, counted by MangoHud in \
                 the app's Vulkan or OpenGL calls, outside the app. A row whose presents \
                 outnumber the frames the app moved on to says so, with the rate of those frames."
            }
            Self::FirstFrame => {
                "Milliseconds from the launch command to the first frame the display showed."
            }
            Self::CpuPerFrame => {
                "CPU time of the app and every process it started, per frame shown; on macOS \
                 the window server's too."
            }
            Self::CpuCores => {
                "Cores the app and every process it started kept busy: their CPU seconds per \
                 second of the run."
            }
            Self::Ram => {
                "Memory the app holds at the run's end: PSS on Android, the footprint of the \
                 app and every process it started on macOS. The chart's curve on Android is the \
                 resident memory (RSS), which counts shared pages whole: PSS costs the app a \
                 garbage collection to read."
            }
            Self::GpuRam => {
                "The GPU's part of that memory: GL and EGL mtrack on Android; Metal buffers, \
                 textures and window surfaces on macOS. Read once, at the run's end."
            }
            Self::CpuClock => {
                "Mean clock over the run of the big cores on Android, the performance cores on \
                 macOS."
            }
            Self::GpuClock => "Mean GPU clock over the run.",
            Self::AppSize => "App size counts the Android APKs, desktop build and supplied assets, browser distribution or board application image.",
        }
    }

    pub(super) fn curve_unit(self) -> &'static str {
        match self {
            Self::Fps | Self::MangoHudFps | Self::FirstFrame => "fps",
            Self::CpuPerFrame => "ms/frame",
            Self::CpuCores => "cores",
            Self::Ram | Self::GpuRam => "MB",
            Self::CpuClock | Self::GpuClock => "MHz",
            Self::AppSize => "MiB",
        }
    }

    pub(super) fn curve_format(self, value: f64) -> String {
        match self {
            Self::FirstFrame => Self::Fps.format(value),
            _ => self.format(value),
        }
    }

    pub(super) fn format(self, value: f64) -> String {
        match self {
            Self::Fps | Self::MangoHudFps | Self::CpuPerFrame => format!("{value:.1}"),
            Self::CpuCores | Self::AppSize => format!("{value:.2}"),
            // Medians in seconds, shown in milliseconds.
            Self::FirstFrame => format!("{:.0}", value * 1000.0),
            Self::Ram | Self::GpuRam | Self::CpuClock | Self::GpuClock => format!("{value:.0}"),
        }
    }

    /// Where its chart reads it: the frames the display showed, the run's
    /// samples by their key, or nowhere, with why.
    pub(super) fn curve(self) -> Curve {
        match self {
            Self::Fps | Self::FirstFrame => Curve::Frames,
            Self::MangoHudFps => Curve::Presents,
            Self::CpuPerFrame => Curve::Samples("cpu_ms_per_frame"),
            Self::CpuCores => Curve::Samples("cores"),
            Self::Ram => Curve::Samples("ram_mb"),
            Self::CpuClock => Curve::Samples("cpu_mhz"),
            Self::GpuClock => Curve::Samples("gpu_mhz"),
            Self::AppSize => Curve::None("Each build has one app size."),
            Self::GpuRam => Curve::None(
                "No curve: the GPU's memory is read once, at the run's end. On Android, \
                 reading it costs the app a garbage collection.",
            ),
        }
    }

    /// Orders two values best first: the higher frame rate, the lower of the
    /// rest, which cost time, memory or power; a value before none.
    pub(super) fn best_first(self, a: Option<f64>, b: Option<f64>) -> Ordering {
        match (a, b) {
            (Some(a), Some(b)) if matches!(self, Self::Fps | Self::MangoHudFps) => b.total_cmp(&a),
            (Some(a), Some(b)) => a.total_cmp(&b),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
    }

    /// What a full bar stands for: 60 fps, or a faster run's largest rate,
    /// for a frame rate; the run's largest value for the rest.
    pub(super) fn full_scale(self, values: &[Option<f64>]) -> f64 {
        let largest = values.iter().flatten().copied().fold(0.0, f64::max);
        match self {
            Self::Fps | Self::MangoHudFps => largest.max(60.0),
            _ => largest,
        }
    }
}
