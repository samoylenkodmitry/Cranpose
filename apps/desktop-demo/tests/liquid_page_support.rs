use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::ScrollState;
use desktop_app::app::{self, DemoTab, StartupSelection, TEST_LIQUID_SCROLL_STATE};

use crate::gpu_test_device;

pub(crate) struct LiquidPage {
    pub shell: AppShell<WgpuRenderer>,
    scroll: ScrollState,
    logical: (u32, u32),
    density: f32,
}

pub(crate) fn physical(logical: u32, density: f32) -> u32 {
    (logical as f32 * density).ceil() as u32
}

pub(crate) fn headless_renderer(label: &'static str) -> Option<WgpuRenderer> {
    let device = gpu_test_device::HeadlessDevice::request(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        label,
    )
    .ok()?;
    let mut renderer = WgpuRenderer::new(desktop_app::fonts::DEMO_FONTS);
    device.attach(
        &mut renderer,
        wgpu::TextureFormat::Bgra8UnormSrgb,
        gpu_test_device::Pipelines::Inline,
    );
    Some(renderer)
}

impl LiquidPage {
    pub(crate) fn open(label: &'static str, logical: (u32, u32), density: f32) -> Option<Self> {
        let renderer = headless_renderer(label)?;
        let root_key = cranpose_core::location_key(file!(), line!(), column!());
        let mut shell = AppShell::new_with_size_and_density(
            renderer,
            root_key,
            || {
                app::combined_app_with_startup(StartupSelection {
                    initial_tab: Some(DemoTab::Liquid),
                    initial_shader_section: None,
                });
            },
            (physical(logical.0, density), physical(logical.1, density)),
            (logical.0 as f32, logical.1 as f32),
            density,
        );
        shell.update();
        shell.update();
        let scroll = TEST_LIQUID_SCROLL_STATE
            .with(|cell| *cell.borrow())
            .expect("the liquid page installs its scroll state");
        Some(Self {
            shell,
            scroll,
            logical,
            density,
        })
    }

    pub(crate) fn physical_size(&self) -> (u32, u32) {
        (
            physical(self.logical.0, self.density),
            physical(self.logical.1, self.density),
        )
    }

    pub(crate) fn capture(&mut self) -> CapturedFrame {
        self.shell.update();
        self.capture_current()
    }

    pub(crate) fn capture_current(&mut self) -> CapturedFrame {
        let (width, height) = self.physical_size();
        self.shell
            .renderer()
            .capture_frame_with_scale(width, height, self.density)
            .expect("liquid page capture should succeed")
    }

    pub(crate) fn stats(&mut self) -> RenderStatsSnapshot {
        self.shell
            .renderer()
            .last_frame_stats()
            .expect("the renderer publishes frame statistics")
    }

    pub(crate) fn scroll_offset(&self) -> f32 {
        self.scroll.value_non_reactive()
    }

    pub(crate) fn scroll_to(&self, offset: f32) {
        self.shell
            .app_context()
            .enter(|| self.scroll.scroll_to(offset));
    }

    pub(crate) fn step_one_pixel(&self) -> f32 {
        self.shell
            .app_context()
            .enter(|| self.scroll.dispatch_raw_delta(1.0 / self.density))
    }

    pub(crate) fn device_errors(&mut self) -> u64 {
        self.shell.renderer().device_error_count_for_tests()
    }
}
