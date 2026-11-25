// In-app measurement explainability card and links to the methodology doc and
// external benchmark harnesses.

use cranpose_core::rememberMutableStateOf;
use cranpose_services::local_uri_handler;
use cranpose_ui::{
    composable,
    text::FontWeight,
    widgets::{FlowRow, FlowSpec},
    Button, ButtonSpec, Color, Column, ColumnSpec, LinearArrangement, Modifier, Row, RowSpec, Text,
    VerticalAlignment,
};

use super::{
    super::demo_tab::{local_active_tab, DemoTab},
    text_style, Palette, SOURCE_URL,
};

const METHODOLOGY_URL: &str =
    "https://github.com/samoylenkodmitry/Cranpose/blob/main/docs/performance_methodology.md";

#[derive(Clone, Copy, PartialEq)]
enum ChipStyle {
    Accent,
    Plain,
}

const HARNESS_LINKS: [(&str, &str); 4] = [
    ("desktop.py", "benchmarks/compose-vs-cranpose/desktop.py"),
    (
        "FrameCount.swift",
        "benchmarks/compose-vs-cranpose/framecount/FrameCount.swift",
    ),
    (
        "perf_window.sh",
        "benchmarks/compose-vs-cranpose/perf_window.sh",
    ),
    (
        "Benchmark README",
        "benchmarks/compose-vs-cranpose/README.md",
    ),
];

#[composable]
pub(super) fn MethodologyCard(palette: Palette) {
    let expanded = rememberMutableStateOf(|| false);
    Column(
        Modifier::empty()
            .fill_max_width()
            .background(palette.surface)
            .rounded_corners(14.0)
            .padding(14.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(8.0)),
        move || {
            Row(
                Modifier::empty().fill_max_width(),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::SpaceBetween)
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Text(
                        "How benchmarks are measured",
                        Modifier::empty(),
                        text_style(15.0, palette.text, Some(FontWeight::BOLD)),
                    );
                    Row(
                        Modifier::empty(),
                        RowSpec::new()
                            .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
                            .vertical_alignment(VerticalAlignment::CenterVertically),
                        move || {
                            LinkChip(
                                palette,
                                "Full methodology",
                                METHODOLOGY_URL,
                                ChipStyle::Accent,
                            );
                            let is_open = expanded.get();
                            Button(
                                Modifier::empty()
                                    .background(palette.background)
                                    .rounded_corners(14.0)
                                    .padding_symmetric(10.0, 4.0),
                                ButtonSpec::default(),
                                move || expanded.set(!is_open),
                                move || {
                                    Text(
                                        if is_open {
                                            "Hide details"
                                        } else {
                                            "More details"
                                        },
                                        Modifier::empty(),
                                        text_style(12.0, palette.text, None),
                                    );
                                },
                            );
                        },
                    );
                },
            );
            Text(
                "External tools measure desktop and Android apps. macOS runs \
                 use FrameCount.app (ScreenCaptureKit pixel-change hashing), libproc, footprint, \
                 and macmon. Android runs use SurfaceFlinger --latency, /proc, dumpsys meminfo, \
                 sysfs clocks, and thermalservice.",
                Modifier::empty(),
                text_style(12.5, palette.muted, None),
            );
            Text(
                "The ESP32 firmware counts completed frames each second. The emulator previews the demo layout.",
                Modifier::empty(),
                text_style(12.5, palette.muted, None),
            );
            if expanded.get() {
                ExpandedDetails(palette);
            }
        },
    );
}

#[composable]
pub(super) fn LayoutLinks(palette: Palette) {
    let active = local_active_tab().current();
    FlowRow(
        Modifier::empty().fill_max_width(),
        FlowSpec::default()
            .main_axis_spacing(6.0)
            .cross_axis_spacing(6.0),
        move || {
            if let Some(active) = active {
                for tab in [DemoTab::Gauntlet, DemoTab::StillWall, DemoTab::Patchwork] {
                    ActionChip(palette, tab.label(), ChipStyle::Plain, move || {
                        active.set(tab);
                    });
                }
            }
            LinkChip(
                palette,
                "ESP32 demo in browser",
                "https://samoylenkodmitry.github.io/cranpose-esp32/",
                ChipStyle::Plain,
            );
        },
    );
}

#[composable]
fn ExpandedDetails(palette: Palette) {
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(6.0)),
        move || {
            DetailLine(
                palette,
                "macOS desktop & browser: ",
                "FrameCount.app captures each window externally and hashes pixels at 1/8 scale so \
                 only changed frames count. CPU includes child processes, WebKit services, and \
                 WindowServer.",
            );
            DetailLine(
                palette,
                "Android native & browser: ",
                "perf_window.sh polls SurfaceFlinger --latency on the device for actual present \
                 timestamps and reads /proc, meminfo, devfreq, and thermalservice.",
            );
            DetailLine(
                palette,
                "Reading the numbers: ",
                "Every leg runs for a fixed span from launch (startup included) on a 60 Hz \
                 display. When top frameworks saturate 60 fps, CPU ms per frame separates \
                 steady-state cost.",
            );
            FlowRow(
                Modifier::empty().fill_max_width(),
                FlowSpec::default()
                    .main_axis_spacing(6.0)
                    .cross_axis_spacing(6.0),
                move || {
                    for (label, path) in HARNESS_LINKS {
                        let url = format!("{SOURCE_URL}/main/{path}");
                        LinkChip(palette, label, &url, ChipStyle::Plain);
                    }
                },
            );
        },
    );
}

#[composable]
fn DetailLine(palette: Palette, heading: &'static str, body: &'static str) {
    Text(
        format!("{heading}{body}"),
        Modifier::empty(),
        text_style(12.0, palette.muted, None),
    );
}

#[composable]
fn LinkChip(palette: Palette, label: &'static str, url: &str, style: ChipStyle) {
    let uri_handler = local_uri_handler().current();
    let target = url.to_string();
    ActionChip(palette, label, style, move || {
        if let Err(error) = uri_handler.open_uri(&target) {
            log::error!("Could not open {target}: {error:#}");
        }
    });
}

#[composable]
fn ActionChip(
    palette: Palette,
    label: &'static str,
    style: ChipStyle,
    action: impl Fn() + Clone + 'static,
) {
    let primary = style == ChipStyle::Accent;
    Button(
        Modifier::empty()
            .background(if primary {
                palette.accent
            } else {
                palette.background
            })
            .rounded_corners(14.0)
            .padding_symmetric(10.0, 4.0),
        ButtonSpec::default(),
        action,
        move || {
            Text(
                label,
                Modifier::empty(),
                text_style(
                    12.0,
                    if primary {
                        Color::WHITE
                    } else {
                        palette.accent
                    },
                    primary.then_some(FontWeight::BOLD),
                ),
            );
        },
    );
}
