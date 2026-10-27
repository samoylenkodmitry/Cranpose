use std::rc::Rc;

use cranpose_app_shell::{AppShell, default_root_key};
use cranpose_render_wgpu::WgpuRenderer;

use super::event_loop::{ShellInputs, android_platform_env};

pub(super) fn create<F: FnMut() + 'static>(
    renderer: WgpuRenderer,
    inputs: &ShellInputs<'_, F>,
    width: u32,
    height: u32,
    density: f32,
) -> AppShell<WgpuRenderer> {
    let content_clone = Rc::clone(inputs.content);
    let density = density.max(f32::EPSILON);
    let platform_env = android_platform_env();
    // The frame loop runs idle prefetch passes while it waits for a frame
    // (`prefetch_while_waiting`): a lazy list leaves them the items
    // beyond its viewport that its scroll does not soon reach.
    cranpose_ui::set_idle_prefetch_passes(true);
    let mut shell = crate::android_host_window::with_android_host_window_registry(
        inputs.host_window_registry,
        || {
            AppShell::new_with_size_and_density(
                renderer,
                default_root_key(),
                move || {
                    platform_env.compose_root(|| content_clone.borrow_mut()());
                },
                (width, height),
                (width as f32 / density, height as f32 / density),
                density,
            )
        },
    );
    shell.set_semantics_enabled(true);
    crate::accessibility::install_inspector(&mut shell, inputs.settings.developer_inspector);
    shell.set_safe_area(android_platform_env().safe_area());
    let tv =
        android_platform_env().configuration().ui_mode_type == cranpose_ui::UiModeType::Television;
    shell.set_dpad_navigation(tv);
    shell
}
