fn web_key_modifiers(event: &web_sys::KeyboardEvent) -> cranpose_app_shell::Modifiers {
    cranpose_app_shell::Modifiers {
        shift: event.shift_key(),
        ctrl: event.ctrl_key(),
        alt: event.alt_key(),
        meta: event.meta_key(),
    }
}

pub(super) fn web_key_event_prefix(
    event: &web_sys::KeyboardEvent,
) -> Option<(cranpose_app_shell::Key, cranpose_app_shell::Modifiers)> {
    if event.is_composing() || event.key_code() == 229 {
        return None;
    }
    Some((
        cranpose_app_shell::Key::from_dom_event(&event.code(), &event.key(), event.key_code()),
        web_key_modifiers(event),
    ))
}

// Hands a key going down to `shell`, and answers whether the page takes it
// from the browser.
pub(super) fn key_down_taken(
    shell: &mut cranpose_app_shell::AppShell<cranpose_render_wgpu::WgpuRenderer>,
    event: &cranpose_app_shell::KeyEvent,
) -> bool {
    shell.on_key_event(event) || take_back_key(event.key())
}

// A TV remote's Back that no handler took goes to the `BackHandler`s when
// one is composed, as Android's Back key does, and to the browser otherwise.
fn take_back_key(key: cranpose_app_shell::Key) -> bool {
    if key != cranpose_app_shell::Key::Back || !cranpose_services::back_interception_enabled() {
        return false;
    }
    cranpose_services::push_back_request();
    true
}
