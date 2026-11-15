use std::rc::Rc;

use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_render_wgpu::WgpuRenderer;

use crate::native_window::{NativeWindowRegistry, with_native_window_registry};

// The shell of an app shown in a window: a desktop window or an embedding
// host's view. It registers the windows it opens in `registry`, keeps the
// semantics tree the platform's accessibility bridge reads and copies through
// the system clipboard.
pub(crate) fn window_shell(
    renderer: WgpuRenderer,
    registry: &Rc<NativeWindowRegistry>,
    content: impl FnMut() + 'static,
    buffer_size: (u32, u32),
    viewport: (f32, f32),
    scale: f32,
) -> AppShell<WgpuRenderer> {
    let mut shell = with_native_window_registry(registry, || {
        AppShell::new_with_size_and_density(
            renderer,
            default_root_key(),
            content,
            buffer_size,
            viewport,
            scale,
        )
    });
    shell.set_semantics_enabled(true);
    shell.use_system_clipboard();
    shell
}
