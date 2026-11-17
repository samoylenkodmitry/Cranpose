// The bench app in the desktop apps' window on the wgpu renderer without a
// GPU: what `first_screen` and `steady_frames` start and step.

use std::time::Duration;

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::WgpuRenderer;
use perf_compare::PerfCompareApp;

const FRAME: Duration = Duration::from_nanos(16_666_667);
const FACES: [&[u8]; 3] = [
    include_bytes!("../../../fonts/Roboto-Regular.ttf"),
    include_bytes!("../../../fonts/Roboto-Medium.ttf"),
    include_bytes!("../../../fonts/Roboto-Bold.ttf"),
];

pub(crate) fn arg(name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    std::env::args().find_map(|arg| arg.strip_prefix(&prefix).map(str::to_owned))
}

pub(crate) fn arg_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    arg(name)
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

// The 1280 by 820 point window of the desktop apps at a density.
#[derive(Clone, Copy)]
pub(crate) struct DesktopWindow {
    size: (u32, u32),
    viewport: (f32, f32),
    density: f32,
}

impl DesktopWindow {
    pub(crate) fn new(density: f32) -> Self {
        let (width, height) = perf_data::DESKTOP_WINDOW;
        let viewport = (width as f32, height as f32);
        let size = (
            (viewport.0 * density).round() as u32,
            (viewport.1 * density).round() as u32,
        );
        Self {
            size,
            viewport,
            density,
        }
    }

    // The app's start: the shell with the app composed in it, and its first
    // frame.
    pub(crate) fn start(self) -> AppShell<WgpuRenderer> {
        let mut shell = AppShell::new_with_size_and_density(
            WgpuRenderer::new(&FACES),
            cranpose_core::location_key(file!(), line!(), column!()),
            PerfCompareApp,
            self.size,
            self.viewport,
            self.density,
        );
        self.frame(&mut shell);
        shell
    }

    // One frame a 60 Hz display shows: the update, then the frame packet
    // built and given back, as a phone's UI thread hands it to its GPU.
    pub(crate) fn frame(self, shell: &mut AppShell<WgpuRenderer>) {
        shell.update_after_exact_interval(FRAME);
        let renderer = shell.renderer();
        if let Some(packet) = renderer.build_frame_packet_for_tests(self.size.0, self.size.1) {
            renderer.return_held_packet_for_tests(packet);
        }
    }
}
