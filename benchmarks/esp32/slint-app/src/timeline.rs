// The values the dashboard shows at `t` milliseconds after its first frame,
// as crates/cranpose/tests/support/dashboard.rs computes them, for the
// properties the Slint file takes from the app: its texts, its line and the
// list's offset. The bindings in ui/dashboard.slint compute the rest.

use std::{f32::consts::TAU, fmt::Write};

use crate::Dashboard;

const POINTS: usize = 32;

fn wave(t: u64, period: u64, phase: f32) -> f32 {
    (TAU * (t % period) as f32 / period as f32 + phase).sin()
}

fn line(t: u64, index: usize) -> f32 {
    0.5 + 0.35 * wave(t, 2500, index as f32 * 0.35)
}

fn gauge_percent(t: u64) -> u64 {
    ((0.5 + 0.45 * wave(t, 4000, 0.0)) * 100.0 + 0.5) as u64
}

/// The values set last, so that each property is set only when it changes,
/// as an app sets its data.
#[derive(Default)]
pub struct Shown {
    seconds: Option<u64>,
    cpu: Option<u64>,
    temperature: Option<u64>,
    network: Option<u64>,
    percent: Option<u64>,
    line: String,
}

impl Shown {
    /// Sets the properties of `ui` that changed since the last call, for the
    /// chart's size `width` by `height` in pixels.
    pub fn update(&mut self, ui: &Dashboard, t: u64, width: f32, height: f32) {
        ui.set_t(t as i32);
        let seconds = t / 1000;
        if self.seconds.replace(seconds) != Some(seconds) {
            ui.set_clock(format!("{:02}:{:02}", seconds / 60 % 60, seconds % 60).into());
        }
        let cpu = 20 + (t / 200 * 37) % 61;
        if self.cpu.replace(cpu) != Some(cpu) {
            ui.set_cpu(format!("{cpu}%").into());
        }
        let temperature = 360 + (t / 330 * 7) % 30;
        if self.temperature.replace(temperature) != Some(temperature) {
            ui.set_temperature(format!("{}.{} C", temperature / 10, temperature % 10).into());
        }
        let network = 5 + (t / 500 * 13) % 90;
        if self.network.replace(network) != Some(network) {
            ui.set_network(format!("{}.{} MB/s", network / 10, network % 10).into());
        }
        let percent = gauge_percent(t);
        if self.percent.replace(percent) != Some(percent) {
            ui.set_percent(format!("{percent}%").into());
        }
        self.line.clear();
        let step = width / (POINTS - 1) as f32;
        for index in 0..POINTS {
            let command = if index == 0 { 'M' } else { 'L' };
            let x = index as f32 * step;
            let y = height * (1.0 - line(t, index));
            let _ = write!(self.line, "{command} {x:.1} {y:.1} ");
        }
        ui.set_line_commands(self.line.as_str().into());
        ui.set_scroll(((t * 60 / 1000 % 1000) as f32).into());
    }
}
