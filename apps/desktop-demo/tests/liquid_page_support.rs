use std::rc::Rc;

use cranpose_app_shell::AppShell;
use cranpose_render_wgpu::{CapturedFrame, RenderStatsSnapshot, WgpuRenderer};
use cranpose_ui::{AppContext, ScrollState};
use desktop_app::app::{self, DemoTab, StartupSelection, TEST_LIQUID_SCROLL_STATE};

#[path = "../../../crates/cranpose-render/wgpu/tests/support/device.rs"]
mod gpu_test_device;

/// The demo showing its Liquid tab headless, at a logical size and density,
/// with its list's scroll state in hand.
pub struct LiquidPage {
    pub shell: AppShell<WgpuRenderer>,
    scroll: ScrollState,
    context: Rc<AppContext>,
    logical: (u32, u32),
    density: f32,
}

pub fn physical(logical: u32, density: f32) -> u32 {
    (logical as f32 * density).ceil() as u32
}

/// A renderer on the headless GPU, or none without one.
pub fn headless_renderer(label: &'static str) -> Option<WgpuRenderer> {
    let device = gpu_test_device::HeadlessDevice::request(
        wgpu::Backends::all(),
        wgpu::Limits::default(),
        label,
    )
    .ok()?;
    let mut renderer = WgpuRenderer::new(desktop_app::fonts::DEMO_FONTS);
    device.attach(&mut renderer, wgpu::TextureFormat::Bgra8UnormSrgb);
    Some(renderer)
}

impl LiquidPage {
    /// Opens the Liquid tab and settles two updates; none without a GPU.
    pub fn open(label: &'static str, logical: (u32, u32), density: f32) -> Option<Self> {
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
        let context = shell.app_context().clone();
        Some(Self {
            shell,
            scroll,
            context,
            logical,
            density,
        })
    }

    pub fn physical_size(&self) -> (u32, u32) {
        (
            physical(self.logical.0, self.density),
            physical(self.logical.1, self.density),
        )
    }

    /// Updates the page and renders it.
    pub fn capture(&mut self) -> CapturedFrame {
        self.shell.update();
        let (width, height) = self.physical_size();
        self.shell
            .renderer()
            .capture_frame_with_scale(width, height, self.density)
            .expect("liquid page capture should succeed")
    }

    pub fn stats(&mut self) -> RenderStatsSnapshot {
        self.shell
            .renderer()
            .last_frame_stats()
            .expect("the renderer publishes frame statistics")
    }

    pub fn scroll_offset(&self) -> f32 {
        self.scroll.value_non_reactive()
    }

    pub fn scroll_to(&self, offset: f32) {
        self.context.enter(|| self.scroll.scroll_to(offset));
    }

    /// Scrolls the list by one physical pixel, returning the logical delta
    /// the page consumed.
    pub fn step_one_pixel(&self) -> f32 {
        self.context
            .enter(|| self.scroll.dispatch_raw_delta(1.0 / self.density))
    }

    pub fn device_errors(&mut self) -> u64 {
        self.shell.renderer().device_error_count_for_tests()
    }
}
