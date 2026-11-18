// The checks of a layer's color filter on text, run through the wgpu
// renderer.

use std::{rc::Rc, sync::MutexGuard};

use cranpose_app_shell::AppShell;
use cranpose_core::location_key;
use cranpose_render_common::Renderer;
use cranpose_render_wgpu::WgpuRenderer;

use crate::support;

#[expect(
    dead_code,
    reason = "wgpu leaves out the parent layer check: it drops a parent's filter on children drawn in place"
)]
#[path = "../../common/tests/support/filtered_word.rs"]
mod filtered_word;

use filtered_word::{FilteredFrames, HEIGHT, WIDTH};

struct WgpuFrames {
    shell: AppShell<WgpuRenderer>,
    _context: Rc<cranpose_ui::AppContext>,
    _lock: MutexGuard<'static, ()>,
}

impl FilteredFrames for WgpuFrames {
    fn compose(content: impl FnMut() + 'static) -> Self {
        let (lock, mut renderer) = support::headless_renderer_parts().expect("text renderer");
        let context = cranpose_ui::AppContext::new();
        renderer.attach_app_context_services(&context);
        let shell = AppShell::new_with_size(
            renderer,
            location_key(file!(), line!(), column!()),
            content,
            (WIDTH, HEIGHT),
            (WIDTH as f32, HEIGHT as f32),
        );
        Self {
            shell,
            _context: context,
            _lock: lock,
        }
    }

    fn rgba(&mut self) -> Vec<u8> {
        self.shell.update();
        self.shell.update();
        let frame = self
            .shell
            .renderer()
            .capture_frame(WIDTH, HEIGHT)
            .expect("text frame");
        frame.pixels
    }
}

#[test]
fn a_layer_color_filter_reaches_text_that_has_its_own_color() {
    filtered_word::reaches_text_that_has_its_own_color::<WgpuFrames>();
}

#[test]
fn a_layer_color_filter_reaches_stroked_text_that_has_its_own_color() {
    filtered_word::reaches_stroked_text_that_has_its_own_color::<WgpuFrames>();
}

#[test]
fn a_changed_layer_color_filter_repaints_the_color_of_a_span() {
    filtered_word::repaints_the_color_of_a_span::<WgpuFrames>();
}
